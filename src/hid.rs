//! Reading a device's own USB HID reports, where no daemon stands between
//! it and the application.
//!
//! The reports are the ones every 3Dconnexion device sends, and other
//! makers' pucks copy:
//!
//! - report 1: the push along x, y and z, three little-endian `i16`s; the
//!   newer devices send the three twists after them in the same report;
//! - report 2: the twists about x, y and z, on the older devices;
//! - report 3: the buttons, one bit each, lowest first.
//!
//! An application sees the numbers spacenavd would give it: each axis taken
//! from the range the device declares onto ±500, y and z swapped and turned
//! on the models whose axes run that way, and the buttons of the models that
//! number them sparsely counted from 0. Devices repeat their reports many
//! hundred times a second whether the puck moves or not; a motion is told
//! only when the axes changed, as the daemon does.

use std::collections::VecDeque;
use std::time::Instant;

use crate::{DeviceInfo, Event, Motion};

/// Vendors whose 6-degree-of-freedom devices speak these reports.
pub(crate) const VENDORS: [u16; 2] = [LOGITECH, CONNEXION];
/// The Logitech-era 3Dconnexion devices.
const LOGITECH: u16 = 0x046d;
/// 3Dconnexion.
const CONNEXION: u16 = 0x256f;

/// The HID usage of a multi-axis controller (generic desktop page).
pub(crate) const USAGE_PAGE: u16 = 0x01;
/// The HID usage of a multi-axis controller (generic desktop page).
pub(crate) const USAGE: u16 = 0x08;

/// The range every axis is taken onto.
const RANGE: (i32, i32) = (-500, 500);

/// Whether a HID interface is a 6-degree-of-freedom device: a multi-axis
/// controller, or failing a usage (some systems do not report one), a
/// 3Dconnexion product.
pub(crate) fn is_puck(vendor: u16, product: u16, usage_page: u16, usage: u16) -> bool {
    if usage_page == USAGE_PAGE && usage == USAGE {
        return true;
    }
    usage_page == 0 && VENDORS.contains(&vendor) && (product & 0xff00) == 0xc600
}

/// Whether a model's y and z run the other way round from the older devices':
/// every 3Dconnexion-branded one, and most of the Logitech-era ones.
fn swaps_yz(vendor: u16, product: u16) -> bool {
    match vendor {
        CONNEXION => true,
        LOGITECH => matches!(
            product,
            0xc605 | 0xc621 | 0xc623 | 0xc625 | 0xc626 | 0xc627 | 0xc628 | 0xc629 | 0xc62b
        ),
        _ => false,
    }
}

/// A button numbering for the models whose report leaves gaps: the button a
/// report bit stands for, `None` for the bits that stand for nothing.
type ButtonMap = fn(u32) -> Option<u32>;

/// SpaceMouse Pro and Pro Wireless: the four view keys first, then the
/// rest, as the daemon numbers them.
fn pro_buttons(bit: u32) -> Option<u32> {
    Some(match bit {
        12..=15 => bit - 12, // 1 to 4
        0 => 4,              // menu
        1 => 5,              // fit
        2 => 6,              // top
        4 => 7,              // right
        5 => 8,              // front
        8 => 9,              // rotation lock
        26 => 10,            // lock
        22 => 11,            // esc
        23 => 12,            // alt
        24 => 13,            // shift
        25 => 14,            // ctrl
        _ => return None,
    })
}

/// SpaceMouse Enterprise.
fn enterprise_buttons(bit: u32) -> Option<u32> {
    Some(match bit {
        12..=21 => bit - 12,   // 1 to 10
        76 | 77 => bit - 66,   // 11, 12
        0 => 12,               // menu
        1 => 13,               // fit
        2 => 14,               // top
        4 => 15,               // right
        5 => 16,               // front
        8 => 17,               // rotation lock
        22 => 18,              // esc
        23 => 19,              // alt
        24 => 20,              // shift
        25 => 21,              // ctrl
        26 => 22,              // lock
        35 => 23,              // enter
        36 => 24,              // delete
        174 => 25,             // tab
        175 => 26,             // space
        102..=104 => bit - 75, // views 1 to 3
        10 => 30,              // iso
        _ => return None,
    })
}

