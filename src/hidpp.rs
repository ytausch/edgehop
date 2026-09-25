//! Just enough HID++ 2.0 to move a device to another Easy-Switch host: look up
//! the ChangeHost feature (0x1814) through IRoot, then call its
//! setCurrentHost function. `edgehop --list` also asks for the device's name
//! (DeviceName, 0x0005) and its hosts (ChangeHost's getHostInfo).
//!
//! Every request is a 20-byte long report:
//!
//! ```text
//! 11 <device index> <feature index> <function << 4 | software id> <params...>
//! ```
//!
//! A reply echoes the first four bytes. An error reply has feature index FF,
//! followed by the request's feature index, function byte, and error code.
//!
//! A Unifying or Bolt receiver answers for a device it cannot reach with a
//! HID++ 1.0 short report instead:
//!
//! ```text
//! 10 <device index> 8F <feature index> <function byte> <error code> 00
//! ```

use std::{
    io,
    time::{Duration, Instant},
};

use log::debug;

use crate::hid::Handle;

const REPORT_ID: u8 = 0x11;
const REPORT_LEN: usize = 20;
const ERROR_FEATURE_INDEX: u8 = 0xFF;
const SHORT_REPORT_ID: u8 = 0x10;
const RECEIVER_ERROR: u8 = 0x8F;
/// The receiver's HID++ 1.0 error codes for a paired device it cannot reach
/// right now, as Solaar reads them: connection request failed, and resource
/// error.
const UNREACHABLE: [u8; 2] = [0x04, 0x09];
/// The HID++ 1.0 error a receiver returns for an empty slot.
pub const UNKNOWN_DEVICE: u8 = 0x08;
/// Tags our requests so their replies can be told apart; any nonzero value
/// works.
const SOFTWARE_ID: u8 = 0x0A;

/// IRoot is always at feature index 0; its function 0 is getFeature.
const IROOT: u8 = 0x00;
const GET_FEATURE: u8 = 0;
const DEVICE_NAME: u16 = 0x0005;
const GET_NAME_LENGTH: u8 = 0;
const GET_NAME: u8 = 1;
const CHANGE_HOST: u16 = 0x1814;
const GET_HOST_INFO: u8 = 0;
const SET_CURRENT_HOST: u8 = 1;

/// Replies arrive within tens of milliseconds. A lost report, which happens
/// while a device wakes up, is recovered faster by resending than by waiting.
const REPLY_TIMEOUT: Duration = Duration::from_millis(250);
const ATTEMPTS: usize = 3;
/// How long setCurrentHost waits for an error before assuming success.
const ERROR_TIMEOUT: Duration = Duration::from_millis(150);

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Io(#[from] io::Error),
    #[error("no reply after {ATTEMPTS} attempts")]
    NoReply,
    #[error("device does not support ChangeHost (0x1814)")]
    Unsupported,
    #[error("device returned HID++ error {0:#04X}")]
    Device(u8),
    #[error("device is not reachable (asleep, out of range, or on another host)")]
    Unreachable,
    #[error("receiver returned HID++ 1.0 error {0:#04X}")]
    Receiver(u8),
}

/// A device's Easy-Switch hosts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Hosts {
    pub count: u8,
    /// Zero-based, as on the wire.
    pub current: u8,
}

/// Tells the device at `device_index` behind `handle` to switch to the
/// zero-based `host`.
pub fn change_host(handle: &impl Handle, device_index: u8, host: u8) -> Result<(), Error> {
    let feature_index =
        feature_index(handle, device_index, CHANGE_HOST)?.ok_or(Error::Unsupported)?;
    debug!("ChangeHost is at feature index {feature_index:#04X}");

    let set_current_host = Request::new(device_index, feature_index, SET_CURRENT_HOST, &[host]);
    handle.write(&set_current_host.0)?;
    // A device that switches drops its connection at once instead of
    // replying, so only an error reply means anything here.
    match await_reply(handle, &set_current_host, ERROR_TIMEOUT) {
        Err(Error::Io(_)) | Ok(_) => Ok(()),
        Err(e) => Err(e),
    }
}

