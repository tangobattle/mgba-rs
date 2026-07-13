# mgba-rs

Rust bindings to [mGBA](https://github.com/tangobattle/mgba) (the tangobattle fork), used by [Tango](https://github.com/tangobattle/tango). `mgba-sys` builds the emulator core with cmake and generates raw bindings with bindgen; the `mgba` crate wraps them in a safe API.

Clone with `--recursive` (or run `git submodule update --init`) — the C core lives in the `mgba-sys/mgba` submodule.

Extracted from the [tango](https://github.com/tangobattle/tango) repository with history preserved.
