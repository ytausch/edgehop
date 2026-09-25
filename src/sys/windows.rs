//! The cursor and monitors on Windows.
//!
//! Both are in virtual-screen coordinates: the origin is the top-left corner
//! of the primary monitor, and monitors left of or above it have negative
//! coordinates. The process declares itself per-monitor DPI aware so that
//! both are in physical pixels; otherwise Windows scales them per monitor and
//! the edges of a scaled monitor land in the wrong place.

use std::{env, path::PathBuf};

use anyhow::{Context, Result};
use log::{info, warn};
use windows::Win32::{
    Foundation::POINT,
    Graphics::Gdi::{MONITOR_DEFAULTTONULL, MonitorFromPoint},
    System::Console::GetConsoleWindow,
    UI::{
        HiDpi::{DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, SetProcessDpiAwarenessContext},
        WindowsAndMessaging::{GetCursorPos, SW_HIDE, ShowWindow},
    },
};

use edgehop::desktop::{self, Point};

pub struct Desktop;

impl Desktop {
    pub fn new() -> Result<Self> {
        // SAFETY: plain Win32 call without pointers.
        if let Err(e) =
            unsafe { SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) }
        {
            warn!("cannot become DPI aware, edges of scaled monitors may be off: {e}");
        }
        Ok(Self)
    }
}

impl desktop::Desktop for Desktop {
    fn cursor(&self) -> Option<Point> {
        let mut point = POINT::default();
        // SAFETY: `point` is a valid, writable POINT. This fails while the
        // secure desktop (lock screen, UAC prompt) is shown.
        unsafe { GetCursorPos(&mut point) }.ok()?;
        Some(Point {
            x: point.x,
            y: point.y,
        })
    }

    fn contains(&self, Point { x, y }: Point) -> bool {
        // SAFETY: plain Win32 call without pointers.
        !unsafe { MonitorFromPoint(POINT { x, y }, MONITOR_DEFAULTTONULL) }.is_invalid()
    }
}

pub fn config_dir() -> Result<PathBuf> {
    env::var_os("APPDATA")
        .map(PathBuf::from)
        .context("APPDATA is not set")
}

/// Hides the console window, and with it its taskbar button. This only works
/// when Windows Console Host is the default terminal: Windows Terminal hosts
/// the console in a window of its own, which this cannot reach.
pub fn hide_console() {
    // SAFETY: plain Win32 call without pointers.
    let window = unsafe { GetConsoleWindow() };
    if window.is_invalid() {
        warn!("cannot hide the console: there is no console window");
        return;
    }
    info!("hiding the console window");
    // SAFETY: `window` is the console window. The return value is only whether
    // the window was visible before.
    let _ = unsafe { ShowWindow(window, SW_HIDE) };
}
