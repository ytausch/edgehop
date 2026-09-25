use std::{path::PathBuf, thread, time::Instant};

use anyhow::{Result, bail};
use clap::Parser;
use log::{LevelFilter, info};

use edgehop::{
    cli::{Cli, Mode},
    config::{Channel, Config},
    hid, switch,
    watch::{POLL_INTERVAL, Watcher},
};

mod sys;

fn main() -> Result<()> {
    let cli = Cli::parse();
    env_logger::Builder::new()
        .filter_module(
            "edgehop",
            if cli.verbose {
                LevelFilter::Debug
            } else {
                LevelFilter::Info
            },
        )
        .format_target(false)
        .init();

    let mut hid = sys::HidApi::new()?;
    match cli.mode() {
        Mode::List => list(&mut hid),
        Mode::Switch(channel) => switch(&load_config(cli.config)?, &mut hid, channel),
        Mode::Watch => watch(&load_config(cli.config)?, &mut hid),
    }
}

fn load_config(path: Option<PathBuf>) -> Result<Config> {
    let path = match path {
        Some(path) => path,
        None => sys::config_path()?,
    };
    let config = Config::load(&path)?;
    info!("using {}", path.display());
    Ok(config)
}

fn list(hid: &mut sys::HidApi) -> Result<()> {
    let interfaces = hid::logitech_interfaces(hid)?;
    if interfaces.is_empty() {
        println!("No Logitech HID interfaces found.");
    }
    for interface in interfaces {
        println!("{interface}");
    }
    Ok(())
}

fn switch(config: &Config, hid: &mut sys::HidApi, channel: Channel) -> Result<()> {
    if !switch::switch_all(hid, &config.devices, channel).is_empty() {
        bail!("not every device switched to channel {channel}");
    }
    Ok(())
}

fn watch(config: &Config, hid: &mut sys::HidApi) -> Result<()> {
    let desktop = sys::Desktop::new()?;
    let mut watcher = Watcher::new(config)?;
    info!("watching the cursor");
    loop {
        watcher.poll(&desktop, hid, Instant::now());
        thread::sleep(POLL_INTERVAL);
    }
}
