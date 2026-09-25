//! The HID access edgehop needs, as traits so that everything built on it can
//! be tested without hardware.

use std::{ffi::CString, fmt, io, time::Duration};

use crate::config::Device;

pub const LOGITECH: u16 = 0x046D;

/// Enumerates and opens HID interfaces.
pub trait Hid {
    type Handle: Handle;

    /// Every HID interface currently present, freshly enumerated.
    fn interfaces(&mut self) -> io::Result<Vec<Interface>>;

    fn open(&mut self, interface: &Interface) -> io::Result<Self::Handle>;
}

/// An open HID interface.
pub trait Handle {
    /// Writes one report; the first byte is the report id.
    fn write(&self, report: &[u8]) -> io::Result<()>;

    /// Reads one input report into `buffer`, waiting at most `timeout`.
    /// Returns its length, or 0 if none arrived in time.
    fn read(&self, buffer: &mut [u8], timeout: Duration) -> io::Result<usize>;
}

/// One top-level collection of a HID device. A device can have several,
/// e.g. its keyboard input and its HID++ channel.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Interface {
    pub vendor_id: u16,
    pub product_id: u16,
    pub usage_page: u16,
    pub usage: u16,
    pub product: String,
    /// Platform-specific path to open the interface with.
    pub path: CString,
}

impl Interface {
    pub fn matches(&self, device: &Device) -> bool {
        (self.vendor_id, self.product_id, self.usage_page, self.usage)
            == (
                device.vendor_id,
                device.product_id,
                device.usage_page,
                device.usage,
            )
    }
}

impl fmt::Display for Interface {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "vendor_id = {:#06X}  product_id = {:#06X}  usage_page = {:#06X}  usage = {:#06X}  {}",
            self.vendor_id, self.product_id, self.usage_page, self.usage, self.product
        )
    }
}

/// The interfaces of connected Logitech devices, sorted by product id and
/// usage.
pub fn logitech_interfaces(hid: &mut impl Hid) -> io::Result<Vec<Interface>> {
    let mut interfaces = hid.interfaces()?;
    interfaces.retain(|interface| interface.vendor_id == LOGITECH);
    interfaces.sort_by_key(|i| (i.product_id, i.usage_page, i.usage));
    Ok(interfaces)
}

#[cfg(test)]
pub mod fake {
    use std::{cell::RefCell, collections::VecDeque, ffi::CString, io, rc::Rc, time::Duration};

    use super::{Handle, Hid, Interface};

    /// Answers a written report with the input reports it causes, or fails
    /// the write.
    pub type Responder = Rc<dyn Fn(&[u8]) -> io::Result<Vec<Vec<u8>>>>;

    /// Every report written to any fake handle, with the product id of the
    /// device it went to.
    pub type WriteLog = Rc<RefCell<Vec<(u16, Vec<u8>)>>>;

    /// A device that never answers.
    pub fn silent() -> Responder {
        Rc::new(|_| Ok(vec![]))
    }

    pub fn interface(vendor_id: u16, product_id: u16, usage_page: u16, usage: u16) -> Interface {
        Interface {
            vendor_id,
            product_id,
            usage_page,
            usage,
            product: format!("Device {product_id:04X}"),
            path: CString::new(format!(
                "{vendor_id:04X}:{product_id:04X}:{usage_page:04X}:{usage:04X}"
            ))
            .unwrap(),
        }
    }

    #[derive(Default)]
    pub struct FakeHid {
        pub devices: Vec<(Interface, Responder)>,
        pub writes: WriteLog,
        pub fail_enumeration: bool,
    }

    impl Hid for FakeHid {
        type Handle = FakeHandle;

        fn interfaces(&mut self) -> io::Result<Vec<Interface>> {
            if self.fail_enumeration {
                return Err(io::Error::other("enumeration failed"));
            }
            Ok(self
                .devices
                .iter()
                .map(|(interface, _)| interface.clone())
                .collect())
        }

        fn open(&mut self, interface: &Interface) -> io::Result<FakeHandle> {
            let (_, respond) = self.devices.iter().find(|(i, _)| i == interface).unwrap();
            Ok(FakeHandle::new(
                interface.product_id,
                respond.clone(),
                self.writes.clone(),
            ))
        }
    }

    pub struct FakeHandle {
        product_id: u16,
        respond: Responder,
        writes: WriteLog,
        pending: RefCell<VecDeque<Vec<u8>>>,
        /// When set, reads never time out but return this report instead.
        pub noise: Option<Vec<u8>>,
    }

    impl FakeHandle {
        pub fn new(product_id: u16, respond: Responder, writes: WriteLog) -> Self {
            Self {
                product_id,
                respond,
                writes,
                pending: RefCell::default(),
                noise: None,
            }
        }
    }

    impl Handle for FakeHandle {
        fn write(&self, report: &[u8]) -> io::Result<()> {
            self.writes
                .borrow_mut()
                .push((self.product_id, report.to_vec()));
            self.pending.borrow_mut().extend((self.respond)(report)?);
            Ok(())
        }

        fn read(&self, buffer: &mut [u8], timeout: Duration) -> io::Result<usize> {
            assert!(timeout > Duration::ZERO, "read without time to wait");
            let report = self
                .pending
                .borrow_mut()
                .pop_front()
                .or_else(|| self.noise.clone());
            let Some(report) = report else {
                return Ok(0);
            };
            buffer[..report.len()].copy_from_slice(&report);
            Ok(report.len())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{fake::*, *};

    fn device(usage_page: u16, usage: u16) -> Device {
        Device {
            name: "MX Keys S".into(),
            vendor_id: LOGITECH,
            product_id: 0xB378,
            usage_page,
            usage,
            device_index: 0xFF,
        }
    }

    #[test]
    fn matches_on_ids_and_usage() {
        let interface = interface(LOGITECH, 0xB378, 0xFF43, 0x0202);
        assert!(interface.matches(&device(0xFF43, 0x0202)));
        assert!(!interface.matches(&device(0xFF43, 0x0001)));
        assert!(!interface.matches(&device(0x0001, 0x0202)));
        assert!(!interface.matches(&Device {
            product_id: 0xB034,
            ..device(0xFF43, 0x0202)
        }));
        assert!(!interface.matches(&Device {
            vendor_id: 0x1234,
            ..device(0xFF43, 0x0202)
        }));
    }

    #[test]
    fn displays_config_ready_values() {
        assert_eq!(
            interface(LOGITECH, 0xB378, 0xFF43, 0x0202).to_string(),
            "vendor_id = 0x046D  product_id = 0xB378  usage_page = 0xFF43  usage = 0x0202  Device B378"
        );
    }

    #[test]
    fn lists_logitech_interfaces_in_order() {
        let listed = [
            interface(LOGITECH, 0xB378, 0xFF43, 0x0202),
            interface(0x05AC, 0x0001, 0x0001, 0x0006),
            interface(LOGITECH, 0xB034, 0xFF43, 0x0202),
            interface(LOGITECH, 0xB378, 0x0001, 0x0006),
        ];
        let mut hid = FakeHid {
            devices: listed.iter().map(|i| (i.clone(), silent())).collect(),
            ..FakeHid::default()
        };
        assert_eq!(
            logitech_interfaces(&mut hid).unwrap(),
            [listed[2].clone(), listed[3].clone(), listed[0].clone()]
        );
    }

    #[test]
    fn fails_listing_when_enumeration_fails() {
        let mut hid = FakeHid {
            fail_enumeration: true,
            ..FakeHid::default()
        };
        assert!(logitech_interfaces(&mut hid).is_err());
    }
}
