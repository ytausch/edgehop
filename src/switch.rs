//! Switching every configured device to a channel.

use anyhow::{Context, Result};
use log::{error, info, warn};

use crate::{
    config::{Channel, Device},
    hid::{Hid, Interface},
    hidpp,
};

/// Switches `devices` to `channel` in the order given. A device that is not
/// connected or fails to switch is logged and skipped. Returns whether every
/// device switched.
///
/// Devices are looked up and opened afresh on every switch rather than kept
/// open: a device that has been away on another host comes back as a new HID
/// device, and a fresh handle has no stale input queued up.
pub fn switch_all(hid: &mut impl Hid, devices: &[Device], channel: Channel) -> bool {
    let interfaces = match hid.interfaces() {
        Ok(interfaces) => interfaces,
        Err(e) => {
            error!("cannot enumerate HID devices: {e}");
            return false;
        }
    };
    let mut all_switched = true;
    for device in devices {
        match switch_one(hid, &interfaces, device, channel) {
            Ok(()) => info!("{}: switched to channel {channel}", device.name),
            Err(e) => {
                warn!("{}: not switched: {e:#}", device.name);
                all_switched = false;
            }
        }
    }
    all_switched
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
    hidpp::change_host(&handle, device.device_index, channel.host_index())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::{io, rc::Rc};

    use super::*;
    use crate::hid::{
        LOGITECH,
        fake::{FakeHid, Responder, interface},
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

    /// Answers getFeature with ChangeHost at index 0x0A, and nothing else.
    fn hidpp_device() -> Responder {
        Rc::new(|request| {
            let mut reply = request.to_vec();
            reply[4] = 0x0A;
            Ok(if request[2] == 0x00 {
                vec![reply]
            } else {
                vec![]
            })
        })
    }

    fn connected(product_ids: &[u16]) -> FakeHid {
        FakeHid {
            devices: product_ids
                .iter()
                .map(|&id| (interface(LOGITECH, id, 0xFF43, 0x0202), hidpp_device()))
                .collect(),
            ..FakeHid::default()
        }
    }

    fn channel(number: u8) -> Channel {
        number.try_into().unwrap()
    }

    /// The (device, host) of every setCurrentHost written.
    fn hosts_set(hid: &FakeHid) -> Vec<(u16, u8)> {
        let writes = hid.writes.borrow();
        writes
            .iter()
            .filter(|(_, r)| r[2] == 0x0A)
            .map(|&(id, ref r)| (id, r[4]))
            .collect()
    }

    #[test]
    fn switches_devices_in_order() {
        let mut hid = connected(&[MOUSE, KEYBOARD]);
        assert!(switch_all(
            &mut hid,
            &[device(KEYBOARD), device(MOUSE)],
            channel(2)
        ));
        assert_eq!(hosts_set(&hid), [(KEYBOARD, 1), (MOUSE, 1)]);
    }

    #[test]
    fn skips_devices_that_are_not_connected() {
        let mut hid = connected(&[MOUSE]);
        assert!(!switch_all(
            &mut hid,
            &[device(KEYBOARD), device(MOUSE)],
            channel(1)
        ));
        assert_eq!(hosts_set(&hid), [(MOUSE, 0)]);
    }

    #[test]
    fn carries_on_after_a_device_fails() {
        let mut hid = connected(&[MOUSE]);
        let broken: Responder = Rc::new(|_| Err(io::Error::other("unplugged")));
        hid.devices
            .insert(0, (interface(LOGITECH, KEYBOARD, 0xFF43, 0x0202), broken));
        assert!(!switch_all(
            &mut hid,
            &[device(KEYBOARD), device(MOUSE)],
            channel(1)
        ));
        assert_eq!(hosts_set(&hid), [(MOUSE, 0)]);
    }

    #[test]
    fn fails_when_enumeration_fails() {
        let mut hid = FakeHid {
            fail_enumeration: true,
            ..connected(&[KEYBOARD])
        };
        assert!(!switch_all(&mut hid, &[device(KEYBOARD)], channel(1)));
        assert!(hid.writes.borrow().is_empty());
    }
}