fn button_map(vendor: u16, product: u16) -> Option<ButtonMap> {
    match (vendor, product) {
        (LOGITECH, 0xc62b) | (CONNEXION, 0xc631 | 0xc632) => Some(pro_buttons),
        (CONNEXION, 0xc633) => Some(enterprise_buttons),
        _ => None,
    }
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

/// The logical range a report descriptor declares for each of the six axes
/// (x, y, z, then the turns about them), where it declares one.
pub(crate) fn axis_ranges(descriptor: &[u8]) -> [Option<(i32, i32)>; 6] {
    let mut ranges = [None; 6];
    let (mut page, mut min, mut max) = (0u32, 0i32, 0i32);
    let mut usages: Vec<u32> = Vec::new();
    let mut span: Option<(u32, u32)> = None;
    let mut at = 0;
    while at < descriptor.len() {
        let prefix = descriptor[at];
        if prefix == 0xfe {
            // A long item: its size follows.
            let size = descriptor.get(at + 1).copied().unwrap_or(0) as usize;
            at += 3 + size;
            continue;
        }
        let size = [0, 1, 2, 4][usize::from(prefix & 3)];
        let Some(bytes) = descriptor.get(at + 1..at + 1 + size) else {
            break;
        };
        at += 1 + size;
        let unsigned = bytes
            .iter()
            .rev()
            .fold(0u32, |acc, b| (acc << 8) | u32::from(*b));
        let signed = match size {
            1 => i32::from(bytes[0] as i8),
            2 => i32::from(i16::from_le_bytes([bytes[0], bytes[1]])),
            4 => unsigned as i32,
            _ => 0,
        };
        match prefix & 0xfc {
            0x04 => page = unsigned,
            0x14 => min = signed,
            // A maximum reads as unsigned unless the minimum is negative.
            0x24 => max = if min < 0 { signed } else { unsigned as i32 },
            0x08 => usages.push(if size == 4 {
                unsigned
            } else {
                (page << 16) | unsigned
            }),
            0x18 => span = Some((unsigned, span.map_or(unsigned, |s| s.1))),
            0x28 => span = Some((span.map_or(unsigned, |s| s.0), unsigned)),
            0x80 => {
                if let Some((from, to)) = span {
                    usages.extend((from..=to).map(|u| (page << 16) | u));
                }
                for usage in &usages {
                    if usage >> 16 == u32::from(USAGE_PAGE) {
                        let axis = (usage & 0xffff).wrapping_sub(0x30) as usize;
                        if axis < 6 && max > min {
                            ranges[axis] = Some((min, max));
                        }
                    }
                }
                usages.clear();
                span = None;
            }
            0x90 | 0xb0 | 0xa0 | 0xc0 => {
                usages.clear();
                span = None;
            }
            _ => {}
        }
    }
    ranges
}

/// The state a device's reports build up: where the puck is, which
/// buttons are down, and when the last motion was.
#[derive(Debug)]
pub(crate) struct Decoder {
    /// The six axes as the device last gave them.
    raw: [i32; 6],
    /// The button report's bits as last given.
    buttons: Vec<u8>,
    last_motion: Option<Instant>,
    /// The axes the last motion told.
    told: ([i32; 3], [i32; 3]),
    /// Each axis's declared range, taken onto [`RANGE`].
    ranges: [(i32, i32); 6],
    swap_yz: bool,
    button_map: Option<ButtonMap>,
    /// Scales every axis reading.
    pub sensitivity: f32,
}

impl Decoder {
    /// A decoder passing the device's numbers on as they come.
    pub fn new() -> Self {
        Self {
            raw: [0; 6],
            buttons: Vec::new(),
            last_motion: None,
            told: ([0; 3], [0; 3]),
            ranges: [RANGE; 6],
            swap_yz: false,
            button_map: None,
            sensitivity: 1.0,
        }
    }

    /// A decoder for one model, its axes taken from the ranges its report
    /// descriptor declares (the device's own numbers where it declares none).
    pub fn for_device(vendor: u16, product: u16, descriptor: &[u8]) -> Self {
        let declared = axis_ranges(descriptor);
        Self {
            ranges: declared.map(|range| range.unwrap_or(RANGE)),
            swap_yz: swaps_yz(vendor, product),
            button_map: button_map(vendor, product),
            ..Self::new()
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
                self.raw[..3].copy_from_slice(&axes(&data[..6]));
                self.raw[3..].copy_from_slice(&axes(&data[6..12]));
                out.extend(self.motion(now));
            }
            1 if data.len() >= 6 => {
                self.raw[..3].copy_from_slice(&axes(&data[..6]));
                out.extend(self.motion(now));
            }
            2 if data.len() >= 6 => {
                self.raw[3..].copy_from_slice(&axes(&data[..6]));
                out.extend(self.motion(now));
            }
            3 => {
                let bytes = data.len().max(self.buttons.len());
                for bit in 0..bytes * 8 {
                    let byte = bit / 8;
                    let mask = 1 << (bit % 8);
                    let down = data.get(byte).is_some_and(|b| b & mask != 0);
                    let was = self.buttons.get(byte).is_some_and(|b| b & mask != 0);
                    if down == was {
                        continue;
                    }
                    let bit = bit as u32;
                    let index = match self.button_map {
                        Some(map) => map(bit),
                        None => Some(bit),
                    };
                    if let Some(index) = index {
                        out.push_back(Event::Button {
                            index,
                            pressed: down,
                        });
                    }
                }
                self.buttons = data.to_vec();
            }
            _ => {}
        }
    }

    /// The puck as it stands, in the daemon's terms, and the time since the
    /// last motion; nothing when that is what the last motion told.
    fn motion(&mut self, now: Instant) -> Option<Event> {
        let mut axes = [0i32; 6];
        for (i, value) in self.raw.iter().enumerate() {
            let (min, max) = self.ranges[i];
            let taken = i64::from(value - min) * i64::from(RANGE.1 - RANGE.0)
                / i64::from(max - min)
                + i64::from(RANGE.0);
            axes[i] = taken as i32;
        }
        if self.swap_yz {
            axes.swap(1, 2);
            axes.swap(4, 5);
            for i in [1, 2, 4, 5] {
                axes[i] = -axes[i];
            }
        }
        let scale = |v: i32| (v as f32 * self.sensitivity).round() as i32;
        let told = (
            [axes[0], axes[1], axes[2]].map(scale),
            [axes[3], axes[4], axes[5]].map(scale),
        );
        if told == self.told {
            return None;
        }
        self.told = told;
        let period_ms = self
            .last_motion
            .map_or(0, |then| now.duration_since(then).as_millis() as u32);
        self.last_motion = Some(now);
        Some(Event::Motion(Motion {
            translate: told.0,
            rotate: told.1,
            period_ms,
        }))
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
    fn a_repeated_report_tells_nothing_new() {
        let mut decoder = Decoder::new();
        assert!(
            events(&mut decoder, &report(1, &[0, 0, 0])).is_empty(),
            "at rest"
        );
        assert_eq!(events(&mut decoder, &report(2, &[0, 0, 9])).len(), 1);
        assert!(events(&mut decoder, &report(2, &[0, 0, 9])).is_empty());
        assert!(events(&mut decoder, &report(1, &[0, 0, 0])).is_empty());
        let back = events(&mut decoder, &report(2, &[0, 0, 0]));
        assert_eq!(motion(&back[0]).rotate, [0, 0, 0], "the return to rest");
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

    /// A SpaceMouse Pro Wireless's own report descriptor, as the device gives it.
    const PRO_WIRELESS: [u8; 86] = [
        0x05, 0x01, 0x09, 0x08, 0xa1, 0x01, 0xa1, 0x00, 0x85, 0x01, 0x16, 0x00, 0x80, 0x26, 0xff,
        0x7f, 0x36, 0x00, 0x80, 0x46, 0xff, 0x7f, 0x09, 0x30, 0x09, 0x31, 0x09, 0x32, 0x75, 0x10,
        0x95, 0x03, 0x81, 0x02, 0xc0, 0xa1, 0x00, 0x85, 0x02, 0x16, 0x00, 0x80, 0x26, 0xff, 0x7f,
        0x36, 0x00, 0x80, 0x46, 0xff, 0x7f, 0x09, 0x33, 0x09, 0x34, 0x09, 0x35, 0x75, 0x10, 0x95,
        0x03, 0x81, 0x02, 0xc0, 0xa1, 0x00, 0x85, 0x03, 0x15, 0x00, 0x25, 0x01, 0x75, 0x01, 0x95,
        0x20, 0x05, 0x09, 0x19, 0x01, 0x29, 0x20, 0x81, 0x02, 0xc0, 0xc0,
    ];

    #[test]
    fn the_descriptor_gives_each_axis_its_range() {
        assert_eq!(axis_ranges(&PRO_WIRELESS), [Some((-32768, 32767)); 6]);
        assert_eq!(axis_ranges(&[]), [None; 6]);
        assert_eq!(axis_ranges(&PRO_WIRELESS[..20]), [None; 6], "cut short");
    }

    #[test]
    fn axes_come_out_as_the_daemon_gives_them() {
        let mut decoder = Decoder::for_device(0x256f, 0xc631, &PRO_WIRELESS);
        // A twist about z, as captured from the device: onto ±500, then
        // y and z swapped and turned.
        let got = events(&mut decoder, &report(2, &[0, 0, -1158]));
        assert_eq!(motion(&got[0]).rotate, [0, 18, 0]);
        let got = events(&mut decoder, &report(1, &[32767, 32767, 0]));
        assert_eq!(motion(&got[0]).translate, [500, 0, -500]);
        // An older model keeps its axes, and a device declaring nothing
        // passes its numbers on.
        let mut decoder = Decoder::for_device(0x046d, 0xc603, &[]);
        let got = events(&mut decoder, &report(1, &[10, 20, 30]));
        assert_eq!(motion(&got[0]).translate, [10, 20, 30]);
    }

    #[test]
    fn sparse_buttons_are_counted_from_zero() {
        let mut decoder = Decoder::for_device(0x256f, 0xc631, &PRO_WIRELESS);
        // Key 1 is bit 12; bit 3 stands for nothing on this model.
        let got = events(&mut decoder, &[3, 0b0000_1000, 0b0001_0000, 0, 0]);
        assert_eq!(
            got,
            [Event::Button {
                index: 0,
                pressed: true
            }]
        );
        let got = events(&mut decoder, &[3, 0b0000_0001, 0, 0, 0b0000_0100]);
        assert_eq!(got.len(), 3, "{got:?}");
        assert!(got.contains(&Event::Button {
            index: 4,
            pressed: true
        }));
        assert!(got.contains(&Event::Button {
            index: 10,
            pressed: true
        }));
        // The Enterprise's space key sits far past the first 64 bits.
        let mut decoder = Decoder::for_device(0x256f, 0xc633, &[]);
        let mut report = vec![0u8; 23];
        report[0] = 3;
        report[1 + 175 / 8] = 1 << (175 % 8);
        assert_eq!(
            events(&mut decoder, &report),
            [Event::Button {
                index: 26,
                pressed: true
            }]
        );
    }
}
