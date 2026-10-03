use std::io;

use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use inquire::{Confirm, InquireError, Password, PasswordDisplayMode, Text};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout},
    style::{Style, Stylize},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph},
    Terminal,
};

use crate::libbc::{
    ai::AiConfig,
    ai_key,
    ai_profiles::{self, Profiles},
    terminal::{self, AlternateScreen},
};

/// Owns input until Esc; playback audio is left untouched. Progress rendering
/// and input hand-off are guarded by the caller.
pub fn show() -> Result<()> {
    let _screen = AlternateScreen::enter(false)?;
    let mut term = Terminal::new(CrosstermBackend::new(io::stdout()))?;
    // Never overwrite a malformed existing file with an empty configuration.
    let mut profiles = ai_profiles::load()?;
    let mut selection = ListState::default();
    selection.select(
        profiles
            .profiles
            .iter()
            .position(|p| profiles.active.as_deref() == Some(&p.name))
            .or(Some(0)),
    );
    let mut status =
        "Select a profile and press Enter to activate it. API keys are never displayed.".to_owned();
    loop {
        terminal::ensure_raw_mode()?;
        term.draw(|f| render(f, &profiles, &mut selection, &status))?;
        let Event::Key(key) = event::read()? else {
            continue;
        };
        if key.kind != KeyEventKind::Press {
            continue;
        }
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            return Err(InquireError::OperationInterrupted.into());
        }
        if !key.modifiers.is_empty() && key.modifiers != KeyModifiers::SHIFT {
            continue;
        }
        let len = profiles.profiles.len();
        let index = selection.selected().unwrap_or(0);
        match key.code {
            KeyCode::Esc => return Ok(()),
            KeyCode::Up | KeyCode::Char('k') if len > 0 => {
                selection.select(Some((index + len - 1) % len))
            }
            KeyCode::Down | KeyCode::Char('j') if len > 0 => {
                selection.select(Some((index + 1) % len))
            }
            KeyCode::Enter | KeyCode::Char('a' | 'e' | 'K' | 'S' | 'D' | 'd') => {
                let result = action(key.code, &mut profiles, index);
                match result {
                    Ok(message) => status = message,
                    Err(e)
                        if matches!(
                            e.downcast_ref::<InquireError>(),
                            Some(InquireError::OperationCanceled)
                        ) =>
                    {
                        status = "Canceled.".into()
                    }
                    Err(e)
                        if matches!(
                            e.downcast_ref::<InquireError>(),
                            Some(InquireError::OperationInterrupted)
                        ) =>
                    {
                        return Err(e)
                    }
                    Err(e) => status = format!("Error: {e:#}"),
                }
                // Reload after a save error as well: the on-disk state remains authoritative.
                profiles = ai_profiles::load()?;
                selection.select(Some(index.min(profiles.profiles.len().saturating_sub(1))));
                term.clear()?;
            }
            _ => {}
        }
    }
}

fn render(f: &mut ratatui::Frame, profiles: &Profiles, selection: &mut ListState, status: &str) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(4),
            Constraint::Min(1),
            Constraint::Length(4),
        ])
        .split(f.area());
    f.render_widget(Paragraph::new("j/k or arrows: select   Enter: activate   a: add   e: edit URL/model\nK: set key   S: key status   D: delete key   d: delete profile   Esc: close\nHTTP endpoints send the API key without encryption.")
        .block(Block::default().title("Options — AI profiles").borders(Borders::BOTTOM)), chunks[0]);
    let items: Vec<ListItem> = profiles
        .profiles
        .iter()
        .map(|p| {
            ListItem::new(format!(
                "{} {}  |  {}  |  {}",
                if profiles.active.as_deref() == Some(&p.name) {
                    "*"
                } else {
                    " "
                },
                p.name,
                p.config.url,
                p.config.model
            ))
        })
        .collect();
    if items.is_empty() {
        f.render_widget(
            Paragraph::new("No profiles. Press a to add one."),
            chunks[1],
        );
    } else {
        f.render_stateful_widget(
            List::new(items)
                .highlight_style(Style::new().reversed())
                .highlight_symbol("> "),
            chunks[1],
            selection,
        );
    }
    f.render_widget(
        Paragraph::new(status)
            .block(Block::default().borders(Borders::TOP))
            .wrap(ratatui::widgets::Wrap { trim: false }),
        chunks[2],
    );
}

