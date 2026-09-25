//! The TOML configuration file.

use std::{fmt, fs, path::Path, str::FromStr, time::Duration};

use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Deserializer};

use crate::desktop::Edge;

#[derive(Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Config {
    /// How long the cursor has to rest at an edge before switching.
    #[serde(
        rename = "dwell_ms",
        default = "default_dwell",
        deserialize_with = "millis"
    )]
    pub dwell: Duration,
    /// How long after a switch the next one may start at the earliest.
    #[serde(
        rename = "cooldown_ms",
        default = "default_cooldown",
        deserialize_with = "millis"
    )]
    pub cooldown: Duration,
    #[serde(default)]
    pub edges: Edges,
    /// Switched in the order listed.
    pub devices: Vec<Device>,
}

/// The channel each edge switches to; an edge without one does nothing.
#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Edges {
    pub left: Option<Channel>,
    pub right: Option<Channel>,
    pub top: Option<Channel>,
    pub bottom: Option<Channel>,
}

/// A device's HID++ interface, as `edgehop --list` shows it.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Device {
    /// Only used in log messages.
    pub name: String,
    pub vendor_id: u16,
    pub product_id: u16,
    pub usage_page: u16,
    pub usage: u16,
    /// 0xFF for a device connected directly (Bluetooth or USB cable), or the
    /// device's slot (1-6) on a Unifying or Bolt receiver.
    pub device_index: u8,
}

/// An Easy-Switch channel, numbered 1-3 as printed on the device.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(try_from = "u8")]
pub struct Channel(u8);

impl Config {
    pub fn load(path: &Path) -> Result<Self> {
        let text =
            fs::read_to_string(path).with_context(|| format!("cannot read {}", path.display()))?;
        let config: Self =
            toml::from_str(&text).with_context(|| format!("invalid config {}", path.display()))?;
        ensure!(
            !config.devices.is_empty(),
            "{} lists no devices",
            path.display()
        );
        Ok(config)
    }
}

impl Edges {
    pub fn channel(&self, edge: Edge) -> Option<Channel> {
        match edge {
            Edge::Left => self.left,
            Edge::Right => self.right,
            Edge::Top => self.top,
            Edge::Bottom => self.bottom,
        }
    }

    /// The edges that have a channel.
    pub fn configured(&self) -> impl Iterator<Item = Edge> {
        let edges = *self;
        Edge::ALL
            .into_iter()
            .filter(move |&edge| edges.channel(edge).is_some())
    }
}

impl Channel {
    /// The zero-based host number HID++ uses on the wire.
    pub fn host_index(self) -> u8 {
        self.0 - 1
    }
}

impl TryFrom<u8> for Channel {
    type Error = String;

    fn try_from(number: u8) -> Result<Self, String> {
        if (1..=3).contains(&number) {
            Ok(Self(number))
        } else {
            Err(format!(
                "Easy-Switch channel must be 1, 2 or 3, not {number}"
            ))
        }
    }
}

impl FromStr for Channel {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, String> {
        s.parse::<u8>()
            .map_err(|_| format!("Easy-Switch channel must be 1, 2 or 3, not {s:?}"))?
            .try_into()
    }
}

impl fmt::Display for Channel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

fn default_dwell() -> Duration {
    Duration::from_millis(250)
}

fn default_cooldown() -> Duration {
    Duration::from_secs(2)
}

