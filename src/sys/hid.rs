//! HID access through hidapi, built from source and linked statically.
//!
//! On macOS the crate's `macos-shared-device` feature opens devices shared
//! instead of exclusively: seizing a keyboard would need root.

use std::{io, time::Duration};

use anyhow::{Context, Result};

use edgehop::hid::{self, Interface};

pub struct HidApi(hidapi::HidApi);

pub struct Handle(hidapi::HidDevice);

impl HidApi {
    pub fn new() -> Result<Self> {
        hidapi::HidApi::new()
            .map(Self)
            .context("cannot initialize hidapi")
    }
}

impl hid::Hid for HidApi {
    type Handle = Handle;

    fn interfaces(&mut self) -> io::Result<Vec<Interface>> {
        self.0.refresh_devices().map_err(io::Error::other)?;
        let interfaces = self.0.device_list().map(|info| Interface {
            vendor_id: info.vendor_id(),
            product_id: info.product_id(),
            usage_page: info.usage_page(),
            usage: info.usage(),
            product: info.product_string().unwrap_or_default().to_owned(),
            path: info.path().to_owned(),
        });
        Ok(interfaces.collect())
    }

    fn open(&mut self, interface: &Interface) -> io::Result<Handle> {
        self.0
            .open_path(&interface.path)
            .map(Handle)
            .map_err(io::Error::other)
    }
}

impl hid::Handle for Handle {
    fn write(&self, report: &[u8]) -> io::Result<()> {
        self.0.write(report).map(drop).map_err(io::Error::other)
    }

    fn read(&self, buffer: &mut [u8], timeout: Duration) -> io::Result<usize> {
        let millis = i32::try_from(timeout.as_millis()).unwrap_or(i32::MAX);
        self.0
            .read_timeout(buffer, millis)
            .map_err(io::Error::other)
    }
}
