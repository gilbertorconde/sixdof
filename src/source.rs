//! One connection, whichever route the device is reachable by.
//!
//! On Unix the daemon's socket is tried first: it carries device details
//! and settings, and it works with no display server at all. Failing that
//! (which is what running the vendor's own driver looks like) the Magellan
//! protocol over the display server is tried. On Windows, and on macOS when
//! neither answers, the device's own USB HID reports are read directly.

use std::time::Duration;

use crate::{DeviceInfo, Error, Event, EventMask};

/// Which route a [`Source`] took.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Backend {
    /// The daemon's UNIX socket: everything this crate offers.
    Daemon,
    /// The Magellan protocol over a display server: motion, buttons and
    /// device removal, and nothing else.
    Magellan,
    /// The device's own USB HID reports: motion, buttons and what the
    /// device says of itself.
    Hid,
}

impl std::fmt::Display for Backend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Backend::Daemon => write!(f, "daemon"),
            Backend::Magellan => write!(f, "display server"),
            Backend::Hid => write!(f, "USB HID"),
        }
    }
}

/// A device connection over whichever protocol answered.
pub enum Source {
    /// The daemon's socket: everything this crate offers.
    #[cfg(unix)]
    Daemon(crate::Client),
    /// Boxed because a display connection carries far more state than a
    /// socket does.
    #[cfg(all(unix, feature = "magellan"))]
    Magellan(Box<crate::magellan::Client>),
    /// The device read directly over USB HID.
    #[cfg(hid_route)]
    Hid(crate::hid_device::Client),
}

/// Runs `$body` with the connection whichever variant holds it.
macro_rules! each {
    ($source:expr, $client:ident => $body:expr) => {
        match $source {
            #[cfg(unix)]
            Source::Daemon($client) => $body,
            #[cfg(all(unix, feature = "magellan"))]
            Source::Magellan($client) => $body,
            #[cfg(hid_route)]
            Source::Hid($client) => $body,
        }
    };
}

impl Source {
    /// Connects by the best route available.
    ///
    /// The error returned is the first route's, since that is the one a
    /// machine without a device is expected to give.
    pub fn connect() -> Result<Source, Error> {
        #[cfg(unix)]
        let first = match crate::Client::connect() {
            Ok(client) => return Ok(Source::Daemon(client)),
            Err(err) => err,
        };
        #[cfg(all(unix, feature = "magellan"))]
        if let Ok(client) = crate::magellan::Client::connect() {
            return Ok(Source::Magellan(Box::new(client)));
        }
        #[cfg(hid_route)]
        {
            #[cfg(unix)]
            if let Ok(client) = crate::hid_device::Client::connect() {
                return Ok(Source::Hid(client));
            }
            #[cfg(not(unix))]
            return crate::hid_device::Client::connect().map(Source::Hid);
        }
        #[cfg(unix)]
        Err(first)
    }

    /// Which protocol this connection speaks.
    #[must_use]
    pub fn backend(&self) -> Backend {
        match self {
            #[cfg(unix)]
            Source::Daemon(_) => Backend::Daemon,
            #[cfg(all(unix, feature = "magellan"))]
            Source::Magellan(_) => Backend::Magellan,
            #[cfg(hid_route)]
            Source::Hid(_) => Backend::Hid,
        }
    }

    /// Names this client in the daemon's log. The other routes have no such
    /// notion and take it silently.
    pub fn set_name(&mut self, name: &str) -> Result<(), Error> {
        match self {
            #[cfg(unix)]
            Source::Daemon(client) => client.set_name(name),
            #[allow(unreachable_patterns)]
            _ => {
                let _ = name;
                Ok(())
            }
        }
    }

    /// Chooses which events arrive. The other routes carry motion, buttons
    /// and device changes and offer no choice about it.
    pub fn set_event_mask(&mut self, mask: EventMask) -> Result<(), Error> {
        match self {
            #[cfg(unix)]
            Source::Daemon(client) => client.set_event_mask(mask),
            #[allow(unreachable_patterns)]
            _ => {
                let _ = mask;
                Ok(())
            }
        }
    }

    /// Scales this application's readings, leaving other applications alone.
    pub fn set_sensitivity(&mut self, sensitivity: f32) -> Result<(), Error> {
        each!(self, client => client.set_sensitivity(sensitivity))
    }

    /// What is known about the device. The Magellan protocol tells an
    /// application nothing about it, so this is `None` there.
    #[must_use]
    pub fn device(&self) -> Option<&DeviceInfo> {
        match self {
            #[cfg(unix)]
            Source::Daemon(client) => client.device(),
            #[cfg(all(unix, feature = "magellan"))]
            Source::Magellan(_) => None,
            #[cfg(hid_route)]
            Source::Hid(client) => client.device(),
        }
    }

    /// Asks again what device is connected.
    pub fn refresh_device(&mut self) -> Result<Option<&DeviceInfo>, Error> {
        match self {
            #[cfg(unix)]
            Source::Daemon(client) => client.refresh_device(),
            #[cfg(all(unix, feature = "magellan"))]
            Source::Magellan(_) => Ok(None),
            #[cfg(hid_route)]
            Source::Hid(client) => Ok(client.device()),
        }
    }

    /// The daemon's own settings, when connected to one.
    #[cfg(unix)]
    pub fn config(&mut self) -> Option<crate::Config<'_>> {
        match self {
            Source::Daemon(client) => Some(client.config()),
            #[allow(unreachable_patterns)]
            _ => None,
        }
    }

    /// Waits for the next event.
    pub fn read_blocking(&mut self) -> Result<Event, Error> {
        each!(self, client => client.read_blocking())
    }

    /// Waits for the next event, giving up after `timeout`.
    pub fn read_timeout(&mut self, timeout: Duration) -> Result<Option<Event>, Error> {
        each!(self, client => client.read_timeout(timeout))
    }

    /// Takes the next event if one is already waiting.
    pub fn poll(&mut self) -> Result<Option<Event>, Error> {
        each!(self, client => client.poll())
    }

    /// The socket or display connection, for callers running their own
    /// readiness loop; `None` for a device read over USB HID, which has no
    /// descriptor to wait on.
    #[cfg(unix)]
    #[must_use]
    pub fn as_raw_fd(&self) -> Option<std::os::fd::RawFd> {
        match self {
            Source::Daemon(client) => Some(client.as_raw_fd()),
            #[cfg(feature = "magellan")]
            Source::Magellan(client) => Some(client.as_raw_fd()),
            #[cfg(hid_route)]
            Source::Hid(_) => None,
        }
    }
}
