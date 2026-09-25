//! The HID access edgehop needs, as traits so that everything built on it can
//! be tested without hardware.

use std::{
    ffi::CString,
    fmt, io,
    time::{Duration, Instant},
};

use log::debug;

use crate::config::Device;

pub const LOGITECH: u16 = 0x046D;

/// A Unifying or Bolt receiver's HID++ interfaces: one top-level collection
/// for long reports, and one for short reports.
const RECEIVER_USAGE_PAGE: u16 = 0xFF00;
const SHORT_REPORTS: u16 = 0x0001;
const LONG_REPORTS: u16 = 0x0002;

/// How long a [`Joined`] handle waits on one of its handles before trying the
/// other.
const READ_SLICE: Duration = Duration::from_millis(5);

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

    /// The interface that carries the short reports of this receiver's long
    /// report interface, if the platform lists it on its own path. Windows
    /// opens each top-level collection apart, so a receiver's short reports,
    /// among them its errors for devices it cannot reach, never arrive on the
    /// long report interface. macOS lists both with the same path, and a handle
    /// to either gets every report.
    pub fn short_reports<'a>(&self, interfaces: &'a [Interface]) -> Option<&'a Interface> {
        if (self.usage_page, self.usage) != (RECEIVER_USAGE_PAGE, LONG_REPORTS) {
            return None;
        }
        interfaces.iter().find(|interface| {
            (
                interface.vendor_id,
                interface.product_id,
                interface.usage_page,
                interface.usage,
            ) == (
                self.vendor_id,
                self.product_id,
                RECEIVER_USAGE_PAGE,
                SHORT_REPORTS,
            ) && interface.path != self.path
        })
    }
}

/// A handle that also reads from a second one, if there is one. Writes go to
/// the first.
pub struct Joined<H> {
    main: H,
    extra: Option<H>,
}

impl<H: Handle> Joined<H> {
    pub fn new(main: H, extra: Option<H>) -> Self {
        Self { main, extra }
    }
}

impl<H: Handle> Handle for Joined<H> {
    fn write(&self, report: &[u8]) -> io::Result<()> {
        self.main.write(report)
    }

