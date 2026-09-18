# Changelog

All notable changes to this crate are recorded here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the crate
follows [semantic versioning](https://semver.org/spec/v2.0.0.html).

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

[0.1.0]: https://github.com/gilbertorconde/sixdof/releases/tag/v0.1.0
