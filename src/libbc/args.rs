use clap::Parser;
use std::fmt::Debug;
use std::sync::OnceLock;

#[derive(clap::Subcommand, Debug, Clone, Copy, PartialEq, Eq)]
pub enum AiKeyCommand {
    /// Read a key without echoing it and save it in the OS credential store
    Set,
    /// Check whether a key exists (never prints the key)
    Status,
    /// Remove the key from the OS credential store
    Delete,
}

#[derive(clap::Subcommand, Debug, Clone, PartialEq, Eq)]
pub enum ConfigCommand {
    /// Manage the AI API key in the OS credential store
    AiKey {
        #[command(subcommand)]
        action: AiKeyCommand,
    },
    /// Configure the OpenAI-compatible chat API
    AiConfig {
        #[command(subcommand)]
        action: AiConfigCommand,
    },
}

#[derive(clap::Subcommand, Debug, Clone, PartialEq, Eq)]
pub enum AiConfigCommand {
    /// Save the API base URL and model (never the API key)
    Set {
        #[arg(long)]
        url: String,
        #[arg(long)]
        model: String,
    },
    /// Display the configured API base URL and model
    Show,
}

const ABOUT: &str = "
A command line music player for https://bandcamp.com

[Key]                [Description]
 0-9                  adjust volume
 h                    help
 H                    playback history
 I                    generate AI playlist from a description
 i                    play info
 s                    free word search
 f                    favorite search
 n                    play next
 m                    menu
 l                    playlist (up:k, down:j, select:enter key)
 p                    play/pause
 Q                    graceful kill
 Esc                  cancel a pending Q and resume normal playback
 Ctrl+C               exit";

#[derive(Parser, Debug)]
#[clap(author, version, about = ABOUT)]
pub struct Args {
    #[command(subcommand)]
    command: Option<ConfigCommand>,
    /// verbose log
    #[arg(short, long, action = clap::ArgAction::Count)]
    verbose: u8,
    /// disable SSL verification
    #[arg(long, short)]
    no_ssl_verify: bool,
    /// image size
    #[arg(long, short, default_value_t = 30)]
    img_width: u16,
    /// genre
    #[arg(hide = true, short, long, help = "genre")]
    genre: Option<String>,
    /// sub genre
    #[arg(hide = true, short, long, help = "sub genre")]
    sub_genre: Option<String>,
    /// list host devices
    #[arg(hide = true, short, num_args(0), required = false)]
    list_devices: bool,
}

pub fn about() -> &'static str {
    ABOUT
}

static ARGS: OnceLock<Args> = OnceLock::new();

pub fn init_args() {
    let _ = ARGS.set(Args::parse());
}

fn args() -> &'static Args {
    ARGS.get().expect("arguments must be initialized")
}

pub fn args_verbose_log() -> u8 {
    args().verbose
}

pub fn args_command() -> Option<&'static ConfigCommand> {
    args().command.as_ref()
}
#[test]
fn test_verbose() {
    for (flags, expected) in [
        (&[][..], 0),
        (&["-v"][..], 1),
        (&["-vv"][..], 2),
        (&["-vvv"][..], 3),
    ] {
        let mut argv = vec!["bcradio"];
        argv.extend(flags);
        assert_eq!(Args::try_parse_from(argv).unwrap().verbose, expected);
    }
}

#[cfg(test)]
mod ai_key_tests {
    use super::*;

    #[test]
    fn parses_key_management_without_a_secret_argument() {
        for (action, expected) in [
            ("set", AiKeyCommand::Set),
            ("status", AiKeyCommand::Status),
            ("delete", AiKeyCommand::Delete),
        ] {
            let args = Args::try_parse_from(["bcradio", "ai-key", action]).unwrap();
            assert_eq!(
                args.command,
                Some(ConfigCommand::AiKey { action: expected })
            );
        }
        assert!(Args::try_parse_from(["bcradio", "ai-key", "set", "secret"]).is_err());
        assert!(Args::try_parse_from(["bcradio"]).unwrap().command.is_none());
    }

    #[test]
    fn parses_ai_config_commands() {
        let args = Args::try_parse_from([
            "bcradio",
            "ai-config",
            "set",
            "--url",
            "https://example.com/v1",
            "--model",
            "test",
        ])
        .unwrap();
        assert_eq!(
            args.command,
            Some(ConfigCommand::AiConfig {
                action: AiConfigCommand::Set {
                    url: "https://example.com/v1".into(),
                    model: "test".into(),
                }
            })
        );
        assert!(Args::try_parse_from([
            "bcradio",
            "ai-config",
            "set",
            "--url",
            "https://example.com/v1"
        ])
        .is_err());
    }

    #[test]
    fn rejects_removed_proxy_option() {
        assert!(Args::try_parse_from(["bcradio", "--proxy", "socks5://localhost:1080"]).is_err());
    }
}
pub fn args_no_ssl_verify() -> bool {
    args().no_ssl_verify
}

pub fn args_img_size() -> u16 {
    match args().img_width {
        100.. => 100,
        ..=10 => 10,
        a => a,
    }
}

pub fn args_genre() -> Option<String> {
    args().genre.to_owned()
}

pub fn args_sub_genre() -> Option<String> {
    args().sub_genre.to_owned()
}

pub fn args_list_devices() -> bool {
    args().list_devices
}
