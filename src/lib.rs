//! Switch Logitech Easy-Switch devices to another host when the cursor is
//! pushed against an outer edge of the desktop.
//!
//! This library holds everything that does not touch the operating system, so
//! it can be tested without hardware. The binary adds the platform glue: the
//! real cursor and displays, and hidapi.

pub mod cli;
pub mod config;
pub mod desktop;
pub mod hid;
pub mod hidpp;
pub mod switch;
pub mod trigger;
pub mod watch;
