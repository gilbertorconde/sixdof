# sixdof

[![Crates.io](https://img.shields.io/crates/v/sixdof.svg)](https://crates.io/crates/sixdof)
[![Documentation](https://docs.rs/sixdof/badge.svg)](https://docs.rs/sixdof)
[![License](https://img.shields.io/badge/license-MIT%2FApache--2.0-blue.svg)](#licence)

A small Rust client for 6-degree-of-freedom input devices (SpaceMouse,
SpaceNavigator, Spaceball and friends) on Linux, macOS and Windows.

On Unix it talks to [spacenavd], the free daemon that owns the device
(detection, calibration, dead zones, per-device configuration) and publishes
events on a UNIX socket. Where no daemon runs (Windows, macOS without
spacenavd) it reads the device's own USB HID reports.

```rust
// `Source::connect` takes whichever route answers on this platform.
let mut source = sixdof::Source::connect()?;
source.set_name("my-app")?;
source.set_event_mask(sixdof::EventMask::DEFAULT)?;

loop {
    match source.read_blocking()? {
        sixdof::Event::Motion(m) => println!("{:?} {:?}", m.translate, m.rotate),
        sixdof::Event::Button { index, pressed } => println!("button {index} {pressed}"),
        other => println!("{other:?}"),
    }
}
```

## What it does

- Speaks the daemon's protocol directly over `/var/run/spnav.sock`
  (`SPNAV_SOCKET`, then `socket = <path>` in `/etc/spnavrc`, then the default).
- Negotiates protocol version 1 and falls back to version 0 against an older
  daemon, where events still decode but requests are unavailable.
- Blocking (`read_blocking`) and non-blocking (`poll`) reads, plus
  `as_raw_fd` for `select`/`poll`/`epoll` loops.
- Queries the connected device: name, path, axis and button counts, USB id.
- Reads and writes the daemon's settings — global and per-axis sensitivity,
  dead zones, axis inversion, axis and button mapping, button actions, key
  emulation, LED, device grab, repeat interval — and can save them to its
  configuration file.
- Falls back to the Magellan protocol over the display server, which is what
  3Dconnexion's own driver speaks (`magellan` feature, one dependency:
  [x11rb]). `Source::connect` takes whichever route answers.
- Reads the device directly over USB HID on Windows and macOS (through
  [hidapi], which needs no system library on Windows; on macOS the device is
  opened shared, so 3Dconnexion's driver keeps working beside it).
- No unsafe code of its own; on Linux no dependencies at all with the default
  feature set.

```rust
let mut client = sixdof::Client::connect()?;
let mut config = client.config();
config.set_axis_sensitivity([1.0, 1.0, 1.0, 0.5, 0.5, 0.5])?;
config.set_led(sixdof::LedMode::On)?;
config.save()?;
# Ok::<(), sixdof::Error>(())
```

## Platforms

| Platform | Route                                                        |
| -------- | ------------------------------------------------------------ |
| Linux, BSD | spacenavd, then Magellan over the display server (`magellan` feature), then on Linux the device over USB HID (`hid` feature) |
| macOS    | spacenavd when it runs, else the device over USB HID          |
| Windows  | the device over USB HID                                        |

`Client` and `Config` (the daemon and its settings) exist on Unix only;
`Source` is the portable entry point. Over USB HID the numbers are the ones
spacenavd would give (each axis onto ±500, the models' own axis and button
quirks undone), and `set_sensitivity` scales them; there is no daemon
configuration to read.

## What it does not do

- No device configuration over USB HID beyond a sensitivity scale: dead
  zones and mappings are the application's to apply there.
- Only the first device found is read.
- No hotplug over USB HID: when the device goes, reads fail with
  `Error::Disconnected` and the caller is expected to connect again.

## Licence

MIT ([LICENSE-MIT]) or Apache-2.0 ([LICENSE-APACHE]), at your option.

[LICENSE-MIT]: LICENSE-MIT
[LICENSE-APACHE]: LICENSE-APACHE

[spacenavd]: https://spacenav.sourceforge.net/
[x11rb]: https://crates.io/crates/x11rb
[hidapi]: https://crates.io/crates/hidapi
