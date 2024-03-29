
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
 p                    play/pause
 Q                    graceful kill
 Ctrl+C               exit";

#[derive(Parser, Debug)]
#[clap(author, version, about = ABOUT)]
pub struct Args {
    /// disable SSL verification
    #[arg(long)]
    pub no_ssl_verify: bool,
    /// image size
    #[arg(long, short, default_value_t = 30)]
    pub img_width: u16,
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
