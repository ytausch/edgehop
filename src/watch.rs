//! The `--watch` loop's decisions: sample the cursor, and say when to switch.

use std::time::{Duration, Instant};

use anyhow::{Result, ensure};
use log::{debug, info};

use crate::{
    config::{Channel, Config, Edges},
    desktop::Desktop,
    trigger::Trigger,
};

/// How often the cursor is sampled.
pub const POLL_INTERVAL: Duration = Duration::from_millis(20);

pub struct Watcher {
    edges: Edges,
    trigger: Trigger,
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
        })
    }

    /// Samples the cursor once. Returns the channel to switch to, if any.
    pub fn poll(&mut self, desktop: &impl Desktop, now: Instant) -> Option<Channel> {
        let cursor = desktop.cursor()?;
        let at = desktop.edge_at(cursor, self.edges.configured());
        let edge = self.trigger.update(now, cursor, at)?;
        let channel = self.edges.channel(edge)?;
        info!("cursor rested at the {edge} edge; switching to channel {channel}");
        debug!("cursor at {cursor:?}");
        Some(channel)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::desktop::fake::FakeDesktop;

    const DEVICES: &str = r#"
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

    #[test]
    fn switches_to_the_channel_of_the_edge() {
        let mut watcher = watcher("left = 1\nright = 3").unwrap();
        let desktop = FakeDesktop::new(&[(0, 0, 1920, 1080)]);
        let start = Instant::now();
        desktop.move_to(1919, 500);
        assert_eq!(watcher.poll(&desktop, start), None);
        let channel = watcher.poll(&desktop, start + Duration::from_millis(100));
        assert_eq!(channel, Some(3.try_into().unwrap()));
    }

    #[test]
    fn ignores_edges_without_a_channel() {
        let mut watcher = watcher("right = 1").unwrap();
        let desktop = FakeDesktop::new(&[(0, 0, 1920, 1080)]);
        let start = Instant::now();
        desktop.move_to(0, 0);
        assert_eq!(watcher.poll(&desktop, start), None);
        assert_eq!(watcher.poll(&desktop, start + Duration::from_secs(1)), None);
    }

    #[test]
    fn waits_while_the_cursor_is_unknown() {
        let mut watcher = watcher("right = 1").unwrap();
        let desktop = FakeDesktop::new(&[(0, 0, 1920, 1080)]);
        assert_eq!(watcher.poll(&desktop, Instant::now()), None);
    }

    #[test]
    fn needs_an_edge_with_a_channel() {
        let error = watcher("").err().unwrap();
        assert_eq!(error.to_string(), "no edge has a channel in [edges]");
    }
}
