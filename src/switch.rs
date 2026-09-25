//! Switching every configured device to a channel.

use anyhow::{Context, Result};
use log::{Level, debug, info, log};

use crate::{
    config::{Channel, Device},
    hid::{Hid, Interface, Joined},
    hidpp,
};

/// Switches `devices` to `channel` in the order given. A device that is not
/// connected or fails to switch is logged as a warning and skipped. Returns
/// the devices that did not switch.
///
/// Devices are looked up and opened afresh on every switch rather than kept
/// open: a device that has been away on another host comes back as a new HID
/// device, and a fresh handle has no stale input queued up.
pub fn switch_all(hid: &mut impl Hid, devices: &[Device], channel: Channel) -> Vec<Device> {
    switch_logging(hid, devices, channel, Level::Warn)
}

/// Like [`switch_all`], for devices that have failed before: their failures
/// are logged only in debug output.
pub fn retry_all(hid: &mut impl Hid, devices: &[Device], channel: Channel) -> Vec<Device> {
    switch_logging(hid, devices, channel, Level::Debug)
}

fn switch_logging(
    hid: &mut impl Hid,
    devices: &[Device],
    channel: Channel,
    failures: Level,
) -> Vec<Device> {
    let interfaces = match hid.interfaces() {
        Ok(interfaces) => interfaces,
        Err(e) => {
            log!(failures, "cannot enumerate HID devices: {e}");
            return devices.to_vec();
        }
    };
    devices
        .iter()
        .filter(
            |device| match switch_one(hid, &interfaces, device, channel) {
                Ok(()) => {
                    info!("{}: switched to channel {channel}", device.name);
                    false
                }
                Err(e) => {
                    log!(failures, "{}: not switched: {e:#}", device.name);
                    true
                }
            },
        )
        .cloned()
        .collect()
}

