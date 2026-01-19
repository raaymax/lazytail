use crate::app::{App, InputMode};
use crate::event::AppEvent;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

/// Handle keyboard input and return corresponding events
/// Does not mutate app state directly - returns events to be processed
pub fn handle_input_event(key: KeyEvent, app: &App) -> Vec<AppEvent> {
    match app.input_mode {
        InputMode::EnteringFilter => handle_filter_input_mode(key),
        InputMode::Normal => handle_normal_mode(key),
    }
}

/// Handle keyboard input in filter input mode
fn handle_filter_input_mode(key: KeyEvent) -> Vec<AppEvent> {
    match key.code {
        KeyCode::Char(c) => vec![AppEvent::FilterInputChar(c)],
        KeyCode::Backspace => vec![AppEvent::FilterInputBackspace],
        KeyCode::Enter => vec![AppEvent::FilterInputSubmit],
        KeyCode::Esc => vec![AppEvent::FilterInputCancel, AppEvent::ClearFilter],
        _ => vec![],
    }
}

/// Handle keyboard input in normal navigation mode
fn handle_normal_mode(key: KeyEvent) -> Vec<AppEvent> {
    match key.code {
        KeyCode::Char('q') => vec![AppEvent::Quit],
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            vec![AppEvent::Quit]
        }
        KeyCode::Down | KeyCode::Char('j') => {
            vec![AppEvent::ScrollDown, AppEvent::DisableFollowMode]
        }
        KeyCode::Up | KeyCode::Char('k') => vec![AppEvent::ScrollUp, AppEvent::DisableFollowMode],
        KeyCode::PageDown => {
            // Page size will be set by caller based on terminal size
            vec![AppEvent::DisableFollowMode]
        }
        KeyCode::PageUp => {
            // Page size will be set by caller based on terminal size
            vec![AppEvent::DisableFollowMode]
        }
        KeyCode::Char('g') => vec![AppEvent::JumpToStart, AppEvent::DisableFollowMode],
        KeyCode::Char('G') => vec![AppEvent::JumpToEnd, AppEvent::DisableFollowMode],
        KeyCode::Char('f') => vec![AppEvent::ToggleFollowMode],
        KeyCode::Char('/') => vec![AppEvent::StartFilterInput],
        KeyCode::Esc => vec![AppEvent::ClearFilter],
        _ => vec![],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::App;

    #[test]
    fn test_quit_on_q() {
        let app = App::new(10);
        let key = KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE);
        let events = handle_input_event(key, &app);
        assert_eq!(events, vec![AppEvent::Quit]);
    }

    #[test]
    fn test_quit_on_ctrl_c() {
        let app = App::new(10);
        let key = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL);
        let events = handle_input_event(key, &app);
        assert_eq!(events, vec![AppEvent::Quit]);
    }

    #[test]
    fn test_scroll_down() {
        let app = App::new(10);
        let key = KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE);
        let events = handle_input_event(key, &app);
        assert_eq!(
            events,
            vec![AppEvent::ScrollDown, AppEvent::DisableFollowMode]
        );
    }

    #[test]
    fn test_scroll_up() {
        let app = App::new(10);
        let key = KeyEvent::new(KeyCode::Char('k'), KeyModifiers::NONE);
        let events = handle_input_event(key, &app);
        assert_eq!(
            events,
            vec![AppEvent::ScrollUp, AppEvent::DisableFollowMode]
        );
    }

    #[test]
    fn test_toggle_follow_mode() {
        let app = App::new(10);
        let key = KeyEvent::new(KeyCode::Char('f'), KeyModifiers::NONE);
        let events = handle_input_event(key, &app);
        assert_eq!(events, vec![AppEvent::ToggleFollowMode]);
    }

    #[test]
    fn test_start_filter_input() {
        let app = App::new(10);
        let key = KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE);
        let events = handle_input_event(key, &app);
        assert_eq!(events, vec![AppEvent::StartFilterInput]);
    }

    #[test]
    fn test_filter_input_char() {
        let mut app = App::new(10);
        app.start_filter_input();
        let key = KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE);
        let events = handle_input_event(key, &app);
        assert_eq!(events, vec![AppEvent::FilterInputChar('a')]);
    }

    #[test]
    fn test_filter_input_backspace() {
        let mut app = App::new(10);
        app.start_filter_input();
        let key = KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE);
        let events = handle_input_event(key, &app);
        assert_eq!(events, vec![AppEvent::FilterInputBackspace]);
    }

    #[test]
    fn test_filter_input_submit() {
        let mut app = App::new(10);
        app.start_filter_input();
        let key = KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);
        let events = handle_input_event(key, &app);
        assert_eq!(events, vec![AppEvent::FilterInputSubmit]);
    }

    #[test]
    fn test_filter_input_cancel() {
        let mut app = App::new(10);
        app.start_filter_input();
        let key = KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE);
        let events = handle_input_event(key, &app);
        assert_eq!(
            events,
            vec![AppEvent::FilterInputCancel, AppEvent::ClearFilter]
        );
    }

    #[test]
    fn test_jump_to_start() {
        let app = App::new(10);
        let key = KeyEvent::new(KeyCode::Char('g'), KeyModifiers::NONE);
        let events = handle_input_event(key, &app);
        assert_eq!(
            events,
            vec![AppEvent::JumpToStart, AppEvent::DisableFollowMode]
        );
    }

    #[test]
    fn test_jump_to_end() {
        let app = App::new(10);
        let key = KeyEvent::new(KeyCode::Char('G'), KeyModifiers::SHIFT);
        let events = handle_input_event(key, &app);
        assert_eq!(
            events,
            vec![AppEvent::JumpToEnd, AppEvent::DisableFollowMode]
        );
    }
}
