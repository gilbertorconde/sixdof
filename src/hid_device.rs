//! The device read directly over USB HID, where no daemon stands between it
//! and the application: Windows, and macOS without spacenavd.
//!
//! The first multi-axis controller found is opened. On macOS it is opened
//! shared, so the vendor's own driver, when installed, keeps reading it too.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use hidapi::{HidApi, HidDevice};

use crate::hid::{Decoder, device_info, is_puck};
use crate::{DeviceInfo, Error, Event};

/// The longest report a device sends, with its id.
const REPORT_BYTES: usize = 64;

fn hid_error(err: hidapi::HidError) -> Error {
    Error::Io(std::io::Error::other(err.to_string()))
}

/// A connection to a device over USB HID.
pub struct Client {
    device: HidDevice,
    info: DeviceInfo,
    decoder: Decoder,
    pending: VecDeque<Event>,
}

impl std::fmt::Debug for Client {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Client").field("info", &self.info).finish()
    }
}

impl Client {
    /// Opens the first 6-degree-of-freedom device plugged in.
    ///
    /// Fails with [`Error::Refused`] when there is none: treat that as an
    /// ordinary state and retry.
    pub fn connect() -> Result<Client, Error> {
        let api = HidApi::new().map_err(hid_error)?;
        // Shared, so the vendor's driver keeps the device too.
        #[cfg(target_os = "macos")]
        api.set_open_exclusive(false);
        let found = api
            .device_list()
            .find(|d| is_puck(d.vendor_id(), d.product_id(), d.usage_page(), d.usage()))
            .ok_or(Error::Refused)?;
        let device = found.open_device(&api).map_err(hid_error)?;
        let mut descriptor = [0u8; hidapi::MAX_REPORT_DESCRIPTOR_SIZE];
        let length = device.get_report_descriptor(&mut descriptor).unwrap_or(0);
        let decoder =
            Decoder::for_device(found.vendor_id(), found.product_id(), &descriptor[..length]);
        let name = found
            .product_string()
            .map(str::to_string)
            .unwrap_or_else(|| "6-DoF device".to_string());
        let info = device_info(
            &name,
            &found.path().to_string_lossy(),
            found.vendor_id(),
            found.product_id(),
        );
        let mut pending = VecDeque::new();
        pending.push_back(Event::Device {
            added: true,
            id: 0,
            device_type: 0,
            usb_id: info.usb_id,
        });
        Ok(Client {
            device,
            info,
            decoder,
            pending,
        })
    }

    /// What is known about the device.
    #[must_use]
    pub fn device(&self) -> Option<&DeviceInfo> {
        Some(&self.info)
    }

    /// Scales the axis readings.
    pub fn set_sensitivity(&mut self, sensitivity: f32) -> Result<(), Error> {
        self.decoder.sensitivity = sensitivity;
        Ok(())
    }

    /// Waits for the next event.
    pub fn read_blocking(&mut self) -> Result<Event, Error> {
        loop {
            if let Some(event) = self.read_for(-1)? {
                return Ok(event);
            }
        }
    }

    /// Waits for the next event, giving up after `timeout`.
    pub fn read_timeout(&mut self, timeout: Duration) -> Result<Option<Event>, Error> {
        let deadline = Instant::now() + timeout;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            let ms = i32::try_from(left.as_millis()).unwrap_or(i32::MAX);
            if let Some(event) = self.read_for(ms)? {
                return Ok(Some(event));
            }
            if Instant::now() >= deadline {
                return Ok(None);
            }
        }
    }

    /// Takes the next event if one is already waiting.
    pub fn poll(&mut self) -> Result<Option<Event>, Error> {
        self.read_for(0)
    }

    /// One report, waiting up to `ms` milliseconds for it (forever when
    /// negative), and the first event it makes, the rest kept for later.
    fn read_for(&mut self, ms: i32) -> Result<Option<Event>, Error> {
        if let Some(event) = self.pending.pop_front() {
            return Ok(Some(event));
        }
        let mut buf = [0u8; REPORT_BYTES];
        let read = self
            .device
            .read_timeout(&mut buf, ms)
            .map_err(|_| Error::Disconnected)?;
        if read > 0 {
            self.decoder
                .feed(&buf[..read], Instant::now(), &mut self.pending);
        }
        Ok(self.pending.pop_front())
    }
}
