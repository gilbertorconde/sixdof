//! Reading a device's own USB HID reports, where no daemon stands between
//! it and the application (Windows, macOS without spacenavd).
//!
//! The reports are the ones every 3Dconnexion device sends, and other
//! makers' pucks copy:
//!
//! - report 1: the push along x, y and z, three little-endian `i16`s; the
//!   newer devices send the three twists after them in the same report;
//! - report 2: the twists about x, y and z, on the older devices;
//! - report 3: the buttons, one bit each, lowest first.
//!
//! The axes come through as the device gives them, which is what spacenavd
//! passes on too, so an application sees the same numbers either way.

use std::collections::VecDeque;
use std::time::Instant;

use crate::{DeviceInfo, Event, Motion};

/// Vendors whose 6-degree-of-freedom devices speak these reports.
pub(crate) const VENDORS: [u16; 2] = [
    0x046d, // the Logitech-era 3Dconnexion devices
    0x256f, // 3Dconnexion
];

/// The HID usage of a multi-axis controller (generic desktop page).
pub(crate) const USAGE_PAGE: u16 = 0x01;
/// The HID usage of a multi-axis controller (generic desktop page).
pub(crate) const USAGE: u16 = 0x08;

/// Whether a HID interface is a 6-degree-of-freedom device: a multi-axis
/// controller, or failing a usage (some systems do not report one), a
/// 3Dconnexion product.
pub(crate) fn is_puck(vendor: u16, product: u16, usage_page: u16, usage: u16) -> bool {
    if usage_page == USAGE_PAGE && usage == USAGE {
        return true;
    }
    usage_page == 0 && VENDORS.contains(&vendor) && (product & 0xff00) == 0xc600
}

/// How many buttons a device has, by its product, when known.
pub(crate) fn button_count(vendor: u16, product: u16) -> u32 {
    if !VENDORS.contains(&vendor) {
        return 0;
    }
    match product {
        0xc626 | 0xc628 | 0xc62e | 0xc62f | 0xc635 | 0xc652 => 2,
        0xc627 => 15,
        0xc625 => 21,
        0xc629 => 31,
        0xc62b | 0xc631 | 0xc632 => 15,
        0xc633 => 31,
        _ => 0,
    }
}

/// What is known of a device from its HID descriptor.
pub(crate) fn device_info(name: &str, path: &str, vendor: u16, product: u16) -> DeviceInfo {
    DeviceInfo {
        name: name.to_string(),
        path: Some(path.to_string()),
        axes: 6,
        buttons: button_count(vendor, product),
        usb_id: Some((vendor, product)),
        device_type: 0,
    }
}

/// The state a device's reports build up: where the puck is, which
/// buttons are down, and when the last motion was.
#[derive(Debug, Default)]
pub(crate) struct Decoder {
    translate: [i32; 3],
    rotate: [i32; 3],
    buttons: u64,
    last_motion: Option<Instant>,
    /// Scales every axis reading.
    pub sensitivity: f32,
}

impl Decoder {
    pub fn new() -> Self {
        Self {
            sensitivity: 1.0,
            ..Self::default()
        }
    }

    /// Takes one report (its id first), leaving the events it makes in
    /// `out`: a motion for the axes, a button event for each button that
    /// changed. Reports of other kinds are passed over.
    pub fn feed(&mut self, report: &[u8], now: Instant, out: &mut VecDeque<Event>) {
        let Some((&id, data)) = report.split_first() else {
            return;
        };
        let axes = |bytes: &[u8]| -> [i32; 3] {
            [0, 1, 2].map(|i| i32::from(i16::from_le_bytes([bytes[2 * i], bytes[2 * i + 1]])))
        };
        match id {
            1 if data.len() >= 12 => {
                self.translate = axes(&data[..6]);
                self.rotate = axes(&data[6..12]);
                out.push_back(self.motion(now));
            }
            1 if data.len() >= 6 => {
                self.translate = axes(&data[..6]);
                out.push_back(self.motion(now));
            }
            2 if data.len() >= 6 => {
                self.rotate = axes(&data[..6]);
                out.push_back(self.motion(now));
            }
            3 => {
                let mut now_down = 0u64;
                for (i, byte) in data.iter().take(8).enumerate() {
                    now_down |= u64::from(*byte) << (8 * i);
                }
                let changed = now_down ^ self.buttons;
                for index in 0..64 {
                    if changed & (1 << index) != 0 {
                        out.push_back(Event::Button {
                            index,
                            pressed: now_down & (1 << index) != 0,
                        });
                    }
                }
                self.buttons = now_down;
            }
            _ => {}
        }
    }