/// The name of the device at `device_index` behind `handle`, or `None` if it
/// does not tell.
pub fn device_name(handle: &impl Handle, device_index: u8) -> Result<Option<String>, Error> {
    let Some(feature_index) = feature_index(handle, device_index, DEVICE_NAME)? else {
        return Ok(None);
    };
    let get_length = Request::new(device_index, feature_index, GET_NAME_LENGTH, &[]);
    let length = call(handle, &get_length)?[0];
    let mut name = Vec::with_capacity(length.into());
    for offset in (0..length).step_by(size_of::<Params>()) {
        let get_name = Request::new(device_index, feature_index, GET_NAME, &[offset]);
        let chunk = call(handle, &get_name)?;
        name.extend_from_slice(&chunk[..chunk.len().min(usize::from(length - offset))]);
    }
    Ok(Some(
        String::from_utf8_lossy(&name)
            .trim_end_matches('\0')
            .to_owned(),
    ))
}

/// The Easy-Switch hosts of the device at `device_index` behind `handle`, or
/// `None` if it cannot switch.
pub fn hosts(handle: &impl Handle, device_index: u8) -> Result<Option<Hosts>, Error> {
    let Some(feature_index) = feature_index(handle, device_index, CHANGE_HOST)? else {
        return Ok(None);
    };
    let get_host_info = Request::new(device_index, feature_index, GET_HOST_INFO, &[]);
    let [count, current, ..] = call(handle, &get_host_info)?;
    Ok(Some(Hosts { count, current }))
}

/// Where the device has `feature`, or `None` if it lacks it.
fn feature_index(
    handle: &impl Handle,
    device_index: u8,
    feature: u16,
) -> Result<Option<u8>, Error> {
    let [high, low] = feature.to_be_bytes();
    let get_feature = Request::new(device_index, IROOT, GET_FEATURE, &[high, low]);
    Ok(match call(handle, &get_feature)?[0] {
        0 => None,
        index => Some(index),
    })
}

/// Sends `request` until it is answered, and returns the reply's parameters.
fn call(handle: &impl Handle, request: &Request) -> Result<Params, Error> {
    for attempt in 1..=ATTEMPTS {
        handle.write(&request.0)?;
        if let Some(params) = await_reply(handle, request, REPLY_TIMEOUT)? {
            return Ok(params);
        }
        debug!("no reply to {:02X?} (attempt {attempt})", request.0);
    }
    Err(Error::NoReply)
}

/// Reads until the reply to `request` arrives, skipping unrelated input such
/// as mouse movement. Returns `None` if there is no reply within `timeout`.
fn await_reply(
    handle: &impl Handle,
    request: &Request,
    timeout: Duration,
) -> Result<Option<Params>, Error> {
    let deadline = Instant::now() + timeout;
    let mut report = [0; REPORT_LEN];
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Ok(None);
        }
        let len = handle.read(&mut report, remaining)?;
        if len == 0 {
            return Ok(None);
        }
        match request.reply(&report[..len]) {
            Reply::Success(params) => return Ok(Some(params)),
            Reply::Error(error) => return Err(error),
            Reply::Unrelated => {}
        }
    }
}

type Params = [u8; REPORT_LEN - 4];

struct Request([u8; REPORT_LEN]);

enum Reply {
    Success(Params),
    Error(Error),
    Unrelated,
}

impl Request {
    fn new(device_index: u8, feature_index: u8, function: u8, params: &[u8]) -> Self {
        let mut report = [0; REPORT_LEN];
        report[..4].copy_from_slice(&[
            REPORT_ID,
            device_index,
            feature_index,
            function << 4 | SOFTWARE_ID,
        ]);
        report[4..4 + params.len()].copy_from_slice(params);
        Self(report)
    }