fn millis<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Duration, D::Error> {
    u64::deserialize(deserializer).map(Duration::from_millis)
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use tempfile::NamedTempFile;

    use super::*;

    const DEVICE: &str = r#"
        [[devices]]
        name = "MX Keys S"
        vendor_id = 0x046D
        product_id = 0xB378
        usage_page = 0xFF43
        usage = 0x0202
        device_index = 0xFF
    "#;

    fn channel(number: u8) -> Channel {
        Channel::try_from(number).unwrap()
    }

    fn write_config(text: &str) -> NamedTempFile {
        let mut file = NamedTempFile::new().unwrap();
        file.write_all(text.as_bytes()).unwrap();
        file
    }

    fn load(text: &str) -> Result<Config> {
        Config::load(write_config(text).path())
    }

    #[test]
    fn parses_a_full_config() {
        let text =
            format!("dwell_ms = 100\ncooldown_ms = 500\n[edges]\nleft = 1\nbottom = 3\n{DEVICE}");
        let config = load(&text).unwrap();
        assert_eq!(
            config,
            Config {
                dwell: Duration::from_millis(100),
                cooldown: Duration::from_millis(500),
                edges: Edges {
                    left: Some(channel(1)),
                    bottom: Some(channel(3)),
                    ..Edges::default()
                },
                devices: vec![Device {
                    name: "MX Keys S".into(),
                    vendor_id: 0x046D,
                    product_id: 0xB378,
                    usage_page: 0xFF43,
                    usage: 0x0202,
                    device_index: 0xFF,
                }],
            }
        );
    }

    #[test]
    fn defaults_timing_and_edges() {
        let config = load(DEVICE).unwrap();
        assert_eq!(config.dwell, Duration::from_millis(250));
        assert_eq!(config.cooldown, Duration::from_secs(2));
        assert_eq!(config.edges, Edges::default());
    }

    #[test]
    fn example_config_is_valid() {
        let config: Config = toml::from_str(include_str!("../config.example.toml")).unwrap();
        assert_eq!(config.edges.configured().count(), 1);
        assert_eq!(config.devices.len(), 2);
    }

    #[test]
    fn rejects_a_config_without_devices() {
        let error = load("devices = []").unwrap_err();
        assert!(error.to_string().ends_with("lists no devices"), "{error}");
    }

    #[test]
    fn rejects_unknown_keys() {
        let error = load(&format!("dwell = 100\n{DEVICE}")).unwrap_err();
        assert!(
            format!("{error:#}").contains("unknown field `dwell`"),
            "{error:#}"
        );
    }

    #[test]
    fn rejects_channels_out_of_range() {
        let error = load(&format!("[edges]\nleft = 4\n{DEVICE}")).unwrap_err();
        assert!(
            format!("{error:#}").contains("must be 1, 2 or 3, not 4"),
            "{error:#}"
        );
    }

    #[test]
    fn reports_a_missing_file() {
        let error = Config::load(Path::new("/nonexistent/config.toml")).unwrap_err();
        assert_eq!(error.to_string(), "cannot read /nonexistent/config.toml");
    }

    #[test]
    fn maps_each_edge_to_its_channel() {
        let edges = Edges {
            left: Some(channel(1)),
            right: Some(channel(2)),
            top: Some(channel(3)),
            bottom: None,
        };
        assert_eq!(edges.channel(Edge::Left), Some(channel(1)));
        assert_eq!(edges.channel(Edge::Right), Some(channel(2)));
        assert_eq!(edges.channel(Edge::Top), Some(channel(3)));
        assert_eq!(edges.channel(Edge::Bottom), None);
        assert_eq!(
            edges.configured().collect::<Vec<_>>(),
            [Edge::Left, Edge::Right, Edge::Top]
        );
    }

    #[test]
    fn numbers_hosts_from_zero() {
        assert_eq!(channel(1).host_index(), 0);
        assert_eq!(channel(3).host_index(), 2);
    }

    #[test]
    fn parses_channels_from_the_command_line() {
        assert_eq!("2".parse(), Ok(channel(2)));
        assert_eq!(
            "0".parse::<Channel>(),
            Err("Easy-Switch channel must be 1, 2 or 3, not 0".into())
        );
        assert_eq!(
            "two".parse::<Channel>(),
            Err(r#"Easy-Switch channel must be 1, 2 or 3, not "two""#.into())
        );
    }

    #[test]
    fn displays_the_channel_number() {
        assert_eq!(channel(2).to_string(), "2");
    }
}