    /// The puck as it stands, scaled, and the time since the last motion.
    fn motion(&mut self, now: Instant) -> Event {
        let period_ms = self
            .last_motion
            .map_or(0, |then| now.duration_since(then).as_millis() as u32);
        self.last_motion = Some(now);
        let scale = |v: i32| (v as f32 * self.sensitivity).round() as i32;
        Event::Motion(Motion {
            translate: self.translate.map(scale),
            rotate: self.rotate.map(scale),
            period_ms,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn report(id: u8, values: &[i16]) -> Vec<u8> {
        let mut out = vec![id];
        for v in values {
            out.extend_from_slice(&v.to_le_bytes());
        }
        out
    }

    fn events(decoder: &mut Decoder, report: &[u8]) -> Vec<Event> {
        let mut out = VecDeque::new();
        decoder.feed(report, Instant::now(), &mut out);
        out.into()
    }

    fn motion(event: &Event) -> Motion {
        match event {
            Event::Motion(m) => *m,
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn older_devices_send_the_push_and_the_twist_apart() {
        let mut decoder = Decoder::new();
        let pushed = events(&mut decoder, &report(1, &[10, -20, 300]));
        assert_eq!(motion(&pushed[0]).translate, [10, -20, 300]);
        assert_eq!(motion(&pushed[0]).rotate, [0, 0, 0]);
        let twisted = events(&mut decoder, &report(2, &[-5, 6, -350]));
        let m = motion(&twisted[0]);
        assert_eq!((m.translate, m.rotate), ([10, -20, 300], [-5, 6, -350]));
    }

    #[test]
    fn newer_devices_send_all_six_at_once() {
        let mut decoder = Decoder::new();
        let got = events(&mut decoder, &report(1, &[1, 2, 3, 4, 5, 6]));
        let m = motion(&got[0]);
        assert_eq!((m.translate, m.rotate), ([1, 2, 3], [4, 5, 6]));
    }

    #[test]
    fn a_button_report_tells_each_change_once() {
        let mut decoder = Decoder::new();
        assert_eq!(
            events(&mut decoder, &[3, 0b0000_0001, 0]),
            [Event::Button {
                index: 0,
                pressed: true
            }]
        );
        assert_eq!(
            events(&mut decoder, &[3, 0b0000_0010, 0x01]),
            [
                Event::Button {
                    index: 0,
                    pressed: false
                },
                Event::Button {
                    index: 1,
                    pressed: true
                },
                Event::Button {
                    index: 8,
                    pressed: true
                },
            ]
        );
        assert!(
            events(&mut decoder, &[3, 0b0000_0010, 0x01]).is_empty(),
            "nothing changed"
        );
    }

    #[test]
    fn sensitivity_scales_the_axes_and_short_or_other_reports_pass() {
        let mut decoder = Decoder::new();
        decoder.sensitivity = 0.5;
        let got = events(&mut decoder, &report(1, &[100, -100, 7, 0, 0, 0]));
        assert_eq!(motion(&got[0]).translate, [50, -50, 4]);
        assert!(events(&mut decoder, &[1, 0, 0]).is_empty());
        assert!(events(&mut decoder, &[0x17, 1, 2, 3]).is_empty());
        assert!(events(&mut decoder, &[]).is_empty());
    }

    #[test]
    fn pucks_are_known_by_their_usage_or_their_maker() {
        assert!(is_puck(0x1234, 0x0001, USAGE_PAGE, USAGE));
        assert!(is_puck(0x256f, 0xc635, 0, 0));
        assert!(!is_puck(0x256f, 0xc635, 0x01, 0x02), "its mouse interface");
        assert!(!is_puck(0x046d, 0xc077, 0x01, 0x02), "a Logitech mouse");
        assert_eq!(button_count(0x256f, 0xc635), 2);
        let info = device_info("SpaceMouse Compact", "hid:0", 0x256f, 0xc635);
        assert_eq!((info.axes, info.buttons), (6, 2));
        assert_eq!(info.usb_id, Some((0x256f, 0xc635)));
    }
}
