# How edgehop works

edgehop watches the cursor. When it rests against a configured edge of the desktop, edgehop sends each configured device the HID++ 2.0 ChangeHost command (feature `0x1814`), which moves it to another Easy-Switch channel. The same binary runs on every computer, each with its own config. The computers never talk to each other; the devices themselves carry the switch.

edgehop has no GUI, no tray icon, and no event hooks. It samples the cursor every 20 ms.

## Behavior

- **Edges** are the outer boundary of the whole desktop across all displays. Borders between displays never count, but the exposed side of a display does. For example, the bottom of a smaller display next to a taller one is an edge.
- **Switching** happens once the cursor has rested at an edge for `dwell_ms` (default 250 ms). The devices then switch in the order the config lists them: the keyboard first, then the mouse.
- **Re-arming**: after a switch, the cursor has to move at least 50 px away from the edge and `cooldown_ms` (default 2 s) has to pass before the next switch. That way a cursor left at the edge does not send the devices straight back when they return.
- **Failures** are logged and skipped. If a device is not connected (asleep, or already on another host) or does not answer, edgehop logs it, switches the others, and keeps running. Devices are opened afresh on every switch, so a device that comes back from another host is picked up without a restart.
- **Retries**: a keyboard is often asleep by the time the mouse pushes the cursor against an edge, and a sleeping device cannot be reached until you press a key. So `--watch` tries the devices that did not switch again every 500 ms, for up to a minute. The first key press wakes the keyboard, and it follows the mouse. That key press still goes to the computer the keyboard was on. The retries stop early once the cursor moves: the mouse is then back, or another mouse is in use, and the devices left behind stay where they are. `--switch` tries each device only once.
- **Receivers**: behind a Logi Bolt or Unifying receiver, the receiver answers for a device it cannot reach with a HID++ 1.0 short report. On Windows, short and long reports come in on separate HID interfaces (usage `0x0001` and `0x0002`), so edgehop opens the short report interface too. That way it hears about an unreachable device at once, instead of waiting for a reply that never comes.