    /// How `report` answers this request.
    fn reply(&self, report: &[u8]) -> Reply {
        let [_, device_index, feature_index, function, ..] = self.0;
        match *report {
            [REPORT_ID, d, ERROR_FEATURE_INDEX, f, g, code, ..]
                if (d, f, g) == (device_index, feature_index, function) =>
            {
                Reply::Error(Error::Device(code))
            }
            [SHORT_REPORT_ID, d, RECEIVER_ERROR, f, g, code, ..]
                if (d, f, g) == (device_index, feature_index, function) =>
            {
                Reply::Error(if UNREACHABLE.contains(&code) {
                    Error::Unreachable
                } else {
                    Error::Receiver(code)
                })
            }
            [REPORT_ID, d, f, g, ref params @ ..]
                if (d, f, g) == (device_index, feature_index, function) =>
            {
                params.try_into().map_or(Reply::Unrelated, Reply::Success)
            }
            _ => Reply::Unrelated,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{cell::Cell, rc::Rc};

    use super::*;
    use crate::hid::fake::{FakeHandle, Responder, WriteLog, silent};

    const INDEX: u8 = 0xFF;
    const CHANGE_HOST_INDEX: u8 = 0x0A;

    fn report(bytes: &[u8]) -> Vec<u8> {
        let mut report = bytes.to_vec();
        report.resize(REPORT_LEN, 0);
        report
    }

    const GET_FEATURE_REQUEST: [u8; 6] = [0x11, INDEX, 0x00, 0x0A, 0x18, 0x14];
    const SET_HOST_PREFIX: [u8; 4] = [0x11, INDEX, CHANGE_HOST_INDEX, 0x1A];

    /// A device with ChangeHost at `feature_index` that answers setCurrentHost
    /// with `set_host_reply`.
    fn device(feature_index: u8, set_host_reply: Vec<Vec<u8>>) -> Responder {
        Rc::new(move |request| {
            Ok(if request[..6] == GET_FEATURE_REQUEST {
                vec![report(&[0x11, INDEX, 0x00, 0x0A, feature_index])]
            } else {
                set_host_reply.clone()
            })
        })
    }

    fn change_host_with(responder: Responder) -> (Result<(), Error>, Vec<Vec<u8>>) {
        let writes = WriteLog::default();
        let handle = FakeHandle::new(0xB378, responder, writes.clone());
        let result = change_host(&handle, INDEX, 1);
        let written = writes
            .take()
            .into_iter()
            .map(|(_, report)| report)
            .collect();
        (result, written)
    }

    #[test]
    fn looks_up_change_host_and_sets_the_host() {
        let (result, written) = change_host_with(device(CHANGE_HOST_INDEX, vec![]));
        result.unwrap();
        assert_eq!(
            written,
            [
                report(&GET_FEATURE_REQUEST),
                report(&[0x11, INDEX, 0x0A, 0x1A, 0x01])
            ]
        );
    }

    #[test]
    fn addresses_the_configured_device_index() {
        let writes = WriteLog::default();
        let handle = FakeHandle::new(0xC548, silent(), writes.clone());
        assert!(matches!(change_host(&handle, 0x02, 0), Err(Error::NoReply)));
        assert_eq!(
            writes.take()[0].1,
            report(&[0x11, 0x02, 0x00, 0x0A, 0x18, 0x14])
        );
    }

    #[test]
    fn skips_unrelated_reports() {
        let responder: Responder = Rc::new(|request| {
            Ok(if request[..6] == GET_FEATURE_REQUEST {
                vec![
                    vec![0x02, 0x00, 0x05, 0x00],                    // mouse movement
                    report(&[0x11, 0x01, 0x00, 0x0A, 0x01]),         // another device
                    report(&[0x11, INDEX, 0x00, 0x1A, 0x01]),        // another function
                    report(&[0x11, INDEX, 0x05, 0x0A, 0x01]),        // another feature
                    report(&[0x11, INDEX, 0x00, 0x0A])[..10].into(), // truncated
                    report(&[0x11, INDEX, 0xFF, 0x05, 0x0A, 0x01]),  // error for another feature
                    report(&[0x11, INDEX, 0x00, 0x0A, CHANGE_HOST_INDEX]),
                ]
            } else {
                vec![]
            })
        });
        let (result, written) = change_host_with(responder);
        result.unwrap();
        assert_eq!(written[1][..4], SET_HOST_PREFIX);
    }

    #[test]
    fn resends_unanswered_requests() {
        let attempts = Rc::new(Cell::new(0));
        let counter = attempts.clone();
        let answer = device(CHANGE_HOST_INDEX, vec![]);
        let responder: Responder = Rc::new(move |request| {
            if request[..6] == GET_FEATURE_REQUEST {
                counter.set(counter.get() + 1);
                if counter.get() < ATTEMPTS {
                    return Ok(vec![]);
                }
            }
            answer(request)
        });
        let (result, written) = change_host_with(responder);
        result.unwrap();
        assert_eq!(attempts.get(), ATTEMPTS);
        assert_eq!(written.len(), ATTEMPTS + 1);
    }

    #[test]
    fn gives_up_without_a_reply() {
        let (result, written) = change_host_with(silent());
        assert!(matches!(result, Err(Error::NoReply)));
        assert_eq!(written.len(), ATTEMPTS);
    }

    #[test]
    fn gives_up_when_input_never_stops() {
        let writes = WriteLog::default();
        let mut handle = FakeHandle::new(0xB034, silent(), writes.clone());
        handle.noise = Some(vec![0x02, 0x00, 0x05, 0x00]);
        let start = Instant::now();
        assert!(matches!(
            change_host(&handle, INDEX, 1),
            Err(Error::NoReply)
        ));
        assert!(start.elapsed() >= REPLY_TIMEOUT * ATTEMPTS as u32);
    }

    #[test]
    fn fails_when_change_host_is_unsupported() {
        let (result, written) = change_host_with(device(0, vec![]));
        assert!(matches!(result, Err(Error::Unsupported)));
        assert_eq!(written.len(), 1);
    }

    #[test]
    fn reports_errors_from_the_lookup() {
        let responder: Responder =
            Rc::new(|_| Ok(vec![report(&[0x11, INDEX, 0xFF, 0x00, 0x0A, 0x05])]));
        let (result, _) = change_host_with(responder);
        assert!(matches!(result, Err(Error::Device(0x05))));
    }

    #[test]
    fn reports_errors_from_set_current_host() {
        let error = report(&[0x11, INDEX, 0xFF, CHANGE_HOST_INDEX, 0x1A, 0x02]);
        let (result, _) = change_host_with(device(CHANGE_HOST_INDEX, vec![error]));
        assert!(matches!(result, Err(Error::Device(0x02))));
    }

    #[test]
    fn accepts_a_reply_to_set_current_host() {
        let reply = report(&SET_HOST_PREFIX);
        let (result, _) = change_host_with(device(CHANGE_HOST_INDEX, vec![reply]));
        result.unwrap();
    }

    /// A receiver's short error report for a request to the device at INDEX.
    fn receiver_error(request: &[u8], code: u8) -> Vec<u8> {
        vec![0x10, INDEX, 0x8F, request[2], request[3], code, 0x00]
    }

    #[test]
    fn reports_unreachable_devices() {
        for code in [0x04, 0x09] {
            let responder: Responder =
                Rc::new(move |request| Ok(vec![receiver_error(request, code)]));
            let (result, written) = change_host_with(responder);
            assert!(matches!(result, Err(Error::Unreachable)));
            assert_eq!(written.len(), 1);
        }
    }

    #[test]
    fn reports_other_receiver_errors() {
        let responder: Responder = Rc::new(|request| Ok(vec![receiver_error(request, 0x08)]));
        let (result, _) = change_host_with(responder);
        assert!(matches!(result, Err(Error::Receiver(0x08))));
    }

    #[test]
    fn reports_receiver_errors_from_set_current_host() {
        let error = receiver_error(&SET_HOST_PREFIX, 0x09);
        let (result, _) = change_host_with(device(CHANGE_HOST_INDEX, vec![error]));
        assert!(matches!(result, Err(Error::Unreachable)));
    }

    #[test]
    fn skips_receiver_errors_for_other_requests() {
        let responder: Responder = Rc::new(|request| {
            Ok(if request[..6] == GET_FEATURE_REQUEST {
                vec![
                    vec![0x10, 0x01, 0x8F, 0x00, 0x0A, 0x09, 0x00], // another device
                    vec![0x10, INDEX, 0x8F, 0x05, 0x0A, 0x09, 0x00], // another feature
                    vec![0x10, INDEX, 0x8F, 0x00, 0x1A, 0x09, 0x00], // another function
                    vec![0x10, INDEX, 0x41, 0x00, 0x0A, 0x09, 0x00], // not an error
                    report(&[0x11, INDEX, 0x00, 0x0A, CHANGE_HOST_INDEX]),
                ]
            } else {
                vec![]
            })
        });
        let (result, _) = change_host_with(responder);
        result.unwrap();
    }

    #[test]
    fn ignores_read_errors_after_set_current_host() {
        let writes = WriteLog::default();
        let handle = Vanishing {
            inner: FakeHandle::new(0xB378, device(CHANGE_HOST_INDEX, vec![]), writes.clone()),
            reads: Cell::new(0),
        };
        change_host(&handle, INDEX, 1).unwrap();
        assert_eq!(writes.take().len(), 2);
    }

    /// A handle whose reads fail after the first one, like a device that
    /// dropped its connection when it switched.
    struct Vanishing {
        inner: FakeHandle,
        reads: Cell<usize>,
    }

    impl Handle for Vanishing {
        fn write(&self, report: &[u8]) -> io::Result<()> {
            self.inner.write(report)
        }

        fn read(&self, buffer: &mut [u8], timeout: Duration) -> io::Result<usize> {
            self.reads.set(self.reads.get() + 1);
            if self.reads.get() > 1 {
                return Err(io::Error::other("device gone"));
            }
            self.inner.read(buffer, timeout)
        }
    }

    #[test]
    fn fails_when_writing_fails() {
        let (result, _) = change_host_with(Rc::new(|_| Err(io::Error::other("unplugged"))));
        assert_eq!(result.unwrap_err().to_string(), "unplugged");
    }

    const DEVICE_NAME_INDEX: u8 = 0x03;

    /// A device with DeviceName at `feature_index` that is called `name`.
    fn named(feature_index: u8, name: &'static [u8]) -> Responder {
        named_with_length(feature_index, name, u8::try_from(name.len()).unwrap())
    }

    /// Like `named`, but claiming the name is `length` bytes long.
    fn named_with_length(feature_index: u8, name: &'static [u8], length: u8) -> Responder {
        Rc::new(move |request| {
            Ok(match [request[2], request[3], request[4]] {
                [0x00, 0x0A, 0x00] => vec![report(&[0x11, INDEX, 0x00, 0x0A, feature_index])],
                [DEVICE_NAME_INDEX, 0x0A, _] => {
                    vec![report(&[0x11, INDEX, DEVICE_NAME_INDEX, 0x0A, length])]
                }
                [_, _, offset] => {
                    let chunk = &name[usize::from(offset)..name.len().min(offset as usize + 16)];
                    vec![report(
                        &[&[0x11, INDEX, DEVICE_NAME_INDEX, 0x1A], chunk].concat(),
                    )]
                }
            })
        })
    }

    fn name_of(responder: Responder) -> (Result<Option<String>, Error>, Vec<Vec<u8>>) {
        let writes = WriteLog::default();
        let handle = FakeHandle::new(0xB034, responder, writes.clone());
        let result = device_name(&handle, INDEX);
        let written = writes.take().into_iter().map(|(_, r)| r).collect();
        (result, written)
    }

    #[test]
    fn reads_the_name_in_chunks() {
        let (name, written) = name_of(named(DEVICE_NAME_INDEX, b"MX Master 3S Mouse"));
        assert_eq!(name.unwrap().as_deref(), Some("MX Master 3S Mouse"));
        assert_eq!(
            written,
            [
                report(&[0x11, INDEX, 0x00, 0x0A, 0x00, 0x05]),
                report(&[0x11, INDEX, DEVICE_NAME_INDEX, 0x0A]),
                report(&[0x11, INDEX, DEVICE_NAME_INDEX, 0x1A, 0]),
                report(&[0x11, INDEX, DEVICE_NAME_INDEX, 0x1A, 16]),
            ]
        );
    }

    #[test]
    fn reads_a_name_that_fits_one_chunk() {
        let (name, written) = name_of(named(DEVICE_NAME_INDEX, b"MX Keys S"));
        assert_eq!(name.unwrap().as_deref(), Some("MX Keys S"));
        assert_eq!(written.len(), 3);
    }

    #[test]
    fn stops_the_name_at_its_length() {
        let (name, _) = name_of(named_with_length(
            DEVICE_NAME_INDEX,
            b"MX Master 3S Mouse 2",
            18,
        ));
        assert_eq!(name.unwrap().as_deref(), Some("MX Master 3S Mouse"));
    }

    #[test]
    fn trims_padding_from_the_name() {
        let (name, _) = name_of(named(DEVICE_NAME_INDEX, b"MX Keys\0\0"));
        assert_eq!(name.unwrap().as_deref(), Some("MX Keys"));
    }

    #[test]
    fn has_no_name_without_device_name() {
        let (name, written) = name_of(named(0, b"MX Keys S"));
        assert_eq!(name.unwrap(), None);
        assert_eq!(written.len(), 1);
    }

    #[test]
    fn fails_the_name_without_a_reply() {
        assert!(matches!(name_of(silent()).0, Err(Error::NoReply)));
    }

    /// A device with ChangeHost at `feature_index`, on host 2 of 3.
    fn with_hosts(feature_index: u8) -> Responder {
        Rc::new(move |request| {
            Ok(match request[2..4] {
                [0x00, 0x0A] => vec![report(&[0x11, INDEX, 0x00, 0x0A, feature_index])],
                _ => vec![report(&[0x11, INDEX, CHANGE_HOST_INDEX, 0x0A, 3, 1])],
            })
        })
    }

    fn hosts_of(responder: Responder) -> Result<Option<Hosts>, Error> {
        let handle = FakeHandle::new(0xB034, responder, WriteLog::default());
        hosts(&handle, INDEX)
    }

    #[test]
    fn reads_the_hosts() {
        assert_eq!(
            hosts_of(with_hosts(CHANGE_HOST_INDEX)).unwrap(),
            Some(Hosts {
                count: 3,
                current: 1
            })
        );
    }

    #[test]
    fn has_no_hosts_without_change_host() {
        assert_eq!(hosts_of(with_hosts(0)).unwrap(), None);
    }

    #[test]
    fn fails_the_hosts_without_a_reply() {
        assert!(matches!(hosts_of(silent()), Err(Error::NoReply)));
    }

    #[test]
    fn describes_errors() {
        assert_eq!(Error::NoReply.to_string(), "no reply after 3 attempts");
        assert_eq!(
            Error::Unsupported.to_string(),
            "device does not support ChangeHost (0x1814)"
        );
        assert_eq!(
            Error::Device(0x05).to_string(),
            "device returned HID++ error 0x05"
        );
        assert_eq!(
            Error::Unreachable.to_string(),
            "device is not reachable (asleep, out of range, or on another host)"
        );
        assert_eq!(
            Error::Receiver(0x08).to_string(),
            "receiver returned HID++ 1.0 error 0x08"
        );
    }
}