fn switch_one(
    hid: &mut impl Hid,
    interfaces: &[Interface],
    device: &Device,
    channel: Channel,
) -> Result<()> {
    let interface = interfaces
        .iter()
        .find(|interface| interface.matches(device))
        .context("not connected")?;
    let handle = hid.open(interface).context("cannot open")?;
    // Without the short reports, a receiver's error for a device it cannot
    // reach is lost, and the request just times out.
    let short_reports = interface.short_reports(interfaces).and_then(|short| {
        hid.open(short)
            .inspect_err(|e| debug!("cannot open {short}: {e}"))
            .ok()
    });
    let handle = Joined::new(handle, short_reports);
    hidpp::change_host(&handle, device.device_index, channel.host_index())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::{io, rc::Rc};

    use super::*;
    use crate::hid::{
        LOGITECH,
        fake::{FakeHid, Responder, connected, hidpp_device, hosts_set, interface, silent},
    };

    const KEYBOARD: u16 = 0xB378;
    const MOUSE: u16 = 0xB034;

    fn device(product_id: u16) -> Device {
        Device {
            name: format!("{product_id:04X}"),
            vendor_id: LOGITECH,
            product_id,
            usage_page: 0xFF43,
            usage: 0x0202,
            device_index: 0xFF,
        }
    }

    fn channel(number: u8) -> Channel {
        number.try_into().unwrap()
    }

    fn names(devices: &[Device]) -> Vec<&str> {
        devices.iter().map(|device| device.name.as_str()).collect()
    }

    #[test]
    fn switches_devices_in_order() {
        let mut hid = connected(&[MOUSE, KEYBOARD]);
        let failed = switch_all(&mut hid, &[device(KEYBOARD), device(MOUSE)], channel(2));
        assert!(failed.is_empty());
        assert_eq!(hosts_set(&hid), [(KEYBOARD, 1), (MOUSE, 1)]);
    }

    #[test]
    fn skips_devices_that_are_not_connected() {
        let mut hid = connected(&[MOUSE]);
        let failed = switch_all(&mut hid, &[device(KEYBOARD), device(MOUSE)], channel(1));
        assert_eq!(names(&failed), ["B378"]);
        assert_eq!(hosts_set(&hid), [(MOUSE, 0)]);
    }

    #[test]
    fn carries_on_after_a_device_fails() {
        let mut hid = connected(&[MOUSE]);
        let broken: Responder = Rc::new(|_| Err(io::Error::other("unplugged")));
        hid.devices
            .insert(0, (interface(LOGITECH, KEYBOARD, 0xFF43, 0x0202), broken));
        let failed = switch_all(&mut hid, &[device(KEYBOARD), device(MOUSE)], channel(1));
        assert_eq!(names(&failed), ["B378"]);
        assert_eq!(hosts_set(&hid), [(MOUSE, 0)]);
    }

    #[test]
    fn fails_when_enumeration_fails() {
        let mut hid = FakeHid {
            fail_enumeration: true,
            ..connected(&[KEYBOARD, MOUSE])
        };
        let failed = switch_all(&mut hid, &[device(KEYBOARD), device(MOUSE)], channel(1));
        assert_eq!(names(&failed), ["B378", "B034"]);
        assert!(hid.writes.borrow().is_empty());
    }

    #[test]
    fn retries_like_it_switches() {
        let mut hid = connected(&[MOUSE]);
        let failed = retry_all(&mut hid, &[device(KEYBOARD), device(MOUSE)], channel(3));
        assert_eq!(names(&failed), ["B378"]);
        assert_eq!(hosts_set(&hid), [(MOUSE, 2)]);
    }

    const RECEIVER: u16 = 0xC548;
    const KEYBOARD_SLOT: u8 = 1;
    const MOUSE_SLOT: u8 = 2;

    fn on_receiver(name: &str, slot: u8) -> Device {
        Device {
            name: name.into(),
            product_id: RECEIVER,
            usage_page: 0xFF00,
            usage: 0x0002,
            device_index: slot,
            ..device(RECEIVER)
        }
    }

    /// A receiver whose mouse answers, and whose keyboard is asleep: the
    /// receiver answers for it with a short "resource error" report.
    fn receiver(short_reports_listed: bool) -> FakeHid {
        let mouse = hidpp_device();
        let respond: Responder = Rc::new(move |request| {
            if request[1] == KEYBOARD_SLOT {
                return Ok(vec![vec![
                    0x10,
                    KEYBOARD_SLOT,
                    0x8F,
                    request[2],
                    request[3],
                    0x09,
                    0x00,
                ]]);
            }
            mouse(request)
        });
        let mut devices = vec![(interface(LOGITECH, RECEIVER, 0xFF00, 0x0002), respond)];
        if short_reports_listed {
            devices.push((interface(LOGITECH, RECEIVER, 0xFF00, 0x0001), silent()));
        }
        FakeHid {
            devices,
            ..FakeHid::default()
        }
    }

    /// How many reports went to the device in `slot`.
    fn writes_to(hid: &FakeHid, slot: u8) -> usize {
        hid.writes
            .borrow()
            .iter()
            .filter(|(_, r)| r[1] == slot)
            .count()
    }

    fn keyboard_and_mouse() -> [Device; 2] {
        [
            on_receiver("keyboard", KEYBOARD_SLOT),
            on_receiver("mouse", MOUSE_SLOT),
        ]
    }

    #[test]
    fn hears_the_receiver_about_unreachable_devices() {
        let mut hid = receiver(true);
        let failed = switch_all(&mut hid, &keyboard_and_mouse(), channel(2));
        assert_eq!(names(&failed), ["keyboard"]);
        assert_eq!(writes_to(&hid, KEYBOARD_SLOT), 1);
        assert_eq!(writes_to(&hid, MOUSE_SLOT), 2);
    }

    #[test]
    fn times_out_without_the_short_reports() {
        let mut hid = receiver(false);
        let failed = switch_all(&mut hid, &keyboard_and_mouse(), channel(2));
        assert_eq!(names(&failed), ["keyboard"]);
        assert_eq!(writes_to(&hid, KEYBOARD_SLOT), 3);
        assert_eq!(writes_to(&hid, MOUSE_SLOT), 2);
    }

    #[test]
    fn switches_when_the_short_reports_cannot_be_opened() {
        let mut hid = receiver(true);
        hid.fail_open = vec![interface(LOGITECH, RECEIVER, 0xFF00, 0x0001)];
        let failed = switch_all(&mut hid, &[on_receiver("mouse", MOUSE_SLOT)], channel(2));
        assert!(failed.is_empty());
    }

    #[test]
    fn fails_when_a_device_cannot_be_opened() {
        let mut hid = connected(&[KEYBOARD]);
        hid.fail_open = vec![interface(LOGITECH, KEYBOARD, 0xFF43, 0x0202)];
        let failed = switch_all(&mut hid, &[device(KEYBOARD)], channel(2));
        assert_eq!(names(&failed), ["B378"]);
        assert!(hid.writes.borrow().is_empty());
    }
}
