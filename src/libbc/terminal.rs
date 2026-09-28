use colored_text::Colorize;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, Clear, ClearType, EnterAlternateScreen, LeaveAlternateScreen,
};
use crossterm::{cursor, execute};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::prelude::Style;
use ratatui::style::Stylize;
use ratatui::widgets::Borders;
use ratatui::Terminal;
use std::fmt::Display;
use std::io::StdoutLock;
use std::{cmp, io};
use tui_textarea::{CursorMove, Input, Key, TextArea};
use viu::app;
use viu::config::Config;
use viuer::Config as ViuerConfig;

use crate::libbc::args::args_img_size;
#[cfg(windows)]
use log::info;

pub fn init() {
    enable_color_on_windows();
    clear_screen();
}
pub fn ensure_raw_mode() -> io::Result<()> {
    if !crossterm::terminal::is_raw_mode_enabled()? {
        enable_raw_mode()?;
    }
    Ok(())
}

/// Restores the normal screen even when a modal view exits with an error.
pub struct AlternateScreen {
    mouse: bool,
}

impl AlternateScreen {
    pub fn enter(mouse: bool) -> io::Result<Self> {
        ensure_raw_mode()?;
        execute!(io::stdout(), EnterAlternateScreen)?;
        if mouse {
            if let Err(e) = execute!(io::stdout(), crossterm::event::EnableMouseCapture) {
                let _ = execute!(io::stdout(), LeaveAlternateScreen);
                return Err(e);
            }
        }
        Ok(Self { mouse })
    }
}

impl Drop for AlternateScreen {
    fn drop(&mut self) {
        if self.mouse {
            let _ = execute!(io::stdout(), crossterm::event::DisableMouseCapture);
        }
        let _ = execute!(io::stdout(), LeaveAlternateScreen, cursor::Show);
    }
}
fn enable_color_on_windows() {
    #[cfg(windows)]
    colored::control::set_virtual_terminal(true).unwrap();
}

pub(crate) fn clear_screen() {
    execute!(io::stdout(), Clear(ClearType::All), cursor::MoveTo(0, 0)).unwrap();
}
pub struct Quit;
impl Drop for Quit {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), cursor::Show);
        #[cfg(windows)]
        asio_kill();
    }
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
        info!("[{}] {}\r", process.pid(), process.name());
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

pub fn show_alt_term<T>(v: &Vec<T>, img: Option<Vec<u8>>) -> anyhow::Result<()>
where
    T: Into<String>,
    String: for<'a> From<&'a T>,
{
    let _screen = AlternateScreen::enter(false)?;
    let stdout = io::stdout();
    let mut stdout = stdout.lock();
    execute!(stdout, cursor::MoveTo(0, 1), cursor::Hide)?;

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
                    width: Option::from(args_img_size() as u32),
                    height: Option::from(args_img_size() as u32 / 2 - 1),
                    absolute_offset: false,
                    ..Default::default()
                },
                frame_duration: None,
            };

            app::viu(config, img)?;
        }
        None => f = false,
    }

    let backend = CrosstermBackend::new(stdout);
    let mut term = Terminal::new(backend)?;
    let mut textarea = TextArea::from(v);
    textarea.set_cursor_style(Style::default().hidden());
    textarea.set_block(ratatui::widgets::block::Block::default().borders(Borders::NONE));

    if f {
        draw_img(&mut term, textarea.clone())?;
        textarea.move_cursor(CursorMove::Jump(0, 0));
    } else {
        draw(&mut term, textarea)?
    }

    loop {
        match crossterm::event::read()?.into() {
            Input { key: Key::Esc, .. } => break,
            Input {
                key: Key::Char(_c), // any
                ..
            } => break,
            Input { .. } => {}
        }
    }

    Ok(())
}

pub fn draw(
    term: &mut Terminal<CrosstermBackend<StdoutLock>>,
    textarea: TextArea,
) -> anyhow::Result<()> {
    term.draw(|f| {
        const MIN_HEIGHT: usize = 13;
        let height = cmp::max(textarea.lines().len(), MIN_HEIGHT) as u16;
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(height), Constraint::Min(0)].as_slice())
            .split(f.area());
        f.render_widget(&textarea, chunks[0]);
    })?;
    Ok(())
}

pub fn draw_img(
    term: &mut Terminal<CrosstermBackend<StdoutLock>>,
    textarea: TextArea,
) -> anyhow::Result<()> {
    term.draw(|f| {
        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints(
                [
                    Constraint::Length(args_img_size() + 1),
                    Constraint::Percentage(100),
                ]
                .as_slice(),
            )
            .split(f.area());

        f.render_widget(&textarea, chunks[1])
    })?;
    Ok(())
}

pub fn show_alt_term2<T>(v: &Vec<T>) -> anyhow::Result<Option<usize>>
where
    T: Into<String>,
    String: for<'a> From<&'a T>,
{
    let _screen = AlternateScreen::enter(false)?;
    let stdout = io::stdout();
    let mut stdout = stdout.lock();
    execute!(stdout, cursor::MoveTo(0, 1))?;

    let backend = CrosstermBackend::new(stdout);
    let mut term = Terminal::new(backend)?;
    let max = v.len() - 1;
    let mut textarea = TextArea::from(v);
    textarea.set_cursor_style(Style::new().hidden());
    textarea.set_block(ratatui::widgets::block::Block::default().borders(Borders::NONE));

    let mut line = None;
    loop {
        draw(&mut term, textarea.clone())?;
        match crossterm::event::read()?.into() {
            Input { key: Key::Esc, .. } => break,
            Input { key: Key::Up, .. }
            | Input {
                key: Key::Char('k'),
                ..
            } => match textarea.cursor().0 {
                0 => textarea.move_cursor(CursorMove::Bottom),
                _ => textarea.move_cursor(CursorMove::Up),
            },
            Input { key: Key::Down, .. }
            | Input {
                key: Key::Char('j'),
                ..
            } => {
                if textarea.cursor().0 == max {
                    textarea.move_cursor(CursorMove::Top);
                } else {
                    textarea.move_cursor(CursorMove::Down);
                }
            }
            Input {
                key: Key::Char(_c), // any
                ..
            } => break,
            Input {
                key: Key::Enter, ..
            } => {
                line = match textarea.cursor().0 {
                    0 => None,
                    _ => Some(textarea.cursor().0),
                };
                break;
            }
            Input { .. } => {}
        }
    }

    Ok(line)
}
