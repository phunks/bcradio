use crossterm::event::{Event, KeyCode, KeyEventKind, KeyModifiers};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    Volume(u8),
    Next,
    TogglePause,
    Info,
    Menu,
    Playlist,
    History,
    FavoriteSearch,
    Search,
    AiPlaylist,
    Options,
    Help,
    Quit,
    CancelQuit,
    Interrupt,
}

impl Command {
    pub fn opens_screen(self) -> bool {
        matches!(
            self,
            Self::Info
                | Self::Menu
                | Self::Playlist
                | Self::History
                | Self::FavoriteSearch
                | Self::Search
                | Self::AiPlaylist
                | Self::Options
                | Self::Help
        )
    }
}

pub fn from_event(event: Event) -> Option<Command> {
    let Event::Key(key) = event else { return None };
    if key.kind != KeyEventKind::Press {
        return None;
    }
    if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
        return Some(Command::Interrupt);
    }
    if !key.modifiers.is_empty() && key.modifiers != KeyModifiers::SHIFT {
        return None;
    }
    match key.code {
        KeyCode::Char(c @ '0'..='9') => Some(Command::Volume(c as u8 - b'0')),
        KeyCode::Char('n') => Some(Command::Next),
        KeyCode::Char('p') => Some(Command::TogglePause),
        KeyCode::Char('i') => Some(Command::Info),
        KeyCode::Char('m') => Some(Command::Menu),
        KeyCode::Char('l') => Some(Command::Playlist),
        KeyCode::Char('f') => Some(Command::FavoriteSearch),
        KeyCode::Char('s') => Some(Command::Search),
        KeyCode::Char('I') => Some(Command::AiPlaylist),
        KeyCode::Char('O') => Some(Command::Options),
        KeyCode::Char('H') => Some(Command::History),
        KeyCode::Char('h') => Some(Command::Help),
        KeyCode::Char('Q') => Some(Command::Quit),
        KeyCode::Esc => Some(Command::CancelQuit),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::KeyEvent;

    fn key(code: KeyCode, modifiers: KeyModifiers) -> Event {
        Event::Key(KeyEvent::new(code, modifiers))
    }

    #[test]
    fn commands_and_modifiers() {
        assert_eq!(
            from_event(key(KeyCode::Char('s'), KeyModifiers::NONE)),
            Some(Command::Search)
        );
        assert_eq!(
            from_event(key(KeyCode::Char('Q'), KeyModifiers::SHIFT)),
            Some(Command::Quit)
        );
        assert_eq!(
            from_event(key(KeyCode::Char('I'), KeyModifiers::SHIFT)),
            Some(Command::AiPlaylist)
        );
        assert!(Command::AiPlaylist.opens_screen());
        assert!(Command::Options.opens_screen());
        assert_eq!(
            from_event(key(KeyCode::Char('O'), KeyModifiers::SHIFT)),
            Some(Command::Options)
        );
        assert!(Command::History.opens_screen());
        assert!(Command::FavoriteSearch.opens_screen());
        assert!(Command::Search.opens_screen());
        assert_eq!(
            from_event(key(KeyCode::Char('H'), KeyModifiers::SHIFT)),
            Some(Command::History)
        );
        assert_eq!(
            from_event(key(KeyCode::Char('h'), KeyModifiers::NONE)),
            Some(Command::Help)
        );
        assert_eq!(
            from_event(key(KeyCode::Char('7'), KeyModifiers::NONE)),
            Some(Command::Volume(7))
        );
        assert_eq!(
            from_event(key(KeyCode::Char('c'), KeyModifiers::CONTROL)),
            Some(Command::Interrupt)
        );
        assert_eq!(
            from_event(key(KeyCode::Esc, KeyModifiers::NONE)),
            Some(Command::CancelQuit)
        );
        assert_eq!(from_event(key(KeyCode::Char('s'), KeyModifiers::ALT)), None);
        assert_eq!(
            from_event(key(KeyCode::Char('x'), KeyModifiers::NONE)),
            None
        );
    }

    #[test]
    fn ignores_non_press_events() {
        let mut event = KeyEvent::new(KeyCode::Char('p'), KeyModifiers::NONE);
        event.kind = KeyEventKind::Release;
        assert_eq!(from_event(Event::Key(event)), None);
        assert_eq!(from_event(Event::Resize(80, 24)), None);
    }
}
