
use colored::Colorize;
use std::fmt::Display;
use std::{io, process};
use crossterm::{cursor, execute};
use crossterm::terminal::{Clear, ClearType, disable_raw_mode};
#[cfg(windows)]
use crate::debug_println;

pub fn init() {
    enable_color_on_windows();
    clear_screen();
}
fn enable_color_on_windows() {
    #[cfg(windows)]
    colored::control::set_virtual_terminal(true).unwrap();
}

pub(crate) fn clear_screen() {
    execute!(io::stdout(), Clear(ClearType::All)).unwrap();
    execute!(io::stdout(), cursor::MoveTo(0, 0)).unwrap();
}

pub(crate) fn quit(e: anyhow::Error) -> ! {
    disable_raw_mode().unwrap();
    #[cfg(windows)]
    asio_kill();
    println!("{e}");
    process::exit(0);
}

#[cfg(windows)]
fn try_get_current_executable_name() -> Option<String> {
    std::env::current_exe()
        .ok()?
        .file_name()?
        .to_str()?
        .to_owned()
        .into()
}

#[cfg(windows)]
pub fn asio_kill() {
    // for ASIO Driver
    use sysinfo::{Pid, Signal, System};
    let mut sys = System::new_all();
    sys.refresh_all();
    let exec_name = try_get_current_executable_name().unwrap();
    for process in sys.processes_by_exact_name(&*exec_name) {
        debug_println!("[{}] {}\r", process.pid(), process.name());
        if let Some(process) = sys.process(Pid::from(process.pid().as_u32() as usize)) {
            if process.kill_with(Signal::Kill).is_none() {
                eprintln!("This signal isn't supported on this platform");
            }
        }
    }
}

pub fn print_error(error: impl Display) {
    println!("{} {}", "Error:".bright_red(), error);
}
