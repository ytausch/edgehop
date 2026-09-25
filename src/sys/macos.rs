//! The cursor and displays on macOS.
//!
//! Both come from Quartz, in global display coordinates: the origin is the
//! top-left corner of the main display and y grows downwards. (AppKit's
//! NSEvent and NSScreen put the origin at the bottom left instead; mixing the
//! two would put the top and bottom edges in the wrong place.)

use std::{env, path::PathBuf};

use anyhow::{Context, Result, anyhow};
use core_graphics::{
    display::CGDisplay,
    event::CGEvent,
    event_source::{CGEventSource, CGEventSourceStateID},
    geometry::CGPoint,
};

use edgehop::desktop::{self, Point};

pub struct Desktop {
    source: CGEventSource,
}

impl Desktop {
    pub fn new() -> Result<Self> {
        let source = CGEventSource::new(CGEventSourceStateID::CombinedSessionState)
            .map_err(|()| anyhow!("cannot create a Quartz event source"))?;
        Ok(Self { source })
    }
}

impl desktop::Desktop for Desktop {
    fn cursor(&self) -> Option<Point> {
        // An event created without a type carries the current cursor
        // location. Reading it needs no permission.
        let location = CGEvent::new(self.source.clone()).ok()?.location();
        // Locations are in points and can be fractional; flooring maps the
        // last point before a display's far side onto the display.
        Some(Point {
            x: location.x.floor() as i32,
            y: location.y.floor() as i32,
        })
    }

    fn contains(&self, Point { x, y }: Point) -> bool {
        let point = CGPoint::new(f64::from(x), f64::from(y));
        CGDisplay::display_count_with_point(point).is_ok_and(|count| count > 0)
    }
}

pub fn config_dir() -> Result<PathBuf> {
    let home = env::var_os("HOME").context("HOME is not set")?;
    Ok(PathBuf::from(home).join("Library/Application Support"))
}
