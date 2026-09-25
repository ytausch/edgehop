//! Command-line arguments.

use std::path::PathBuf;

use clap::{Args, Parser};

use crate::config::Channel;

#[derive(Debug, Parser)]
#[command(
    version,
    about = "Switch Logitech Easy-Switch devices when the cursor hits a screen edge"
)]
pub struct Cli {
    #[command(flatten)]
    mode: ModeArgs,

    /// Also log every step, not only switches and failures.
    #[arg(short, long)]
    pub verbose: bool,

    /// Config file to use. Defaults to %APPDATA%\edgehop\config.toml on
    /// Windows and ~/Library/Application Support/edgehop/config.toml on macOS.
    #[arg(long, value_name = "PATH")]
    pub config: Option<PathBuf>,
}

#[derive(Debug, Args)]
#[group(required = true, multiple = false)]
struct ModeArgs {
    /// Watch the cursor and switch when it rests at a configured edge.
    #[arg(long)]
    watch: bool,

    /// List the connected Logitech devices, as entries for the config.
    #[arg(long)]
    list: bool,

    /// Switch the configured devices to CHANNEL (1-3) once and exit.
    #[arg(long, value_name = "CHANNEL")]
    switch: Option<Channel>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Mode {
    Watch,
    List,
    Switch(Channel),
}

impl Cli {
    pub fn mode(&self) -> Mode {
        match self.mode {
            ModeArgs {
                switch: Some(channel),
                ..
            } => Mode::Switch(channel),
            ModeArgs { list: true, .. } => Mode::List,
            ModeArgs { .. } => Mode::Watch,
        }
    }
}

#[cfg(test)]
mod tests {
    use clap::{CommandFactory, error::ErrorKind};

    use super::*;

    fn parse(args: &[&str]) -> Result<Cli, clap::Error> {
        Cli::try_parse_from([&["edgehop"], args].concat())
    }

    #[test]
    fn is_well_formed() {
        Cli::command().debug_assert();
    }

    #[test]
    fn selects_the_mode() {
        assert_eq!(parse(&["--watch"]).unwrap().mode(), Mode::Watch);
        assert_eq!(parse(&["--list"]).unwrap().mode(), Mode::List);
        assert_eq!(
            parse(&["--switch", "2"]).unwrap().mode(),
            Mode::Switch(2.try_into().unwrap())
        );
    }

    #[test]
    fn takes_options() {
        let cli = parse(&["--watch", "-v", "--config", "edgehop.toml"]).unwrap();
        assert!(cli.verbose);
        assert_eq!(cli.config, Some(PathBuf::from("edgehop.toml")));
        let cli = parse(&["--list"]).unwrap();
        assert!(!cli.verbose);
        assert_eq!(cli.config, None);
    }

    #[test]
    fn needs_exactly_one_mode() {
        assert_eq!(
            parse(&[]).unwrap_err().kind(),
            ErrorKind::MissingRequiredArgument
        );
        assert_eq!(
            parse(&["--watch", "--list"]).unwrap_err().kind(),
            ErrorKind::ArgumentConflict
        );
    }

    #[test]
    fn rejects_invalid_channels() {
        assert_eq!(
            parse(&["--switch", "4"]).unwrap_err().kind(),
            ErrorKind::ValueValidation
        );
    }
}
