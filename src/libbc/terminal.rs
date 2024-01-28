
use colored::Colorize;
use std::fmt::Display;
use std::{cmp, io, process};
use std::io::StdoutLock;
use crossterm::{cursor, execute};
use crossterm::terminal::{Clear, ClearType, disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::prelude::Style;
use ratatui::style::Stylize;
use ratatui::Terminal;
use ratatui::widgets::Borders;
use tui_textarea::{CursorMove, Input, Key, TextArea};
use viu::app;
use viu::config::Config;
use viuer::Config as ViuerConfig;

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
    execute!(io::stdout(),
        Clear(ClearType::All),
        cursor::MoveTo(0, 0)).unwrap();
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

pub fn show_alt_term<T>(v: Vec<T>, img: Option<Vec<u8>>) -> anyhow::Result<()>
    where
        T: Into<String> + Clone,
{
    let stdout = io::stdout();
    let mut stdout = stdout.lock();

    enable_raw_mode()?;
    execute!(stdout, EnterAlternateScreen, cursor::MoveTo(0, 1), cursor::Hide)?;

    let mut f = true;
    match img {
        Some(img) => {
            let config = Config {
                files: vec![],
                loop_gif: false,
                name: false,
                recursive: false,
                static_gif: false,
                viuer_config: ViuerConfig {
                    width: Option::from(30),
                    height: Option::from(14),
                    absolute_offset: false,
                    use_kitty: false,
                    ..Default::default()
                },
                frame_duration: None,
            };

            app::viu(config, img)?;
        },
        None => f = false,
    }

    let backend = CrosstermBackend::new(stdout);
    let mut term = Terminal::new(backend)?;
    let mut textarea = TextArea::from(v);
    textarea.set_cursor_style(Style::new().hidden());
    textarea.set_block(ratatui::widgets::block::Block::default().borders(Borders::NONE));

    if f {
        draw_img(&mut term, textarea.clone())?;
        textarea.move_cursor(CursorMove::Jump(0, 0));
    } else {
        draw(&mut term, textarea)?
    }

    match crossterm::event::read()?.into() {
        Input {
            key: Key::Esc,
            ..
        } => {},
        Input {
            key: Key::Char(_c), // any
            ..
        } => {},
        Input { .. } => {}
    }

    execute!(
        term.backend_mut(),
        LeaveAlternateScreen,
        cursor::Show,
    )?;

    Ok(())
}

pub fn draw(
    term: &mut Terminal<CrosstermBackend<StdoutLock>>,
    textarea: TextArea
) -> anyhow::Result<()> {
    term.draw(|f| {
        const MIN_HEIGHT: usize = 13;
        let height = cmp::max(textarea.lines().len(), MIN_HEIGHT) as u16;
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(height),
                Constraint::Min(0)
            ].as_slice())
            .split(f.size());
        f.render_widget(textarea.widget(), chunks[0]);
    })?;
    Ok(())
}

pub fn draw_img(
    term: &mut Terminal<CrosstermBackend<StdoutLock>>,
    textarea: TextArea
) -> anyhow::Result<()> {
    term.draw(|f| {
        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Length(32),
                Constraint::Percentage(100),
            ].as_slice())
            .split(f.size());

        f.render_widget(textarea.widget(), chunks[1])
    })?;
    Ok(())
}