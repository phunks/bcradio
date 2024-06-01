
use std::fmt::Debug;
use std::sync::Mutex;
use clap::Parser;

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
    /// disable SSL verification
    #[arg(long, short)]
    pub no_ssl_verify: bool,
    /// image size
    #[arg(long, short, default_value_t = 30)]
    pub img_width: u16,
    /// genre
    #[arg(short, long, help = "genre")]
    pub genre: Option<String>,
    /// sub genre
    #[arg(short, long, help = "sub genre")]
    pub sub_genre: Option<String>,
}

pub fn about() -> &'static str {
    ABOUT
}

static ARGS: Mutex<Option<Args>> = Mutex::new(None);

pub fn init_args() {
    let arg = Args::parse();
    ARGS.lock().unwrap().replace(arg);
}

pub fn args_no_ssl_verify() -> bool {
    ARGS.lock().unwrap().as_ref().unwrap().no_ssl_verify
}

pub fn args_img_size() -> u16 {
    return match ARGS.lock().unwrap().as_ref().unwrap().img_width {
        100 .. => 100,
        ..= 10 => 10,
        a => a,
    };
}

pub fn args_genre() -> Option<String> {
    ARGS.lock().unwrap().as_ref().unwrap().genre.to_owned()
}

pub fn args_sub_genre() -> Option<String> {
    ARGS.lock().unwrap().as_ref().unwrap().sub_genre.to_owned()
}
