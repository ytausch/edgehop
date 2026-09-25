//! Just enough HID++ 2.0 to move a device to another Easy-Switch host: look up
//! the ChangeHost feature (0x1814) through IRoot, then call its
//! setCurrentHost function.
//!
//! Every message is a 20-byte long report:
//!
//! ```text
//! 11 <device index> <feature index> <function << 4 | software id> <params...>
//! ```
//!
//! A reply echoes the first four bytes. An error reply has feature index FF,
//! followed by the request's feature index, function byte, and error code.

use std::{
    io,
    time::{Duration, Instant},
};

use log::debug;

use crate::hid::Handle;

const REPORT_ID: u8 = 0x11;
const REPORT_LEN: usize = 20;
const ERROR_FEATURE_INDEX: u8 = 0xFF;
/// Tags our requests so their replies can be told apart; any nonzero value
/// works.
const SOFTWARE_ID: u8 = 0x0A;

/// IRoot is always at feature index 0; its function 0 is getFeature.
const IROOT: u8 = 0x00;
const GET_FEATURE: u8 = 0;
const CHANGE_HOST: u16 = 0x1814;
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
}

/// Tells the device at `device_index` behind `handle` to switch to the
/// zero-based `host`.
pub fn change_host(handle: &impl Handle, device_index: u8, host: u8) -> Result<(), Error> {
    let [high, low] = CHANGE_HOST.to_be_bytes();
    let get_feature = Request::new(device_index, IROOT, GET_FEATURE, &[high, low]);
    let feature_index = match call(handle, &get_feature)?[0] {
        0 => return Err(Error::Unsupported),
        index => index,
    };
    debug!("ChangeHost is at feature index {feature_index:#04X}");

    let set_current_host = Request::new(device_index, feature_index, SET_CURRENT_HOST, &[host]);
    handle.write(&set_current_host.0)?;
    // A device that switches drops its connection at once instead of
    // replying, so only an error reply means anything here.
    match await_reply(handle, &set_current_host, ERROR_TIMEOUT) {
        Err(Error::Device(code)) => Err(Error::Device(code)),
        _ => Ok(()),
    }
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
            Reply::Error(code) => return Err(Error::Device(code)),
            Reply::Unrelated => {}
        }
    }
}

type Params = [u8; REPORT_LEN - 4];

struct Request([u8; REPORT_LEN]);

enum Reply {
    Success(Params),
    Error(u8),
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
                Reply::Error(code)
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
    use std::rc::Rc;

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
        let attempts = Rc::new(std::cell::Cell::new(0));
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

    #[test]
    fn fails_when_writing_fails() {
        let (result, _) = change_host_with(Rc::new(|_| Err(io::Error::other("unplugged"))));
        assert_eq!(result.unwrap_err().to_string(), "unplugged");
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
    }
}
