# How edgehop works

edgehop watches the cursor. When it rests against a configured edge of the desktop, edgehop sends each configured device the HID++ 2.0 ChangeHost command (feature `0x1814`), which moves it to another Easy-Switch channel. The same binary runs on every computer, each with its own config. The computers never talk to each other; the devices themselves carry the switch.

edgehop has no GUI, no tray icon, and no event hooks. It samples the cursor every 20 ms.

## Behavior

- **Edges** are the outer boundary of the whole desktop across all displays. Borders between displays never count, but the exposed side of a display does. For example, the bottom of a smaller display next to a taller one is an edge.
- **Switching** happens once the cursor has rested at an edge for `dwell_ms` (default 250 ms). The devices then switch in the order the config lists them: the keyboard first, then the mouse.
- **Re-arming**: after a switch, the cursor has to move at least 50 px away from the edge and `cooldown_ms` (default 2 s) has to pass before the next switch. That way a cursor left at the edge does not send the devices straight back when they return.
- **Failures** are logged and skipped. If a device is not connected (asleep, or already on another host) or does not answer, edgehop logs it, switches the others, and keeps running. Devices are opened afresh on every switch, so a device that comes back from another host is picked up without a restart.
