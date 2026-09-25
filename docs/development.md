# Development

## Build

You need a stable Rust toolchain from [rustup](https://rustup.rs). On Windows, you also need the Visual Studio C++ build tools, because hidapi is compiled from source.

```shell
cargo build --release
```

The binary lands in `target/release/`. On Windows, the C runtime is linked statically (see [`.cargo/config.toml`](../.cargo/config.toml)), so the `.exe` runs without the Visual C++ Redistributable.

On Windows, [`build.rs`](../build.rs) renders the icon from [`assets/icon.svg`](../assets/icon.svg) into the `.exe`, using the simpler [`assets/icon-small.svg`](../assets/icon-small.svg) for 24 px and below.

## Tooling

Development tools come from [pixi](https://pixi.sh):

```shell
pixi run pre-commit-install  # lint on every commit
pixi run lint                # run all linters on all files
pixi run coverage            # tests, failing below 100% line coverage
pixi run mutants             # mutation testing with cargo-mutants
cargo test
```

## Code layout

The code is split so that as much as possible can be tested without hardware:

- **Library (`src/lib.rs` and friends):** everything platform-independent, covered completely by unit tests against fake HID devices and displays.
  - `config`: the TOML config
  - `desktop`: edge detection
  - `trigger`: the dwell/cooldown state machine
  - `hidpp`: the HID++ protocol
  - `switch`: switching all devices
  - `discover`: finding the connected devices for `--list`
  - `watch`: the watch loop's decisions
- **Binary (`src/main.rs` and `src/sys/`):** the thin platform glue. `sys/hid.rs` wraps hidapi, and `sys/macos.rs` and `sys/windows.rs` read the cursor and displays. This is the only code the coverage and mutation checks exclude.

## Releases

Releases are drafted automatically when the version in `Cargo.toml` changes on `main`.