fn action(key: KeyCode, profiles: &mut Profiles, index: usize) -> Result<String> {
    if key == KeyCode::Char('a') {
        terminal::clear_screen();
        let name = Text::new("Profile name").prompt()?;
        ai_profiles::validate_name(&name)?;
        if profiles.profiles.iter().any(|p| p.name == name) {
            anyhow::bail!("profile already exists; use e to edit it");
        }
        let url = Text::new("API base URL")
            .with_default("https://api.openai.com/v1")
            .prompt()?;
        let model = Text::new("Model").prompt()?;
        profiles.set(&name, AiConfig { url, model })?;
        profiles.save()?;
        return Ok(format!(
            "Profile '{name}' saved. Select it and use K to register its API key."
        ));
    }
    let p = profiles
        .profiles
        .get(index)
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("add a profile first"))?;
    match key {
        KeyCode::Enter => {
            profiles.activate(&p.name)?;
            profiles.save()?;
            Ok(format!("Active profile: {}. Applies to the next AI request; the current song and queue are unchanged.", p.name))
        }
        KeyCode::Char('e') => {
            terminal::clear_screen();
            let url = Text::new("API base URL")
                .with_initial_value(&p.config.url)
                .prompt()?;
            let model = Text::new("Model")
                .with_initial_value(&p.config.model)
                .prompt()?;
            profiles.set(&p.name, AiConfig { url, model })?;
            profiles.save()?;
            Ok(format!("Profile '{}' updated.", p.name))
        }
        KeyCode::Char('K') => {
            terminal::clear_screen();
            let key = Password::new("AI API key")
                .without_confirmation()
                .with_display_mode(PasswordDisplayMode::Hidden)
                .prompt()?;
            ai_key::set_for(&p.name, &key)?;
            Ok(format!(
                "API key for '{}' saved in the OS credential store.",
                p.name
            ))
        }
        KeyCode::Char('S') => Ok(format!(
            "API key for '{}': {}",
            p.name,
            if ai_key::load_for(&p.name)?.is_some() {
                "set"
            } else {
                "not set"
            }
        )),
        KeyCode::Char('D' | 'd') => {
            terminal::clear_screen();
            let message = if key == KeyCode::Char('d') {
                format!("Delete profile '{}' AND its API key?", p.name)
            } else {
                format!("Delete API key for '{}' ?", p.name)
            };
            if !Confirm::new(&message).with_default(false).prompt()? {
                return Ok("Canceled.".into());
            }
            if key == KeyCode::Char('d') {
                ai_profiles::delete(profiles, &p.name)?;
                Ok(format!("Profile '{}' and its key deleted.", p.name))
            } else {
                ai_key::delete_for(&p.name)?;
                Ok(format!("API key for '{}' deleted.", p.name))
            }
        }
        _ => unreachable!(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;

    #[test]
    fn renders_empty_and_populated_profiles_and_small_terminals() {
        let mut profiles = Profiles::default();
        let mut selection = ListState::default();
        selection.select(Some(0));
        let mut term = Terminal::new(TestBackend::new(100, 20)).unwrap();
        term.draw(|f| render(f, &profiles, &mut selection, "Ready"))
            .unwrap();
        let text = format!("{:?}", term.backend().buffer());
        assert!(text.contains("No profiles. Press a to add one."));
        profiles
            .set(
                "local",
                AiConfig {
                    url: "http://localhost:4000/v1".into(),
                    model: "test-model".into(),
                },
            )
            .unwrap();
        term.draw(|f| render(f, &profiles, &mut selection, "Ready"))
            .unwrap();
        let text = format!("{:?}", term.backend().buffer());
        assert!(text.contains("* local"));
        assert!(text.contains("test-model"));
        for (width, height) in [(1, 1), (20, 4), (80, 24)] {
            let mut term = Terminal::new(TestBackend::new(width, height)).unwrap();
            term.draw(|f| render(f, &profiles, &mut selection, "Ready"))
                .unwrap();
        }
    }
}
