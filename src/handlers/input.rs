use crate::app::{App, InputMode};
use crate::event::AppEvent;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

/// Handle keyboard input and return corresponding events
/// Does not mutate app state directly - returns events to be processed
pub fn handle_input_event(key: KeyEvent, app: &App) -> Vec<AppEvent> {
    // If help is showing, most keys just hide help (except quit)
    if app.show_help {
        return handle_help_mode(key);
    }

    match app.input_mode {
        InputMode::EnteringFilter => handle_filter_input_mode(key),
        InputMode::EnteringLineJump => handle_line_jump_input_mode(key),
        InputMode::ZPending => handle_z_pending_mode(key),
        InputMode::Normal => handle_normal_mode(key),
    }
}

/// Handle keyboard input when help overlay is showing
fn handle_help_mode(key: KeyEvent) -> Vec<AppEvent> {
    match key.code {
        KeyCode::Char('q') => vec![AppEvent::Quit],
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            vec![AppEvent::Quit]
        }
        // Any other key hides help
        _ => vec![AppEvent::HideHelp],
    }
}

/// Handle keyboard input in filter input mode
fn handle_filter_input_mode(key: KeyEvent) -> Vec<AppEvent> {
    match key.code {
        // Alt+C toggles case sensitivity (Ctrl+I doesn't work - same as Tab in terminals)
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::ALT) => {
            vec![AppEvent::ToggleCaseSensitivity]
        }
        // Ctrl+A goes to start of line
        KeyCode::Char('a') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            vec![AppEvent::CursorHome]
        }
        // Ctrl+E goes to end of line
        KeyCode::Char('e') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            vec![AppEvent::CursorEnd]
        }
        KeyCode::Char(c) => vec![AppEvent::FilterInputChar(c)],
        KeyCode::Backspace => vec![AppEvent::FilterInputBackspace],
        KeyCode::Enter => vec![AppEvent::FilterInputSubmit],
        KeyCode::Esc => vec![AppEvent::FilterInputCancel, AppEvent::ClearFilter],
        KeyCode::Up => vec![AppEvent::HistoryUp],
        KeyCode::Down => vec![AppEvent::HistoryDown],
        // Tab toggles between Plain and Regex mode
        KeyCode::Tab => vec![AppEvent::ToggleFilterMode],
        // Cursor navigation
        KeyCode::Left => vec![AppEvent::CursorLeft],
        KeyCode::Right => vec![AppEvent::CursorRight],
        KeyCode::Home => vec![AppEvent::CursorHome],
        KeyCode::End => vec![AppEvent::CursorEnd],
        _ => vec![],
    }
}

/// Handle keyboard input in line jump input mode
fn handle_line_jump_input_mode(key: KeyEvent) -> Vec<AppEvent> {
    match key.code {
        // Ctrl+A goes to start of line
        KeyCode::Char('a') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            vec![AppEvent::CursorHome]
        }
        // Ctrl+E goes to end of line
        KeyCode::Char('e') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            vec![AppEvent::CursorEnd]
        }
        KeyCode::Char(c) => vec![AppEvent::LineJumpInputChar(c)],
        KeyCode::Backspace => vec![AppEvent::LineJumpInputBackspace],
        KeyCode::Enter => vec![AppEvent::LineJumpInputSubmit],
        KeyCode::Esc => vec![AppEvent::LineJumpInputCancel],
        // Cursor navigation
        KeyCode::Left => vec![AppEvent::CursorLeft],
        KeyCode::Right => vec![AppEvent::CursorRight],
        KeyCode::Home => vec![AppEvent::CursorHome],
        KeyCode::End => vec![AppEvent::CursorEnd],
        _ => vec![],
    }
}

