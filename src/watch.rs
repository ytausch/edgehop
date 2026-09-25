//! The `--watch` loop's decisions: sample the cursor, say when to switch, and
//! retry devices that did not switch.

use std::time::{Duration, Instant};

use anyhow::{Result, ensure};
use log::{debug, info, warn};

use crate::{
    config::{Channel, Config, Device, Edges},
    desktop::{Desktop, Point},
    hid::Hid,
    switch,
    trigger::Trigger,
};

/// How often the cursor is sampled.
pub const POLL_INTERVAL: Duration = Duration::from_millis(20);
/// How often devices that did not switch are tried again.
pub const RETRY_INTERVAL: Duration = Duration::from_millis(500);
/// How long they are tried for at most.
pub const RETRY_FOR: Duration = Duration::from_secs(60);

pub struct Watcher {
    edges: Edges,
    trigger: Trigger,
    devices: Vec<Device>,
    retry: Option<Retry>,
}

/// Devices that did not switch, tried again until they do. A keyboard is
/// often asleep by the time the mouse pushes the cursor against an edge, and
/// a sleeping device cannot be reached until a key press wakes it.
///
/// The retry is called off once the cursor moves: after the switch, the mouse
/// is on the other host, so a moving cursor means it is back (or another mouse
/// is in use here), and the devices left behind should stay.
struct Retry {
    channel: Channel,
    devices: Vec<Device>,
    /// The cursor at the first sample after the switch, once there is one.
    /// Not taken during the switch, as the mouse may still move it until it
    /// has switched itself.
    cursor: Option<Point>,
    next: Instant,
    until: Instant,
}

impl Watcher {
    pub fn new(config: &Config) -> Result<Self> {
        ensure!(
            config.edges.configured().next().is_some(),
            "no edge has a channel in [edges]"
        );
        Ok(Self {
            edges: config.edges,
            trigger: Trigger::new(config.dwell, config.cooldown),
            devices: config.devices.clone(),
            retry: None,
        })
    }

    /// Samples the cursor once, and switches or retries the devices when it
    /// is time to.
    pub fn poll(&mut self, desktop: &impl Desktop, hid: &mut impl Hid, now: Instant) {
        let cursor = desktop.cursor();
        if let Some(channel) = cursor.and_then(|cursor| self.triggered(desktop, cursor, now)) {
            let failed = switch::switch_all(hid, &self.devices, channel);
            self.retry = Retry::new(channel, failed, now);
        } else if let Some(retry) = &mut self.retry
            && !retry.poll(hid, cursor, now)
        {
            self.retry = None;
        }
    }

    /// Feeds `cursor` to the trigger. Returns the channel to switch to, if
    /// any.
    fn triggered(
        &mut self,
        desktop: &impl Desktop,
        cursor: Point,
        now: Instant,
    ) -> Option<Channel> {
        let at = desktop.edge_at(cursor, self.edges.configured());
        let edge = self.trigger.update(now, cursor, at)?;
        let channel = self.edges.channel(edge)?;
        info!("cursor rested at the {edge} edge; switching to channel {channel}");
        debug!("cursor at {cursor:?}");
        Some(channel)
    }
}

impl Retry {
    /// Starts retrying `devices`, which did not switch to `channel` at `now`.
    fn new(channel: Channel, devices: Vec<Device>, now: Instant) -> Option<Self> {
        if devices.is_empty() {
            return None;
        }
        let names = names(&devices);
        info!("{names}: retrying for up to {RETRY_FOR:?}, or until the cursor moves");
        Some(Self {
            channel,
            devices,
            cursor: None,
            next: now + RETRY_INTERVAL,
            until: now + RETRY_FOR,
        })
    }

    /// Tries the devices again if it is time to. Returns whether to go on
    /// retrying.
    fn poll(&mut self, hid: &mut impl Hid, cursor: Option<Point>, now: Instant) -> bool {
        match (self.cursor, cursor) {
            (None, _) => self.cursor = cursor,
            (Some(before), Some(cursor)) if cursor != before => {
                let names = names(&self.devices);
                info!("{names}: cursor moved, so no longer retrying");
                return false;
            }
            _ => {}
        }
        if now >= self.until {
            let (names, channel) = (names(&self.devices), self.channel);
            warn!("{names}: gave up switching to channel {channel}");
            return false;
        }
        if now >= self.next {
            self.devices = switch::retry_all(hid, &self.devices, self.channel);
            self.next = now + RETRY_INTERVAL;
        }
        !self.devices.is_empty()
    }
}

fn names(devices: &[Device]) -> String {
    let names: Vec<_> = devices.iter().map(|device| device.name.as_str()).collect();
    names.join(", ")
}

#[cfg(test)]
mod tests {
    use std::{io, rc::Rc};

    use super::*;
    use crate::{
        desktop::fake::FakeDesktop,
        hid::fake::{FakeHid, Responder, connected, hidpp_device, hosts_set},
    };

    const KEYBOARD: u16 = 0xB378;
    const MOUSE: u16 = 0xB034;

    const DEVICES: &str = r#"
        [[devices]]
        name = "MX Keys S"
        vendor_id = 0x046D
        product_id = 0xB378
        usage_page = 0xFF43
        usage = 0x0202
        device_index = 0xFF

        [[devices]]
        name = "MX Master 3S"
        vendor_id = 0x046D
        product_id = 0xB034
        usage_page = 0xFF43
        usage = 0x0202
        device_index = 0xFF
    "#;

    fn watcher(edges: &str) -> Result<Watcher> {
        let config: Config =
            toml::from_str(&format!("dwell_ms = 100\n[edges]\n{edges}\n{DEVICES}")).unwrap();
        Watcher::new(&config)
    }

