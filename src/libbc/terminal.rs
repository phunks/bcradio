use colored::Colorize;
use console::Term;
use once_cell::sync::Lazy;
use std::fmt::Display;
use crate::debug_println;

static STDOUT: Lazy<Term> = Lazy::new(Term::stdout);

pub fn init() {
    ctrlc_handler();
    enable_color_on_windows();
    clear_screen();
}
fn enable_color_on_windows() {
    #[cfg(windows)]
    colored::control::set_virtual_terminal(true).unwrap();
}

fn clear_screen() {
    STDOUT.clear_screen().expect("failed to clear screen");
}

pub fn read_char() -> std::io::Result<char> {
    STDOUT.read_char()
}

fn try_get_current_executable_name() -> Option<String> {
    std::env::current_exe()
        .ok()?
        .file_name()?
        .to_str()?
        .to_owned()
        .into()
}

fn ctrlc_handler() {
    #[cfg(windows)]
    ctrlc::set_handler(move || {
        asio_kill()
    }).expect("Error setting Ctrl-C handler")
    // tokio::spawn(async move {
    //     tokio::signal::ctrl_c().await.unwrap();
    //     asio_kill();
    // });
}

#[cfg(windows)]
pub fn asio_kill() { // for ASIO Driver
    use sysinfo::{Pid, PidExt, Signal, ProcessExt, System, SystemExt};
    let mut sys = System::new_all();
    sys.refresh_all();
    let exec_name = try_get_current_executable_name().unwrap();
    for process in sys.processes_by_exact_name(&*exec_name) {
        debug_println!("[{}] {}", process.pid(), process.name());
        if let Some(process) = sys.process(Pid::from(process.pid().as_u32() as usize)) {
            if process.kill_with(Signal::Kill).is_none() {
                eprintln!("This signal isn't supported on this platform");
            }
        }
    }
    std::process::exit(0);
}

pub fn print_error(error: impl Display) {
    println!("{} {}", "Error:".bright_red(), error);
}

/// You should create an instance of `CleanUpHelper` by calling this method when the programs starts.
///
/// # The Problem
///
/// This program handles keyboard input (adjust volume) by spawning a thread
/// and calling [console](https://github.com/console-rs/console) crate's `console::Term::stdout().read_char()` in a loop.
///
/// This is how `console::Term::stdout().read_char()` works on Unix-like OS:
/// 1. Call the method
/// 2. Your terminal exits "canonical" mode and enters "raw" mode
/// 3. The method blocks until you press a key
/// 4. Terminal exits "raw" mode and returns to "canonical" mode
/// 5. The method returns the key you pressed
///
/// Unfortunately, on Unix-like OS, if the program exits accidentally when `console::Term::stdout().read_char()` is blocking,
/// your terminal will stay in "raw" mode and the terminal output will get messy:
///
/// - https://github.com/console-rs/console/issues/36
/// - https://github.com/console-rs/console/issues/136
///
/// # The Workaround
///
/// This method will create an instance of `CleanUpHelper` struct, which implements `Drop` trait.
/// When it drops, it will send SIGINT (Ctrl+C) signal to the program itself on Unix-like OS, which fixes the bug.
/// Rust's Drop trait will guarantee the method to be called.
pub const fn create_clean_up_helper() -> CleanUpHelper {
    CleanUpHelper {}
}

pub struct CleanUpHelper {}

impl Drop for CleanUpHelper {
    fn drop(&mut self) {
        #[cfg(unix)]
        unsafe {
            libc::raise(libc::SIGINT);
        }
    }
}
