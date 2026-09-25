# Configuration

edgehop reads its config from:

| Platform | Path                                                |
| -------- | --------------------------------------------------- |
| Windows  | `%APPDATA%\edgehop\config.toml`                     |
| macOS    | `~/Library/Application Support/edgehop/config.toml` |

Or pass any other file with `--config <PATH>`, e.g. `edgehop --watch --config ./my-config.toml`. Each computer has its own config, describing the edges and devices as that computer sees them. [`config.example.toml`](../config.example.toml) is a complete, commented example:

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

## Finding your devices

`edgehop --list` asks every connected Logitech device for its name and its Easy-Switch channels, including each device paired to a Logi Bolt or Unifying receiver, and prints them as config entries:

```toml
# MX Keys S via USB Receiver (0xC548) slot 1, on Easy-Switch channel 1 of 3
[[devices]]
name = "MX Keys S"
vendor_id = 0x046D
product_id = 0xC548
usage_page = 0xFF00
usage = 0x0002
device_index = 0x01
```

Copy the entries of the devices you want to switch into your config, or append them all with `edgehop --list >> config.toml`. A device that is asleep may not answer: press a key or click to wake it, and list again. `edgehop --list --verbose` also logs every HID interface it saw.

The entries use each device's HID++ interface:

| Connection                    | `product_id`     | `usage_page` | `usage`  | `device_index`         |
| ----------------------------- | ---------------- | ------------ | -------- | ---------------------- |
| Bluetooth                     | the device's own | `0xFF43`     | `0x0202` | `0xFF`                 |
| Logi Bolt / Unifying receiver | the receiver's   | `0xFF00`     | `0x0002` | the device's slot, 1-6 |

edgehop finds the ChangeHost feature on each device itself, so the config never contains raw report bytes.