    /// A watcher, switching to channel 3 at the right edge of one display.
    struct Harness {
        watcher: Watcher,
        desktop: FakeDesktop,
        hid: FakeHid,
        start: Instant,
    }

    impl Harness {
        fn new(hid: FakeHid) -> Self {
            Self {
                watcher: watcher("right = 3").unwrap(),
                desktop: FakeDesktop::new(&[(0, 0, 1920, 1080)]),
                hid,
                start: Instant::now(),
            }
        }

        fn at(&mut self, ms: u64) {
            let now = self.start + Duration::from_millis(ms);
            self.watcher.poll(&self.desktop, &mut self.hid, now);
        }

        /// Switches at 100 ms.
        fn switch(&mut self) {
            self.desktop.move_to(1919, 500);
            self.at(0);
            self.at(100);
        }

        /// Makes the keyboard answer from now on.
        fn wake_keyboard(&mut self) {
            self.hid.devices[0].1 = hidpp_device();
        }

        /// How many reports went to the keyboard.
        fn keyboard_writes(&self) -> usize {
            let writes = self.hid.writes.borrow();
            writes.iter().filter(|&&(id, _)| id == KEYBOARD).count()
        }
    }

    /// A mouse, and a keyboard that fails every write until woken.
    fn asleep() -> FakeHid {
        let mut hid = connected(&[KEYBOARD, MOUSE]);
        let asleep: Responder = Rc::new(|_| Err(io::Error::other("asleep")));
        hid.devices[0].1 = asleep;
        hid
    }

    #[test]
    fn switches_to_the_channel_of_the_edge() {
        let mut watcher = watcher("left = 1\nright = 3").unwrap();
        let desktop = FakeDesktop::new(&[(0, 0, 1920, 1080)]);
        let mut hid = connected(&[KEYBOARD, MOUSE]);
        let start = Instant::now();
        desktop.move_to(1919, 500);
        watcher.poll(&desktop, &mut hid, start);
        assert_eq!(hosts_set(&hid), []);
        watcher.poll(&desktop, &mut hid, start + Duration::from_millis(100));
        assert_eq!(hosts_set(&hid), [(KEYBOARD, 2), (MOUSE, 2)]);
    }

    #[test]
    fn ignores_edges_without_a_channel() {
        let mut harness = Harness::new(connected(&[KEYBOARD, MOUSE]));
        harness.desktop.move_to(0, 0);
        harness.at(0);
        harness.at(1000);
        assert!(harness.hid.writes.borrow().is_empty());
    }

    #[test]
    fn waits_while_the_cursor_is_unknown() {
        let mut harness = Harness::new(connected(&[KEYBOARD, MOUSE]));
        harness.at(0);
        harness.at(1000);
        assert!(harness.hid.writes.borrow().is_empty());
    }

    #[test]
    fn needs_an_edge_with_a_channel() {
        let error = watcher("").err().unwrap();
        assert_eq!(error.to_string(), "no edge has a channel in [edges]");
    }

    #[test]
    fn retries_a_device_until_it_switches() {
        let mut harness = Harness::new(asleep());
        harness.switch();
        assert_eq!(hosts_set(&harness.hid), [(MOUSE, 2)]);
        assert_eq!(harness.keyboard_writes(), 1);
        harness.at(120);
        harness.at(599);
        assert_eq!(harness.keyboard_writes(), 1);
        harness.at(600);
        assert_eq!(harness.keyboard_writes(), 2);
        harness.at(1099);
        assert_eq!(harness.keyboard_writes(), 2);
        harness.wake_keyboard();
        harness.at(1100);
        assert_eq!(hosts_set(&harness.hid), [(MOUSE, 2), (KEYBOARD, 2)]);
        harness.at(1600);
        harness.at(2100);
        assert_eq!(hosts_set(&harness.hid), [(MOUSE, 2), (KEYBOARD, 2)]);
    }

    #[test]
    fn retries_while_the_cursor_is_unknown() {
        let mut harness = Harness::new(asleep());
        harness.switch();
        harness.desktop.cursor.set(None);
        harness.at(120);
        harness.desktop.move_to(1919, 500);
        harness.at(140);
        harness.wake_keyboard();
        harness.at(600);
        assert_eq!(hosts_set(&harness.hid), [(MOUSE, 2), (KEYBOARD, 2)]);
    }

    #[test]
    fn stops_retrying_once_the_cursor_moves() {
        let mut harness = Harness::new(asleep());
        harness.switch();
        harness.at(120);
        harness.desktop.move_to(1900, 500);
        harness.at(140);
        harness.desktop.move_to(1919, 500);
        harness.wake_keyboard();
        harness.at(600);
        harness.at(1100);
        assert_eq!(harness.keyboard_writes(), 1);
    }

    #[test]
    fn gives_up_retrying_eventually() {
        let mut harness = Harness::new(asleep());
        harness.switch();
        harness.at(120);
        let end = u64::try_from((RETRY_FOR + Duration::from_millis(100)).as_millis()).unwrap();
        harness.at(end - 1);
        let writes = harness.keyboard_writes();
        assert_eq!(writes, 2);
        harness.wake_keyboard();
        harness.at(end);
        harness.at(end + 500);
        assert_eq!(harness.keyboard_writes(), writes);
    }

    #[test]
    fn needs_no_retry_when_every_device_switched() {
        let mut harness = Harness::new(connected(&[KEYBOARD, MOUSE]));
        harness.switch();
        harness.at(600);
        harness.at(1100);
        assert_eq!(hosts_set(&harness.hid), [(KEYBOARD, 2), (MOUSE, 2)]);
    }

    #[test]
    fn names_devices_for_the_log() {
        let config: Config = toml::from_str(DEVICES).unwrap();
        assert_eq!(names(&config.devices), "MX Keys S, MX Master 3S");
    }
}
