# Installation

Download the binary for your platform from the [latest release](https://github.com/ytausch/edgehop/releases), or [build it yourself](development.md#build). edgehop needs no admin rights, installer, or runtime. It is a single binary with hidapi linked in statically.

On **macOS**, a downloaded binary is quarantined and Gatekeeper refuses to run it. Clear the flag and make it executable:

```shell
mkdir -p ~/.local/bin
mv ~/Downloads/edgehop-aarch64-apple-darwin ~/.local/bin/edgehop
xattr -d com.apple.quarantine ~/.local/bin/edgehop
chmod +x ~/.local/bin/edgehop
```

On macOS, you can also install edgehop with [Nix](https://nixos.org):

```shell
nix profile add github:ytausch/edgehop
```

Or add the flake to your nix-darwin or Home Manager configuration and use its `packages.aarch64-darwin.default`. Don't make its `nixpkgs` input follow yours: with its own locked nixpkgs, the binary and its store path only change when edgehop itself changes, so the [Input Monitoring](#input-monitoring) grant survives your own updates. Grant it to the store path, which `readlink -f "$(command -v edgehop)"` prints.

If you don't use Nix, the recommended installation on macOS and Windows is via [pixi](https://pixi.sh), from [conda-forge](https://prefix.dev/channels/conda-forge/packages/edgehop):

```shell
pixi global install edgehop
```

On **Windows**, put the binary somewhere permanent, for example `%LOCALAPPDATA%\Programs\edgehop\edgehop.exe`.

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
