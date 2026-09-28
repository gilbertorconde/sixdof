# Changelog

All notable changes to this crate are recorded here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the crate
follows [semantic versioning](https://semver.org/spec/v2.0.0.html).

## [0.2.0] - 2026-09-28

### Added

- macOS and Windows: the device read directly over USB HID
  (`hid_device::Client`), through hidapi. On macOS the device is opened
  shared, and spacenavd is tried first.
- `Backend::Hid`, what `Source::backend` reports for that route.
- `hid` feature: the same route on Linux, over hidraw, for a machine with no
  daemon.
- Over USB HID the numbers are the daemon's: each axis taken from the range
  the device declares onto ±500, y and z swapped and turned on the models
  whose axes run that way, the Pro and Enterprise buttons counted from 0,
  and a motion told only when the axes changed.

### Changed

- The crate builds on every platform. `Client`, `Config`, `ButtonAction`,
  `LedMode`, `socket_path` and `DEFAULT_SOCKET` are Unix only.
- `Source::as_raw_fd` returns `Option<RawFd>`: `None` for the USB HID route,
  which has no descriptor to wait on.

## [0.1.0] — 2026-09-18

First release.

### Added

- `Client`: the spacenavd socket protocol, spoken directly — version
  handshake with a fallback to protocol 0, 32-byte event packets, and the
  requests that name the client, choose its event mask and describe the
  device.
- `Config`: the daemon's own settings — global and per-axis sensitivity, dead
  zones, axis inversion, axis and button mapping, button actions, key
  emulation, swapped axes, LED, device grab, repeat interval, serial port and
  socket path — with `save`, `restore` and `reset`.
- `Event`: motion, buttons, hotplug, configuration changes, and the raw axis
  and button events for anyone who wants them before the daemon's own
  processing.
- Blocking, timed and non-blocking reads, plus `as_raw_fd` for callers that
  run their own readiness loop.
- `magellan` feature: the protocol 3Dconnexion's own driver speaks over a
  display server, on its own connection and window so it reads from a
  background thread.
- `Source`: whichever route answers — the daemon first, the display server
  after.

[0.2.0]: https://github.com/gilbertorconde/sixdof/releases/tag/v0.2.0
[0.1.0]: https://github.com/gilbertorconde/sixdof/releases/tag/v0.1.0