    fn read(&self, buffer: &mut [u8], timeout: Duration) -> io::Result<usize> {
        let Some(extra) = &self.extra else {
            return self.main.read(buffer, timeout);
        };
        let deadline = Instant::now() + timeout;
        loop {
            for handle in [&self.main, extra] {
                let remaining = deadline.saturating_duration_since(Instant::now());
                if remaining.is_zero() {
                    return Ok(0);
                }
                let len = handle.read(buffer, remaining.min(READ_SLICE))?;
                if len > 0 {
                    return Ok(len);
                }
            }
        }
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

/// Opens `interface` for HID++, joined with its short report interface among
/// `interfaces` if it has one: without it, a receiver's error for a device it
/// cannot reach is lost, and the request just times out.
pub fn open_hidpp<H: Hid>(
    hid: &mut H,
    interface: &Interface,
    interfaces: &[Interface],
) -> io::Result<Joined<H::Handle>> {
    let handle = hid.open(interface)?;
    let short_reports = interface.short_reports(interfaces).and_then(|short| {
        hid.open(short)
            .inspect_err(|e| debug!("cannot open {short}: {e}"))
            .ok()
    });
    Ok(Joined::new(handle, short_reports))
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

    use super::{
        Handle, Hid, Interface, LOGITECH, LONG_REPORTS, RECEIVER_USAGE_PAGE, SHORT_REPORTS,
    };

    /// Input reports waiting to be read.
    pub type Queue = Rc<RefCell<VecDeque<Vec<u8>>>>;

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

    /// A device that answers getFeature with ChangeHost at index 0x0A, and
    /// nothing else.
    pub fn hidpp_device() -> Responder {
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

    /// Logitech devices with the given product ids, connected over
    /// Bluetooth.
    pub fn connected(product_ids: &[u16]) -> FakeHid {
        FakeHid {
            devices: product_ids
                .iter()
                .map(|&id| (interface(LOGITECH, id, 0xFF43, 0x0202), hidpp_device()))
                .collect(),
            ..FakeHid::default()
        }
    }

    /// The (product id, host) of every setCurrentHost written to a
    /// [`hidpp_device`].
    pub fn hosts_set(hid: &FakeHid) -> Vec<(u16, u8)> {
        let writes = hid.writes.borrow();
        writes
            .iter()
            .filter(|(_, r)| r[2] == 0x0A)
            .map(|&(id, ref r)| (id, r[4]))
            .collect()
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

    /// Keeps a receiver's short reports apart from its long ones, as Windows
    /// does: short reports caused by writes to a receiver's long report
    /// interface arrive only on its short report interface.
    #[derive(Default)]
    pub struct FakeHid {
        pub devices: Vec<(Interface, Responder)>,
        pub writes: WriteLog,
        pub fail_enumeration: bool,
        /// Interfaces that fail to open.
        pub fail_open: Vec<Interface>,
        pub short_reports: Queue,
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
            if self.fail_open.contains(interface) {
                return Err(io::Error::other("access denied"));
            }
            let (_, respond) = self.devices.iter().find(|(i, _)| i == interface).unwrap();
            let mut handle =
                FakeHandle::new(interface.product_id, respond.clone(), self.writes.clone());
            match (interface.usage_page, interface.usage) {
                (RECEIVER_USAGE_PAGE, LONG_REPORTS) => {
                    handle.short_reports = Some(self.short_reports.clone());
                }
                (RECEIVER_USAGE_PAGE, SHORT_REPORTS) => handle.pending = self.short_reports.clone(),
                _ => {}
            }
            Ok(handle)
        }
    }

    pub struct FakeHandle {
        product_id: u16,
        respond: Responder,
        writes: WriteLog,
        pub pending: Queue,
        /// Where short reports (id 0x10) go instead of `pending`, if set.
        pub short_reports: Option<Queue>,
        /// When set, reads never time out but return this report instead.
        pub noise: Option<Vec<u8>>,
    }

    impl FakeHandle {
        pub fn new(product_id: u16, respond: Responder, writes: WriteLog) -> Self {
            Self {
                product_id,
                respond,
                writes,
                pending: Queue::default(),
                short_reports: None,
                noise: None,
            }
        }
    }

    impl Handle for FakeHandle {
        fn write(&self, report: &[u8]) -> io::Result<()> {
            self.writes
                .borrow_mut()
                .push((self.product_id, report.to_vec()));
            for reply in (self.respond)(report)? {
                match &self.short_reports {
                    Some(short) if reply[0] == 0x10 => short.borrow_mut().push_back(reply),
                    _ => self.pending.borrow_mut().push_back(reply),
                }
            }
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

    fn receiver(usage: u16, path: &str) -> Interface {
        Interface {
            path: CString::new(path).unwrap(),
            ..interface(LOGITECH, 0xC548, 0xFF00, usage)
        }
    }

    #[test]
    fn finds_the_short_reports_of_a_receiver() {
        let long = receiver(0x0002, "col02");
        let short = receiver(0x0001, "col01");
        let interfaces = [
            interface(LOGITECH, 0xC547, 0xFF00, 0x0001),
            interface(LOGITECH, 0xC548, 0xFF01, 0x0001),
            interface(0x1234, 0xC548, 0xFF00, 0x0001),
            long.clone(),
            short.clone(),
        ];
        assert_eq!(long.short_reports(&interfaces), Some(&short));
    }

    #[test]
    fn needs_no_short_reports_on_the_same_path() {
        let long = receiver(0x0002, "receiver");
        let interfaces = [long.clone(), receiver(0x0001, "receiver")];
        assert_eq!(long.short_reports(&interfaces), None);
    }

    #[test]
    fn has_short_reports_only_for_a_receivers_long_reports() {
        let interfaces = [receiver(0x0001, "col01")];
        assert_eq!(receiver(0x0003, "col03").short_reports(&interfaces), None);
        let bluetooth = Interface {
            usage_page: 0xFF43,
            ..receiver(0x0002, "bt")
        };
        assert_eq!(bluetooth.short_reports(&interfaces), None);
    }

    /// A handle that has `reports` waiting, and logs writes to `writes`.
    fn waiting(reports: &[&[u8]], writes: &WriteLog) -> FakeHandle {
        let handle = FakeHandle::new(0xC548, silent(), writes.clone());
        handle
            .pending
            .borrow_mut()
            .extend(reports.iter().map(|r| r.to_vec()));
        handle
    }

    fn read(handle: &impl Handle, timeout: Duration) -> Vec<u8> {
        let mut buffer = [0; 20];
        let len = handle.read(&mut buffer, timeout).unwrap();
        buffer[..len].to_vec()
    }

    #[test]
    fn joined_handles_read_from_both_and_write_to_the_first() {
        let main = WriteLog::default();
        let extra = WriteLog::default();
        let joined = Joined::new(
            waiting(&[&[0x11, 1]], &main),
            Some(waiting(&[&[0x10, 2], &[0x10, 3]], &extra)),
        );
        let timeout = Duration::from_millis(50);
        assert_eq!(read(&joined, timeout), [0x11, 1]);
        assert_eq!(read(&joined, timeout), [0x10, 2]);
        assert_eq!(read(&joined, timeout), [0x10, 3]);
        let start = Instant::now();
        assert_eq!(read(&joined, timeout), []);
        assert!(start.elapsed() >= timeout);
        joined.write(&[0x11, 4]).unwrap();
        assert_eq!(main.take(), [(0xC548, vec![0x11, 4])]);
        assert!(extra.take().is_empty());
    }

    #[test]
    fn joined_handles_read_from_the_first_alone() {
        let joined = Joined::new(waiting(&[&[0x11, 1]], &WriteLog::default()), None);
        assert_eq!(read(&joined, Duration::from_millis(50)), [0x11, 1]);
        assert_eq!(read(&joined, Duration::from_millis(50)), []);
    }

    #[test]
    fn joined_handles_fail_when_a_read_fails() {
        struct Failing;
        impl Handle for Failing {
            fn write(&self, _: &[u8]) -> io::Result<()> {
                Ok(())
            }
            fn read(&self, _: &mut [u8], _: Duration) -> io::Result<usize> {
                Err(io::Error::other("gone"))
            }
        }
        let joined = Joined::new(Failing, Some(Failing));
        joined.write(&[]).unwrap();
        let mut buffer = [0; 20];
        let error = joined
            .read(&mut buffer, Duration::from_millis(50))
            .unwrap_err();
        assert_eq!(error.to_string(), "gone");
    }
}
