# edgehop

Push the mouse cursor against an outer edge of the screen, and your Logitech keyboard and mouse switch to another computer.

edgehop watches the cursor. When it rests against a configured edge of the desktop, edgehop sends each configured device the HID++ 2.0 ChangeHost command (feature `0x1814`), which moves it to another Easy-Switch channel. The same binary runs on every computer, each with its own config. The computers never talk to each other; the devices themselves carry the switch.

- Runs on Windows 10/11 (x64) and macOS on Apple Silicon.
- Needs no admin rights, installer, or runtime. It is a single binary with hidapi linked in statically.
- Has no GUI, no tray icon, and no event hooks. It samples the cursor every 20 ms.

## How it behaves

- **Edges** are the outer boundary of the whole desktop across all displays. Borders between displays never count, but the exposed side of a display does. For example, the bottom of a smaller display next to a taller one is an edge.
- **Switching** happens once the cursor has rested at an edge for `dwell_ms` (default 250 ms). The devices then switch in the order the config lists them: the keyboard first, then the mouse.
- **Re-arming**: after a switch, the cursor has to move at least 50 px away from the edge and `cooldown_ms` (default 2 s) has to pass before the next switch. That way a cursor left at the edge does not send the devices straight back when they return.
- **Failures** are logged and skipped. If a device is not connected (asleep, or already on another host) or does not answer, edgehop logs it, switches the others, and keeps running. Devices are opened afresh on every switch, so a device that comes back from another host is picked up without a restart.

## Install

