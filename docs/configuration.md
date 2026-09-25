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

`edgehop --list` shows the ids and usages of every connected Logitech HID interface. Each device has several. Pick the HID++ one:

| Connection                    | `product_id`     | `usage_page` | `usage`  | `device_index`         |
| ----------------------------- | ---------------- | ------------ | -------- | ---------------------- |
| Bluetooth                     | the device's own | `0xFF43`     | `0x0202` | `0xFF`                 |
| Logi Bolt / Unifying receiver | the receiver's   | `0xFF00`     | `0x0002` | the device's slot, 1-6 |

edgehop finds the ChangeHost feature on each device itself, so the config never contains raw report bytes.