/// Handle keyboard input in z pending mode (waiting for zz, zt, zb)
fn handle_z_pending_mode(key: KeyEvent) -> Vec<AppEvent> {
    match key.code {
        KeyCode::Char('z') => vec![AppEvent::CenterView, AppEvent::ExitZMode],
        KeyCode::Char('t') => vec![AppEvent::ViewToTop, AppEvent::ExitZMode],
        KeyCode::Char('b') => vec![AppEvent::ViewToBottom, AppEvent::ExitZMode],
        KeyCode::Esc => vec![AppEvent::ExitZMode],
        // Any other key cancels z mode
        _ => vec![AppEvent::ExitZMode],
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
        KeyCode::Char(':') => vec![AppEvent::StartLineJumpInput],
        KeyCode::Char('?') => vec![AppEvent::ShowHelp],
        KeyCode::Char('z') => vec![AppEvent::EnterZMode],
        KeyCode::Char(' ') => vec![AppEvent::ToggleLineExpansion],
        KeyCode::Char('c') => vec![AppEvent::CollapseAll],
        KeyCode::Esc => vec![AppEvent::ClearFilter],
        // Tab navigation
        KeyCode::Tab => vec![AppEvent::NextTab],
        KeyCode::BackTab => vec![AppEvent::PrevTab],
        // Direct tab selection (1-9)
        KeyCode::Char(c @ '1'..='9') => {
            let index = (c as usize) - ('1' as usize);
            vec![AppEvent::SelectTab(index)]
        }
        _ => vec![],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::App;
    use std::io::Write;
    use tempfile::NamedTempFile;

    fn create_test_app() -> (App, NamedTempFile) {
        let mut file = NamedTempFile::new().unwrap();
        for i in 0..10 {
            writeln!(file, "line{}", i).unwrap();
        }
        file.flush().unwrap();
        let app = App::new(vec![file.path().to_path_buf()], false).unwrap();
        (app, file)
    }

    #[test]
    fn test_quit_on_q() {
        let (app, _file) = create_test_app();
        let key = KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE);
        let events = handle_input_event(key, &app);
        assert_eq!(events, vec![AppEvent::Quit]);
    }

    #[test]
    fn test_quit_on_ctrl_c() {
        let (app, _file) = create_test_app();
        let key = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL);
        let events = handle_input_event(key, &app);
        assert_eq!(events, vec![AppEvent::Quit]);
    }

    #[test]
    fn test_scroll_down() {
        let (app, _file) = create_test_app();
        let key = KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE);
        let events = handle_input_event(key, &app);
        assert_eq!(
            events,
            vec![AppEvent::ScrollDown, AppEvent::DisableFollowMode]
        );
    }

    #[test]
    fn test_scroll_up() {
        let (app, _file) = create_test_app();
        let key = KeyEvent::new(KeyCode::Char('k'), KeyModifiers::NONE);
        let events = handle_input_event(key, &app);
        assert_eq!(
            events,
            vec![AppEvent::ScrollUp, AppEvent::DisableFollowMode]
        );
    }

    #[test]
    fn test_toggle_follow_mode() {
        let (app, _file) = create_test_app();
        let key = KeyEvent::new(KeyCode::Char('f'), KeyModifiers::NONE);
        let events = handle_input_event(key, &app);
        assert_eq!(events, vec![AppEvent::ToggleFollowMode]);
    }

    #[test]
    fn test_start_filter_input() {
        let (app, _file) = create_test_app();
        let key = KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE);
        let events = handle_input_event(key, &app);
        assert_eq!(events, vec![AppEvent::StartFilterInput]);
    }

    #[test]
    fn test_filter_input_char() {
        let (mut app, _file) = create_test_app();
        app.start_filter_input();
        let key = KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE);
        let events = handle_input_event(key, &app);
        assert_eq!(events, vec![AppEvent::FilterInputChar('a')]);
    }

    #[test]
    fn test_filter_input_backspace() {
        let (mut app, _file) = create_test_app();
        app.start_filter_input();
        let key = KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE);
        let events = handle_input_event(key, &app);
        assert_eq!(events, vec![AppEvent::FilterInputBackspace]);
    }

    #[test]
    fn test_filter_input_submit() {
        let (mut app, _file) = create_test_app();
        app.start_filter_input();
        let key = KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);
        let events = handle_input_event(key, &app);
        assert_eq!(events, vec![AppEvent::FilterInputSubmit]);
    }

    #[test]
    fn test_filter_input_cancel() {
        let (mut app, _file) = create_test_app();
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
        let (app, _file) = create_test_app();
        let key = KeyEvent::new(KeyCode::Char('g'), KeyModifiers::NONE);
        let events = handle_input_event(key, &app);
        assert_eq!(
            events,
            vec![AppEvent::JumpToStart, AppEvent::DisableFollowMode]
        );
    }

    #[test]
    fn test_jump_to_end() {
        let (app, _file) = create_test_app();
        let key = KeyEvent::new(KeyCode::Char('G'), KeyModifiers::SHIFT);
        let events = handle_input_event(key, &app);
        assert_eq!(
            events,
            vec![AppEvent::JumpToEnd, AppEvent::DisableFollowMode]
        );
    }

    #[test]
    fn test_show_help() {
        let (app, _file) = create_test_app();
        let key = KeyEvent::new(KeyCode::Char('?'), KeyModifiers::SHIFT);
        let events = handle_input_event(key, &app);
        assert_eq!(events, vec![AppEvent::ShowHelp]);
    }

    #[test]
    fn test_hide_help_on_any_key() {
        let (mut app, _file) = create_test_app();
        app.show_help = true;

        let key = KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE);
        let events = handle_input_event(key, &app);
        assert_eq!(events, vec![AppEvent::HideHelp]);
    }

    #[test]
    fn test_quit_from_help_mode() {
        let (mut app, _file) = create_test_app();
        app.show_help = true;

        let key = KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE);
        let events = handle_input_event(key, &app);
        assert_eq!(events, vec![AppEvent::Quit]);
    }

    #[test]
    fn test_start_line_jump_input() {
        let (app, _file) = create_test_app();
        let key = KeyEvent::new(KeyCode::Char(':'), KeyModifiers::NONE);
        let events = handle_input_event(key, &app);
        assert_eq!(events, vec![AppEvent::StartLineJumpInput]);
    }

    #[test]
    fn test_line_jump_input_char() {
        let (mut app, _file) = create_test_app();
        app.start_line_jump_input();
        let key = KeyEvent::new(KeyCode::Char('5'), KeyModifiers::NONE);
        let events = handle_input_event(key, &app);
        assert_eq!(events, vec![AppEvent::LineJumpInputChar('5')]);
    }

    #[test]
    fn test_line_jump_input_backspace() {
        let (mut app, _file) = create_test_app();
        app.start_line_jump_input();
        let key = KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE);
        let events = handle_input_event(key, &app);
        assert_eq!(events, vec![AppEvent::LineJumpInputBackspace]);
    }

    #[test]
    fn test_line_jump_input_submit() {
        let (mut app, _file) = create_test_app();
        app.start_line_jump_input();
        let key = KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);
        let events = handle_input_event(key, &app);
        assert_eq!(events, vec![AppEvent::LineJumpInputSubmit]);
    }

    #[test]
    fn test_line_jump_input_cancel() {
        let (mut app, _file) = create_test_app();
        app.start_line_jump_input();
        let key = KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE);
        let events = handle_input_event(key, &app);
        assert_eq!(events, vec![AppEvent::LineJumpInputCancel]);
    }

    #[test]
    fn test_filter_input_history_up() {
        let (mut app, _file) = create_test_app();
        app.start_filter_input();
        let key = KeyEvent::new(KeyCode::Up, KeyModifiers::NONE);
        let events = handle_input_event(key, &app);
        assert_eq!(events, vec![AppEvent::HistoryUp]);
    }

    #[test]
    fn test_filter_input_history_down() {
        let (mut app, _file) = create_test_app();
        app.start_filter_input();
        let key = KeyEvent::new(KeyCode::Down, KeyModifiers::NONE);
        let events = handle_input_event(key, &app);
        assert_eq!(events, vec![AppEvent::HistoryDown]);
    }

    #[test]
    fn test_next_tab() {
        let (app, _file) = create_test_app();
        let key = KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE);
        let events = handle_input_event(key, &app);
        assert_eq!(events, vec![AppEvent::NextTab]);
    }

    #[test]
    fn test_prev_tab() {
        let (app, _file) = create_test_app();
        let key = KeyEvent::new(KeyCode::BackTab, KeyModifiers::SHIFT);
        let events = handle_input_event(key, &app);
        assert_eq!(events, vec![AppEvent::PrevTab]);
    }

    #[test]
    fn test_select_tab_by_number() {
        let (app, _file) = create_test_app();

        // Test keys 1-9
        for i in 1..=9 {
            let key = KeyEvent::new(
                KeyCode::Char(char::from_digit(i, 10).unwrap()),
                KeyModifiers::NONE,
            );
            let events = handle_input_event(key, &app);
            assert_eq!(events, vec![AppEvent::SelectTab((i - 1) as usize)]);
        }
    }

    #[test]
    fn test_filter_input_cursor_left() {
        let (mut app, _file) = create_test_app();
        app.start_filter_input();
        let key = KeyEvent::new(KeyCode::Left, KeyModifiers::NONE);
        let events = handle_input_event(key, &app);
        assert_eq!(events, vec![AppEvent::CursorLeft]);
    }

    #[test]
    fn test_filter_input_cursor_right() {
        let (mut app, _file) = create_test_app();
        app.start_filter_input();
        let key = KeyEvent::new(KeyCode::Right, KeyModifiers::NONE);
        let events = handle_input_event(key, &app);
        assert_eq!(events, vec![AppEvent::CursorRight]);
    }

    #[test]
    fn test_filter_input_cursor_home() {
        let (mut app, _file) = create_test_app();
        app.start_filter_input();
        let key = KeyEvent::new(KeyCode::Home, KeyModifiers::NONE);
        let events = handle_input_event(key, &app);
        assert_eq!(events, vec![AppEvent::CursorHome]);
    }

    #[test]
    fn test_filter_input_cursor_end() {
        let (mut app, _file) = create_test_app();
        app.start_filter_input();
        let key = KeyEvent::new(KeyCode::End, KeyModifiers::NONE);
        let events = handle_input_event(key, &app);
        assert_eq!(events, vec![AppEvent::CursorEnd]);
    }

    #[test]
    fn test_filter_input_ctrl_a_home() {
        let (mut app, _file) = create_test_app();
        app.start_filter_input();
        let key = KeyEvent::new(KeyCode::Char('a'), KeyModifiers::CONTROL);
        let events = handle_input_event(key, &app);
        assert_eq!(events, vec![AppEvent::CursorHome]);
    }

    #[test]
    fn test_filter_input_ctrl_e_end() {
        let (mut app, _file) = create_test_app();
        app.start_filter_input();
        let key = KeyEvent::new(KeyCode::Char('e'), KeyModifiers::CONTROL);
        let events = handle_input_event(key, &app);
        assert_eq!(events, vec![AppEvent::CursorEnd]);
    }

    #[test]
    fn test_line_jump_input_cursor_left() {
        let (mut app, _file) = create_test_app();
        app.start_line_jump_input();
        let key = KeyEvent::new(KeyCode::Left, KeyModifiers::NONE);
        let events = handle_input_event(key, &app);
        assert_eq!(events, vec![AppEvent::CursorLeft]);
    }

    #[test]
    fn test_line_jump_input_cursor_right() {
        let (mut app, _file) = create_test_app();
        app.start_line_jump_input();
        let key = KeyEvent::new(KeyCode::Right, KeyModifiers::NONE);
        let events = handle_input_event(key, &app);
        assert_eq!(events, vec![AppEvent::CursorRight]);
    }
}
