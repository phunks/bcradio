use clap::{Parser};
use std::fmt::Debug;
use std::sync::Mutex;
use log::LevelFilter;

const ABOUT: &str = "
A command line music player for https://bandcamp.com

[Key]                [Description]
 0-9                  adjust volume
 h                    help
 i                    play info
 s                    free word search
 f                    favorite search
 n                    play next
 m                    menu
 l                    playlist (up:k, down:j, select:enter key)
 p                    play/pause
 Q                    graceful kill
 Ctrl+C               exit";

#[derive(Parser, Debug)]
#[clap(author, version, about = ABOUT)]
pub struct Args {
    /// verbose log
    #[arg(short, long, action = clap::ArgAction::Count)]
    verbose: u8,
    /// disable SSL verification
    #[arg(long, short)]
    no_ssl_verify: bool,
    /// image size
    #[arg(long, short, default_value_t = 30)]
    img_width: u16,
    /// socks5
    #[arg(hide = true, short, long, help = "socks5")]
    proxy: Option<String>,
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

static ARGS: Mutex<Option<Args>> = Mutex::new(None);

pub fn init_args() {
    let arg = Args::parse();
    ARGS.lock().unwrap().replace(arg);
}

pub fn args_verbose_log() -> LevelFilter {
    match ARGS.lock().unwrap().as_ref().unwrap().verbose {
        1 => LevelFilter::Info,
        2 => LevelFilter::Debug,
        3 => LevelFilter::Trace,
        _ => LevelFilter::Off
    }
}
#[test]
fn test_verbose() {
    println!("{:?}", LevelFilter::Info.to_string());
}
pub fn args_no_ssl_verify() -> bool {
    ARGS.lock().unwrap().as_ref().unwrap().no_ssl_verify
}

pub fn args_socks() -> Option<String> {
    ARGS.lock().unwrap().as_ref().unwrap().proxy.clone()
}

pub fn args_img_size() -> u16 {
    match ARGS.lock().unwrap().as_ref().unwrap().img_width {
        100.. => 100,
        ..=10 => 10,
        a => a,
    }
}

pub fn args_genre() -> Option<String> {
    ARGS.lock().unwrap().as_ref().unwrap().genre.to_owned()
}

pub fn args_sub_genre() -> Option<String> {
    ARGS.lock().unwrap().as_ref().unwrap().sub_genre.to_owned()
}

pub fn args_list_devices() -> bool {
    ARGS.lock().unwrap().as_ref().unwrap().list_devices
}

