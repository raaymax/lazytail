/// Events that can occur in the application
/// Handlers return these events instead of mutating app state directly
#[derive(Debug, Clone, PartialEq)]
pub enum AppEvent {
    // Navigation events
    ScrollDown,
    ScrollUp,
    PageDown(usize), // page size
    PageUp(usize),
    JumpToStart,
    JumpToEnd,
    MouseScrollDown(usize), // scroll amount (lines)
    MouseScrollUp(usize),   // scroll amount (lines)
    ViewportDown,           // Ctrl+E - scroll viewport down, keep selection
    ViewportUp,             // Ctrl+Y - scroll viewport up, keep selection

    // Filter events
    StartFilterInput,
    FilterInputChar(char),
    FilterInputBackspace,
    FilterInputSubmit,
    FilterInputCancel,
    ClearFilter,
    ToggleFilterMode,      // Tab in filter input - switch Plain/Regex
    ToggleCaseSensitivity, // Alt+C in filter input
    CursorLeft,            // Move cursor left in input
    CursorRight,           // Move cursor right in input
    CursorHome,            // Move cursor to start of input
    CursorEnd,             // Move cursor to end of input
    StartFilter {
        pattern: String,
        incremental: bool,
        range: Option<(usize, usize)>,
    },
    FilterProgress(usize),
    /// Partial filter results (for immediate display while filtering continues)
    FilterPartialResults(Vec<usize>),
    FilterComplete {
        indices: Vec<usize>,
        incremental: bool,
    },
    FilterError(String),

    // File events
    FileModified {
        new_total: usize,
        old_total: usize,
    },
    FileTruncated {
        new_total: usize,
    },

    // Stream events (for background loading of pipes/stdin)
    StreamData {
        lines: Vec<String>,
    },
    StreamComplete,

    // Tab navigation events
    NextTab,
    PrevTab,
    SelectTab(usize),

    // Mode toggles
    ToggleFollowMode,
    DisableFollowMode,

    // Help mode
    ShowHelp,
    HideHelp,

    // Line jump events
    StartLineJumpInput,
    LineJumpInputChar(char),
    LineJumpInputBackspace,
    LineJumpInputSubmit,
    LineJumpInputCancel,

    // Filter history navigation
    HistoryUp,
    HistoryDown,

    // View positioning (vim z commands)
    CenterView,   // zz
    ViewToTop,    // zt
    ViewToBottom, // zb
    EnterZMode,   // z pressed, waiting for second key
    ExitZMode,    // cancel z mode

    // Line expansion events
    ToggleLineExpansion, // Toggle expansion of currently selected line
    CollapseAll,         // Collapse all expanded lines

    // System events
    Quit,
}