Download the binary for your platform from the [latest release](https://github.com/ytausch/edgehop/releases), or [build it yourself](#build).

On **macOS**, a downloaded binary is quarantined and Gatekeeper refuses to run it. Clear the flag and make it executable:

```shell
mkdir -p ~/.local/bin
mv ~/Downloads/edgehop-aarch64-apple-darwin ~/.local/bin/edgehop
xattr -d com.apple.quarantine ~/.local/bin/edgehop
chmod +x ~/.local/bin/edgehop
```

On **Windows**, put the binary somewhere permanent, for example `%LOCALAPPDATA%\Programs\edgehop\edgehop.exe`.

## Configure

edgehop reads its config from:

| Platform | Path                                                |
| -------- | --------------------------------------------------- |
| Windows  | `%APPDATA%\edgehop\config.toml`                     |
| macOS    | `~/Library/Application Support/edgehop/config.toml` |

Or pass any other file with `--config <PATH>`, e.g. `edgehop --watch --config ./my-config.toml`. Each computer has its own config, describing the edges and devices as that computer sees them. [`config.example.toml`](config.example.toml) is a complete, commented example:

```toml
dwell_ms = 250      # how long the cursor rests at an edge before switching
cooldown_ms = 2000  # minimum time between two switches

# The Easy-Switch channel (1-3) each edge switches to. Leave an edge out to
# do nothing there.
[edges]
right = 1

# Switched in the order listed.
[[devices]]
name = "MX Keys S"      # only used in log messages
vendor_id = 0x046D
product_id = 0xB378
usage_page = 0xFF43
usage = 0x0202
device_index = 0xFF
```

`edgehop --list` shows the ids and usages of every connected Logitech HID interface. Each device has several. Pick the HID++ one:

| Connection                    | `product_id`     | `usage_page` | `usage`  | `device_index`         |
| ----------------------------- | ---------------- | ------------ | -------- | ---------------------- |
| Bluetooth                     | the device's own | `0xFF43`     | `0x0202` | `0xFF`                 |
| Logi Bolt / Unifying receiver | the receiver's   | `0xFF00`     | `0x0002` | the device's slot, 1-6 |

edgehop finds the ChangeHost feature on each device itself, so the config never contains raw report bytes.

## Use

```shell
edgehop --watch        # switch whenever the cursor rests at a configured edge
edgehop --switch 1     # switch every configured device to channel 1 once, and exit
edgehop --list         # list the HID interfaces of connected Logitech devices
```

Add `--verbose` to log every step, including the HID++ lookups.

## Start at login

Neither setup needs admin rights.

### Windows

Create a shortcut in the Startup folder (`shell:startup`). The window starts minimized; its console shows the log. In PowerShell:

```powershell
$shortcut = (New-Object -ComObject WScript.Shell).CreateShortcut(
  "$([Environment]::GetFolderPath('Startup'))\edgehop.lnk")
$shortcut.TargetPath = "$env:LOCALAPPDATA\Programs\edgehop\edgehop.exe"
$shortcut.Arguments = "--watch"
$shortcut.WindowStyle = 7  # minimized
$shortcut.Save()
```

Or by hand: press <kbd>Win</kbd>+<kbd>R</kbd>, run `shell:startup`, create a shortcut to `edgehop.exe --watch`, and set **Run** to **Minimized** in its properties.

### macOS

Install a LaunchAgent. launchd starts it at login, restarts it if it exits, and writes its log to `~/Library/Logs/edgehop.log`:

```shell
cat > ~/Library/LaunchAgents/io.github.ytausch.edgehop.plist <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key>
  <string>io.github.ytausch.edgehop</string>
  <key>ProgramArguments</key>
  <array>
    <string>$HOME/.local/bin/edgehop</string>
    <string>--watch</string>
  </array>
  <key>RunAtLoad</key>
  <true/>
  <key>KeepAlive</key>
  <true/>
  <key>StandardErrorPath</key>
  <string>$HOME/Library/Logs/edgehop.log</string>
</dict>
</plist>
EOF
launchctl bootstrap gui/$(id -u) ~/Library/LaunchAgents/io.github.ytausch.edgehop.plist
```

To stop it, run `launchctl bootout gui/$(id -u)/io.github.ytausch.edgehop`.

#### Input Monitoring

Reading the cursor needs no permission. Opening a **keyboard's** HID++ interface does: macOS treats it as listening to the keyboard. Without it, the log shows the keyboard failing to open while the mouse still switches.

Grant **Input Monitoring** under System Settings → Privacy & Security:

- For the LaunchAgent, grant it to the binary itself. Click **+**, press <kbd>Cmd</kbd>+<kbd>Shift</kbd>+<kbd>G</kbd>, and enter `~/.local/bin/edgehop`. Then restart the agent with `launchctl kickstart -k gui/$(id -u)/io.github.ytausch.edgehop`.
- When running edgehop from a terminal, grant it to the terminal app.

The grant belongs to that exact binary. After replacing it with a new version, remove the old entry and add the binary again.

## Build

You need a stable Rust toolchain from [rustup](https://rustup.rs). On Windows, you also need the Visual Studio C++ build tools, because hidapi is compiled from source.

```shell
cargo build --release
```

The binary lands in `target/release/`. On Windows, the C runtime is linked statically (see [`.cargo/config.toml`](.cargo/config.toml)), so the `.exe` runs without the Visual C++ Redistributable.

## Development

Development tools come from [pixi](https://pixi.sh):

```shell
pixi run pre-commit-install  # lint on every commit
pixi run lint                # run all linters on all files
pixi run coverage            # tests, failing below 100% line coverage
pixi run mutants             # mutation testing with cargo-mutants
cargo test
```

The code is split so that as much as possible can be tested without hardware:

- **Library (`src/lib.rs` and friends):** everything platform-independent, covered completely by unit tests against fake HID devices and displays.
  - `config`: the TOML config
  - `desktop`: edge detection
  - `trigger`: the dwell/cooldown state machine
  - `hidpp`: the HID++ protocol
  - `switch`: switching all devices
  - `watch`: the watch loop's decisions
- **Binary (`src/main.rs` and `src/sys/`):** the thin platform glue. `sys/hid.rs` wraps hidapi, and `sys/macos.rs` and `sys/windows.rs` read the cursor and displays. This is the only code the coverage and mutation checks exclude.

Releases are drafted automatically when the version in `Cargo.toml` changes on `main`.
