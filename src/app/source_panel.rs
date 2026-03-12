use super::{SourceType, TreeSelection};

/// State for the source panel tree navigation
#[derive(Debug)]
pub struct SourcePanelState {
    /// Currently selected tree item
    pub selection: Option<TreeSelection>,
    /// Whether each category is expanded: [ProjectSource, GlobalSource, Global, Files, Pipes]
    pub expanded: [bool; 5],
}

impl Default for SourcePanelState {
    fn default() -> Self {
        Self {
            selection: None,
            expanded: [true, true, true, true, true],
        }
    }
}

/// Manages the source panel tree navigation and width.
#[derive(Debug)]
pub struct SourcePanelController {
    /// Source panel tree state
    pub state: SourcePanelState,

    /// Side panel width
    pub width: u16,
}

impl SourcePanelController {
    pub fn new() -> Self {
        Self {
            state: SourcePanelState::default(),
            width: 32,
        }
    }

    /// Navigate tree selection up or down
    pub fn navigate(&mut self, delta: i32, items: &[TreeSelection]) {
        if items.is_empty() {
            return;
        }

        let current_pos = self
            .state
            .selection
            .as_ref()
            .and_then(|sel| items.iter().position(|x| x == sel))
            .unwrap_or(0);

        let new_pos = (current_pos as i32 + delta)
            .max(0)
            .min(items.len() as i32 - 1) as usize;

        self.state.selection = Some(items[new_pos].clone());
    }

    /// Toggle expand/collapse on the selected category
    pub fn toggle_category_expand(&mut self) {
        if let Some(TreeSelection::Category(cat)) = self.state.selection {
            let idx = cat as usize;
            self.state.expanded[idx] = !self.state.expanded[idx];
        }
    }

    /// Fix source panel selection after a tab is closed
    pub fn fix_selection_after_close(&mut self, cat_count_fn: impl Fn(SourceType) -> usize) {
        if let Some(TreeSelection::Item(cat, idx)) = self.state.selection {
            let cat_count = cat_count_fn(cat);
            if cat_count == 0 {
                self.state.selection = None;
            } else if idx >= cat_count {
                self.state.selection = Some(TreeSelection::Item(cat, cat_count - 1));
            }
        }
    }
}
