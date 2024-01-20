use std::{cmp, io};
use std::fmt::{Debug, Display};

use anyhow::Result;
use clap::Parser;
use crossterm::event::{DisableMouseCapture, EnableMouseCapture};
use crossterm::execute;
use crossterm::terminal::enable_raw_mode;
use crossterm::terminal::{EnterAlternateScreen, LeaveAlternateScreen};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::Terminal;
use ratatui::widgets::Borders;
use tui_textarea::{Key, TextArea};
use tui_textarea::Input;


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
pub struct Args {}

pub fn show_help() -> Result<()> {
    let stdout = io::stdout();
    let mut stdout = stdout.lock();

    enable_raw_mode()?;
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture,)?;

    let backend = CrosstermBackend::new(stdout);
    let mut term = Terminal::new(backend)?;
    term.hide_cursor()?;
    let mut textarea = TextArea::from(ABOUT.split('\n'));
    textarea.set_block(ratatui::widgets::block::Block::default().borders(Borders::NONE));

    term.draw(|f| {
        const MIN_HEIGHT: usize = 13;
        let height = cmp::max(textarea.lines().len(), MIN_HEIGHT) as u16;
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(height), Constraint::Min(0)].as_slice())
            .split(f.size());
        f.render_widget(textarea.widget(), chunks[0]);
    })?;

    loop {
        match crossterm::event::read()?.into() {
            Input {
                key: Key::Esc,
                ..
            } => break,
            Input {
                key: Key::Char('h'),
                ..
            } => break,
            Input { .. } => {}
        }
    }

    execute!(
        term.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    term.show_cursor()?;
    Ok(())
}

