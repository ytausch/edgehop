//! Finding the connected devices for `edgehop --list`, by asking each one over
//! HID++ for its name and Easy-Switch hosts. This is what makes devices behind
//! a receiver findable at all: the operating system only sees the receiver.

use std::{fmt, io, ops::RangeInclusive};

use log::debug;

use crate::{
    config::Device,
    hid::{self, Hid, Interface},
    hidpp::{self, Hosts},
};

/// The HID++ interface of a device connected over Bluetooth.
const BLUETOOTH: (u16, u16) = (0xFF43, 0x0202);
/// The HID++ interface of a Logi Bolt or Unifying receiver, which the devices
/// paired to it share.
const RECEIVER: (u16, u16) = (0xFF00, 0x0002);
const SLOTS: RangeInclusive<u8> = 1..=6;
/// The device index of a device connected directly.
const DIRECT: u8 = 0xFF;

/// What `edgehop --list` found.
#[derive(Debug, PartialEq)]
pub enum Entry {
    /// A device that answered, with its hosts if it can switch.
    Found {
        device: Device,
        /// How it is connected.
        via: String,
        hosts: Option<Hosts>,
    },
    /// A device, or a receiver's slots, that could not be asked.
    Unreachable { what: String, reason: String },
}

/// Asks every connected Logitech device for its name and hosts.
pub fn discover(hid: &mut impl Hid) -> io::Result<Vec<Entry>> {
    let interfaces = hid::logitech_interfaces(hid)?;
    let mut entries = vec![];
    for interface in &interfaces {
        debug!("{interface}");
        match (interface.usage_page, interface.usage) {
            BLUETOOTH => entries.push(bluetooth(hid, interface)),
            RECEIVER => entries.extend(receiver(hid, interface, &interfaces)),
            _ => {}
        }
    }
    Ok(entries)
}

/// The entries as `edgehop --list` prints them: valid TOML, so that the
/// devices can be pasted into the config as they are.
pub fn report(entries: &[Entry]) -> String {
    if entries.is_empty() {
        return "# No Logitech HID++ devices found.\n".into();
    }
    entries
        .iter()
        .map(Entry::to_string)
        .collect::<Vec<_>>()
        .join("\n")
}

fn bluetooth(hid: &mut impl Hid, interface: &Interface) -> Entry {
    let asked = hid
        .open(interface)
        .map_err(|e| format!("cannot open: {e}"))
        .and_then(|handle| ask(&handle, DIRECT).map_err(|e| e.to_string()));
    match asked {
        Ok((name, hosts)) => Entry::Found {
            device: device(interface, DIRECT, name.unwrap_or(interface.product.clone())),
            via: "Bluetooth".into(),
            hosts,
        },
        Err(reason) => Entry::Unreachable {
            what: describe(interface),
            reason,
        },
    }
}

fn receiver(hid: &mut impl Hid, interface: &Interface, interfaces: &[Interface]) -> Vec<Entry> {
    let handle = match hid::open_hidpp(hid, interface, interfaces) {
        Ok(handle) => handle,
        Err(e) => {
            return vec![Entry::Unreachable {
                what: describe(interface),
                reason: format!("cannot open: {e}"),
            }];
        }
    };
    let mut entries = vec![];
    // The slots that did not answer, grouped by why.
    let mut failed: Vec<(String, Vec<String>)> = vec![];
    for slot in SLOTS {
        match ask(&handle, slot) {
            Ok((name, hosts)) => entries.push(Entry::Found {
                device: device(
                    interface,
                    slot,
                    name.unwrap_or_else(|| format!("Device in slot {slot}")),
                ),
                via: format!("{} slot {slot}", describe(interface)),
                hosts,
            }),
            Err(hidpp::Error::Receiver(hidpp::UNKNOWN_DEVICE)) => {
                debug!("{}: slot {slot} is empty", describe(interface));
            }
            Err(e) => {
                let reason = e.to_string();
                match failed.iter_mut().find(|(r, _)| *r == reason) {
                    Some((_, slots)) => slots.push(slot.to_string()),
                    None => failed.push((reason, vec![slot.to_string()])),
                }
            }
        }
    }
    entries.extend(
        failed
            .into_iter()
            .map(|(reason, slots)| Entry::Unreachable {
                what: format!(
                    "{} {} {}",
                    describe(interface),
                    if slots.len() == 1 { "slot" } else { "slots" },
                    slots.join(", ")
                ),
                reason: format!(
                    "{reason}. If a device is paired there, press a key on it to wake it, \
             and list again."
                ),
            }),
    );
    entries
}

