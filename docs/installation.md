# Installation

edgehop needs no admin rights, installer, or runtime. It is a single binary with hidapi linked in statically.

Install it with [pixi](https://pixi.sh), from [conda-forge](https://prefix.dev/channels/conda-forge/packages/edgehop):

```shell
pixi global install edgehop
```

This puts `edgehop` on your `PATH`. Update it with `pixi global update edgehop`.

## Nix

If you manage your Mac with [Nix](https://nixos.org), install edgehop from its flake instead:

```shell
nix profile add github:ytausch/edgehop
```

Or add the flake to your nix-darwin or Home Manager configuration and use its `packages.aarch64-darwin.default`. Don't make its `nixpkgs` input follow yours: with its own locked nixpkgs, the binary and its store path only change when edgehop itself changes, so the [Input Monitoring](#input-monitoring) grant survives your own updates. Grant it to the store path, which `readlink -f "$(command -v edgehop)"` prints.

## Download

You can also download the binary for your platform from the [latest release](https://github.com/ytausch/edgehop/releases), or [build it yourself](development.md#build).

On **macOS**, a downloaded binary is quarantined and Gatekeeper refuses to run it. Clear the flag and make it executable:

```shell
mkdir -p ~/.local/bin
mv ~/Downloads/edgehop-aarch64-apple-darwin ~/.local/bin/edgehop
xattr -d com.apple.quarantine ~/.local/bin/edgehop
chmod +x ~/.local/bin/edgehop
```

On **Windows**, put the downloaded binary somewhere permanent, for example `%LOCALAPPDATA%\Programs\edgehop\edgehop.exe`. The binary isn't signed, so SmartScreen blocks a downloaded copy with "Windows protected your PC". Unblock it once:

```powershell
Unblock-File "$env:LOCALAPPDATA\Programs\edgehop\edgehop.exe"
```

If Smart App Control is on (Windows 11), it blocks unsigned binaries however you install them, and it has no per-app exceptions. edgehop only runs with Smart App Control turned off, under Windows Security → App & browser control.

## Start at login

Neither setup needs admin rights. Both pick up edgehop from your `PATH`; if it isn't on your `PATH`, replace that lookup with the binary's full path.

### Windows

Register a scheduled task that starts edgehop at login. Its console window is hidden once edgehop has loaded its config, so edgehop runs without a taskbar button. In PowerShell:

```powershell
$action = New-ScheduledTaskAction -Execute (Get-Command edgehop).Source -Argument "--watch --hide-console"
$trigger = New-ScheduledTaskTrigger -AtLogOn -User $env:USERNAME
$settings = New-ScheduledTaskSettingsSet -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries `
  -ExecutionTimeLimit ([TimeSpan]::Zero)
Register-ScheduledTask -TaskName edgehop -Action $action -Trigger $trigger -Settings $settings
```

The settings keep the task running: by default, Task Scheduler doesn't start a task on battery, stops it when the computer is unplugged, and ends it after three days.

Or by hand: open Task Scheduler and choose **Create Task**. Under **Triggers**, add one that begins **At log on** for your user. Under **Actions**, start `edgehop.exe` (with pixi, `%USERPROFILE%\.pixi\bin\edgehop.exe`) with the arguments `--watch --hide-console`. Under **Conditions**, clear **Start the task only if the computer is on AC power**, and under **Settings**, clear **Stop the task if it runs longer than**.

Run `Start-ScheduledTask edgehop` to start it without logging out, and `Unregister-ScheduledTask edgehop` to remove it.

With the window hidden, the log isn't shown anywhere. To stop edgehop, end it in Task Manager or run `Stop-Process -Name edgehop`. To see the log, leave out `--hide-console`, or stop edgehop and run `edgehop --watch` in a terminal. Don't pass `--hide-console` in a terminal: it hides the terminal's window along with edgehop's.

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
    <string>$(command -v edgehop)</string>
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

- For the LaunchAgent, grant it to the binary itself. Click **+**, press <kbd>Cmd</kbd>+<kbd>Shift</kbd>+<kbd>G</kbd>, and enter its path:
  - pixi: `~/.pixi/envs/edgehop/bin/edgehop`. The `edgehop` in `~/.pixi/bin` is only a launcher that replaces itself with this binary, so macOS checks the grant against this one.
  - Nix: the store path that `readlink -f "$(command -v edgehop)"` prints.
  - Downloaded binary: `~/.local/bin/edgehop`.

  Then restart the agent with `launchctl kickstart -k gui/$(id -u)/io.github.ytausch.edgehop`.

- When running edgehop from a terminal, grant it to the terminal app.

The grant belongs to that exact binary. After replacing it with a new version, for example through `pixi global update`, remove the old entry and add the binary again.