fn ask(
    handle: &impl hid::Handle,
    device_index: u8,
) -> Result<(Option<String>, Option<Hosts>), hidpp::Error> {
    Ok((
        hidpp::device_name(handle, device_index)?,
        hidpp::hosts(handle, device_index)?,
    ))
}

fn device(interface: &Interface, device_index: u8, name: String) -> Device {
    Device {
        name,
        vendor_id: interface.vendor_id,
        product_id: interface.product_id,
        usage_page: interface.usage_page,
        usage: interface.usage,
        device_index,
    }
}

fn describe(interface: &Interface) -> String {
    format!("{} ({:#06X})", interface.product, interface.product_id)
}

impl fmt::Display for Entry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Found {
                device,
                via,
                hosts: Some(hosts),
            } => write!(
                f,
                "# {} via {via}, on Easy-Switch channel {} of {}\n\
                 [[devices]]\n\
                 name = {}\n\
                 vendor_id = {:#06X}\n\
                 product_id = {:#06X}\n\
                 usage_page = {:#06X}\n\
                 usage = {:#06X}\n\
                 device_index = {:#04X}\n",
                device.name,
                u16::from(hosts.current) + 1,
                hosts.count,
                toml::Value::String(device.name.clone()),
                device.vendor_id,
                device.product_id,
                device.usage_page,
                device.usage,
                device.device_index,
            ),
            Self::Found {
                device,
                via,
                hosts: None,
            } => writeln!(
                f,
                "# {} via {via} cannot switch: it has no Easy-Switch.",
                device.name
            ),
            Self::Unreachable { what, reason } => writeln!(f, "# {what}: {reason}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::rc::Rc;

    use super::*;
    use crate::{
        config::Config,
        hid::{
            LOGITECH,
            fake::{FakeHid, Responder, interface, silent},
        },
    };

    const KEYBOARD: u16 = 0xB378;
    const BOLT: u16 = 0xC548;
    const NAME_INDEX: u8 = 0x03;
    const CHANGE_HOST_INDEX: u8 = 0x0A;

    fn reply(request: &[u8], params: &[u8]) -> Vec<u8> {
        let mut reply = request[..4].to_vec();
        reply.extend_from_slice(params);
        reply.resize(20, 0);
        reply
    }

    /// Answers for the devices at `slots`, each as (device index, name, host
    /// info), and reports every other index as `missing` says: with a HID++ 1.0
    /// error code, or `None` for no reply.
    fn devices(
        slots: Vec<(u8, &'static str, Option<[u8; 2]>)>,
        missing: fn(u8) -> Option<u8>,
    ) -> Responder {
        Rc::new(move |request| {
            let (index, feature_index, function, param) =
                (request[1], request[2], request[3], request[4]);
            let Some(&(_, name, hosts)) = slots.iter().find(|(i, ..)| *i == index) else {
                return Ok(missing(index)
                    .map(|code| vec![0x10, index, 0x8F, feature_index, function, code, 0])
                    .into_iter()
                    .collect());
            };
            let params = match (feature_index, function >> 4) {
                (0x00, _) if request[4..6] == [0x00, 0x05] => vec![NAME_INDEX],
                (0x00, _) => vec![hosts.map_or(0, |_| CHANGE_HOST_INDEX)],
                (NAME_INDEX, 0) => vec![u8::try_from(name.len()).unwrap()],
                (NAME_INDEX, _) => name.as_bytes()[usize::from(param)..].to_vec(),
                _ => hosts.unwrap().to_vec(),
            };
            Ok(vec![reply(request, &params)])
        })
    }

    /// A Bolt receiver's long report interface answering with `responder`,
    /// and its short report interface, listed apart as on Windows.
    fn bolt(responder: Responder) -> Vec<(Interface, Responder)> {
        vec![
            (interface(LOGITECH, BOLT, 0xFF00, 0x0002), responder),
            (interface(LOGITECH, BOLT, 0xFF00, 0x0001), silent()),
        ]
    }

    fn hid(devices: Vec<(Interface, Responder)>) -> FakeHid {
        FakeHid {
            devices,
            ..FakeHid::default()
        }
    }

    fn found(product_id: u16, usage: (u16, u16), index: u8, name: &str) -> Device {
        Device {
            name: name.into(),
            vendor_id: LOGITECH,
            product_id,
            usage_page: usage.0,
            usage: usage.1,
            device_index: index,
        }
    }

    #[test]
    fn asks_bluetooth_devices() {
        let mut hid = hid(vec![
            (
                interface(LOGITECH, KEYBOARD, 0x0001, 0x0006),
                devices(vec![], |_| None),
            ),
            (
                interface(LOGITECH, KEYBOARD, 0xFF43, 0x0202),
                devices(vec![(DIRECT, "MX Keys S", Some([3, 1]))], |_| None),
            ),
            (
                interface(0x05AC, 0x0001, 0xFF43, 0x0202),
                devices(vec![], |_| None),
            ),
        ]);
        assert_eq!(
            discover(&mut hid).unwrap(),
            [Entry::Found {
                device: found(KEYBOARD, BLUETOOTH, DIRECT, "MX Keys S"),
                via: "Bluetooth".into(),
                hosts: Some(Hosts {
                    count: 3,
                    current: 1
                }),
            }]
        );
    }

    #[test]
    fn asks_every_slot_of_a_receiver() {
        let responder = devices(
            vec![
                (1, "MX Keys S", Some([3, 0])),
                (2, "Lift", None),
                (4, "MX Master 3S", Some([3, 0])),
            ],
            |_| Some(hidpp::UNKNOWN_DEVICE),
        );
        let mut hid = hid(bolt(responder));
        let hosts = Some(Hosts {
            count: 3,
            current: 0,
        });
        assert_eq!(
            discover(&mut hid).unwrap(),
            [
                Entry::Found {
                    device: found(BOLT, RECEIVER, 1, "MX Keys S"),
                    via: "Device C548 (0xC548) slot 1".into(),
                    hosts,
                },
                Entry::Found {
                    device: found(BOLT, RECEIVER, 2, "Lift"),
                    via: "Device C548 (0xC548) slot 2".into(),
                    hosts: None,
                },
                Entry::Found {
                    device: found(BOLT, RECEIVER, 4, "MX Master 3S"),
                    via: "Device C548 (0xC548) slot 4".into(),
                    hosts,
                },
            ]
        );
    }

    #[test]
    fn groups_the_slots_that_do_not_answer_by_why() {
        let responder = devices(vec![(2, "MX Keys S", Some([3, 0]))], |slot| match slot {
            1 => Some(0x09),
            3 => Some(hidpp::UNKNOWN_DEVICE),
            4 => Some(hidpp::UNKNOWN_DEVICE),
            _ => None,
        });
        let mut hid = hid(bolt(responder));
        let entries = discover(&mut hid).unwrap();
        assert_eq!(entries.len(), 3);
        assert_eq!(
            entries[1..],
            [
                Entry::Unreachable {
                    what: "Device C548 (0xC548) slot 1".into(),
                    reason: "device is not reachable (asleep, out of range, or on another host). \
                             If a device is paired there, press a key on it to wake it, and list \
                             again."
                        .into(),
                },
                Entry::Unreachable {
                    what: "Device C548 (0xC548) slots 5, 6".into(),
                    reason: "no reply after 3 attempts. If a device is paired there, press a \
                             key on it to wake it, and list again."
                        .into(),
                },
            ]
        );
    }

    #[test]
    fn names_nameless_devices_by_where_they_are() {
        let nameless: Responder = Rc::new(|request| {
            Ok(match request[2] {
                0x00 if request[4..6] == [0x18, 0x14] => vec![reply(request, &[CHANGE_HOST_INDEX])],
                0x00 => vec![reply(request, &[0])],
                _ => vec![reply(request, &[3, 0])],
            })
        });
        let mut hid = hid(vec![
            (
                interface(LOGITECH, KEYBOARD, 0xFF43, 0x0202),
                nameless.clone(),
            ),
            (interface(LOGITECH, BOLT, 0xFF00, 0x0002), nameless),
        ]);
        let hosts = Some(Hosts {
            count: 3,
            current: 0,
        });
        let bluetooth = Entry::Found {
            device: found(KEYBOARD, BLUETOOTH, DIRECT, "Device B378"),
            via: "Bluetooth".into(),
            hosts,
        };
        let slots = SLOTS.map(|slot| Entry::Found {
            device: found(BOLT, RECEIVER, slot, &format!("Device in slot {slot}")),
            via: format!("Device C548 (0xC548) slot {slot}"),
            hosts,
        });
        assert_eq!(
            discover(&mut hid).unwrap(),
            std::iter::once(bluetooth).chain(slots).collect::<Vec<_>>()
        );
    }

    #[test]
    fn reports_devices_that_do_not_answer() {
        let mut hid = hid(vec![(
            interface(LOGITECH, KEYBOARD, 0xFF43, 0x0202),
            silent(),
        )]);
        assert_eq!(
            discover(&mut hid).unwrap(),
            [Entry::Unreachable {
                what: "Device B378 (0xB378)".into(),
                reason: "no reply after 3 attempts".into(),
            }]
        );
    }

    #[test]
    fn reports_interfaces_that_cannot_be_opened() {
        let keyboard = interface(LOGITECH, KEYBOARD, 0xFF43, 0x0202);
        let receiver = interface(LOGITECH, BOLT, 0xFF00, 0x0002);
        let mut hid = FakeHid {
            fail_open: vec![keyboard.clone(), receiver.clone()],
            ..hid(vec![(keyboard, silent()), (receiver, silent())])
        };
        assert_eq!(
            discover(&mut hid).unwrap(),
            [
                Entry::Unreachable {
                    what: "Device B378 (0xB378)".into(),
                    reason: "cannot open: access denied".into(),
                },
                Entry::Unreachable {
                    what: "Device C548 (0xC548)".into(),
                    reason: "cannot open: access denied".into(),
                },
            ]
        );
    }

    #[test]
    fn fails_when_enumeration_fails() {
        let mut hid = FakeHid {
            fail_enumeration: true,
            ..FakeHid::default()
        };
        assert!(discover(&mut hid).is_err());
    }

    #[test]
    fn reports_in_config_syntax() {
        let hosts = Some(Hosts {
            count: 3,
            current: 1,
        });
        let entries = [
            Entry::Found {
                device: found(BOLT, RECEIVER, 1, r#"MX "Keys" S"#),
                via: "USB Receiver (0xC548) slot 1".into(),
                hosts,
            },
            Entry::Found {
                device: found(BOLT, RECEIVER, 2, "Lift"),
                via: "USB Receiver (0xC548) slot 2".into(),
                hosts: None,
            },
            Entry::Found {
                device: found(KEYBOARD, BLUETOOTH, DIRECT, "MX Keys S"),
                via: "Bluetooth".into(),
                hosts,
            },
            Entry::Unreachable {
                what: "USB Receiver (0xC548) slots 3, 4".into(),
                reason: "no reply.".into(),
            },
        ];
        let report = report(&entries);
        assert_eq!(
            report,
            r#"# MX "Keys" S via USB Receiver (0xC548) slot 1, on Easy-Switch channel 2 of 3
[[devices]]
name = 'MX "Keys" S'
vendor_id = 0x046D
product_id = 0xC548
usage_page = 0xFF00
usage = 0x0002
device_index = 0x01

# Lift via USB Receiver (0xC548) slot 2 cannot switch: it has no Easy-Switch.

# MX Keys S via Bluetooth, on Easy-Switch channel 2 of 3
[[devices]]
name = "MX Keys S"
vendor_id = 0x046D
product_id = 0xB378
usage_page = 0xFF43
usage = 0x0202
device_index = 0xFF

# USB Receiver (0xC548) slots 3, 4: no reply.
"#
        );
        let config: Config = toml::from_str(&report).unwrap();
        assert_eq!(
            config.devices,
            [
                found(BOLT, RECEIVER, 1, r#"MX "Keys" S"#),
                found(KEYBOARD, BLUETOOTH, DIRECT, "MX Keys S"),
            ]
        );
    }

    #[test]
    fn reports_when_nothing_was_found() {
        assert_eq!(report(&[]), "# No Logitech HID++ devices found.\n");
    }
}
