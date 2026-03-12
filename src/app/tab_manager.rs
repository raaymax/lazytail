use super::tab::TabState;
use super::{SourceType, ViewMode};
use crate::source::SourceStatus;

/// Manages the collection of tabs and combined views.
pub struct TabManager {
    /// All open tabs
    pub tabs: Vec<TabState>,

    /// Currently active tab index
    pub active: usize,

    /// Per-category combined ($all) tabs, indexed by SourceType as usize
    pub combined: [Option<TabState>; 5],

    /// Which category's combined tab is active (None = regular tab active)
    pub active_combined: Option<SourceType>,
}

impl TabManager {
    pub fn new(tabs: Vec<TabState>) -> Self {
        debug_assert!(!tabs.is_empty(), "TabManager must have at least one tab");
        Self {
            tabs,
            active: 0,
            combined: [None, None, None, None, None],
            active_combined: None,
        }
    }

    /// Get a reference to the active tab
    pub fn active_tab(&self) -> &TabState {
        if let Some(cat) = self.active_combined {
            self.combined[cat as usize]
                .as_ref()
                .expect("active_combined set but no combined tab for category")
        } else {
            debug_assert!(!self.tabs.is_empty(), "No tabs available");
            &self.tabs[self.active]
        }
    }

    /// Get a mutable reference to the active tab
    pub fn active_tab_mut(&mut self) -> &mut TabState {
        if let Some(cat) = self.active_combined {
            self.combined[cat as usize]
                .as_mut()
                .expect("active_combined set but no combined tab for category")
        } else {
            debug_assert!(!self.tabs.is_empty(), "No tabs available");
            &mut self.tabs[self.active]
        }
    }

    /// Switch to a specific tab by index
    pub fn select_tab(&mut self, index: usize) {
        if index < self.tabs.len() {
            self.active = index;
            self.active_combined = None;
        }
    }

    /// Map a sidebar shortcut number (0-based) to the real tab index.
    pub fn tab_index_for_shortcut(&self, shortcut: usize) -> Option<usize> {
        let categories = self.tabs_by_category();
        let mut count = 0;
        for (_, tab_indices) in &categories {
            for &tab_idx in tab_indices {
                if count == shortcut {
                    return Some(tab_idx);
                }
                count += 1;
            }
        }
        None
    }

    /// Get the number of tabs
    pub fn tab_count(&self) -> usize {
        self.tabs.len()
    }

    /// Add a new tab
    pub fn add_tab(&mut self, tab: TabState) {
        self.tabs.push(tab);
    }

    /// Close a tab by index. Returns true if app should quit (last tab closed).
    pub fn close_tab(&mut self, index: usize) -> bool {
        if self.tabs.len() <= 1 {
            return true;
        }

        if index < self.tabs.len() {
            let tab = &self.tabs[index];

            // If this is an ended discovered source, delete it
            if tab.source.source_status == Some(SourceStatus::Ended) {
                if let Some(ref path) = tab.source.source_path {
                    let _ = crate::source::delete_source(&tab.source.name, path);
                }
            }

            self.tabs.remove(index);

            if self.active >= self.tabs.len() {
                self.active = self.tabs.len() - 1;
            } else if self.active > index {
                self.active -= 1;
            }

            self.ensure_combined_tabs();

            if let Some(cat) = self.active_combined {
                if self.combined[cat as usize].is_none() {
                    self.active_combined = None;
                }
            }
        }

        false
    }

    /// Get tabs grouped by source type, returning (type, vec of global tab indices)
    pub fn tabs_by_category(&self) -> [(SourceType, Vec<usize>); 5] {
        let mut result = [
            (SourceType::ProjectSource, Vec::new()),
            (SourceType::GlobalSource, Vec::new()),
            (SourceType::Global, Vec::new()),
            (SourceType::File, Vec::new()),
            (SourceType::Pipe, Vec::new()),
        ];

        for (idx, tab) in self.tabs.iter().enumerate() {
            match tab.source_type() {
                SourceType::ProjectSource => result[0].1.push(idx),
                SourceType::GlobalSource => result[1].1.push(idx),
                SourceType::Global => result[2].1.push(idx),
                SourceType::File => result[3].1.push(idx),
                SourceType::Pipe => result[4].1.push(idx),
            }
        }

        result
    }

    /// Find global tab index from category and in-category index
    pub fn find_tab_index(&self, category: SourceType, idx: usize) -> Option<usize> {
        self.tabs
            .iter()
            .enumerate()
            .filter(|(_, t)| t.source_type() == category)
            .nth(idx)
            .map(|(i, _)| i)
    }

    /// Create or remove per-category combined ($all) tabs based on source counts.
    pub fn ensure_combined_tabs(&mut self) {
        use crate::reader::combined_reader::SourceEntry;

        let categories = self.tabs_by_category();

        for (cat, tab_indices) in &categories {
            let cat_idx = *cat as usize;

            let sources: Vec<SourceEntry> = tab_indices
                .iter()
                .map(|&idx| &self.tabs[idx])
                .filter(|t| !t.source.disabled)
                .map(|tab| SourceEntry {
                    name: tab.source.name.clone(),
                    reader: tab.source.reader.clone(),
                    index_reader: tab
                        .source
                        .source_path
                        .as_ref()
                        .and_then(|p| crate::index::reader::IndexReader::open(p)),
                    source_path: tab.source.source_path.clone(),
                    total_lines: tab.source.total_lines,
                    renderer_names: tab.source.renderer_names.clone(),
                })
                .collect();

            if sources.len() >= 2 {
                if self.combined[cat_idx].is_none() {
                    self.combined[cat_idx] = Some(TabState::from_combined(sources));
                }
            } else {
                self.combined[cat_idx] = None;
                if self.active_combined == Some(*cat) {
                    self.active_combined = None;
                }
            }
        }
    }

    /// Rebuild a specific category's combined tab reader from current sources.
    pub fn refresh_combined_tab(&mut self, cat: SourceType) {
        use crate::reader::combined_reader::{CombinedReader, SourceEntry};
        use crate::reader::LogReader;

        let cat_idx = cat as usize;
        let combined = match self.combined[cat_idx].as_mut() {
            Some(tab) => tab,
            None => return,
        };

        let sources: Vec<SourceEntry> = self
            .tabs
            .iter()
            .filter(|t| !t.source.disabled && t.source_type() == cat)
            .map(|tab| SourceEntry {
                name: tab.source.name.clone(),
                reader: tab.source.reader.clone(),
                index_reader: tab
                    .source
                    .source_path
                    .as_ref()
                    .and_then(|p| crate::index::reader::IndexReader::open(p)),
                source_path: tab.source.source_path.clone(),
                total_lines: tab.source.total_lines,
                renderer_names: tab.source.renderer_names.clone(),
            })
            .collect();

        let source_count = sources.len();
        let new_reader = CombinedReader::new(sources);
        let total_lines = new_reader.total_lines();

        combined.source.reader = std::sync::Arc::new(std::sync::Mutex::new(new_reader));
        combined.source.total_lines = total_lines;
        if combined.source.mode == ViewMode::Normal {
            combined.source.line_indices = (0..total_lines).collect();
        }
        combined.source.name = format!("$all ({} sources)", source_count);
    }

    /// Switch to a category's combined ($all) tab with a lazy refresh.
    pub fn select_combined_tab(&mut self, cat: SourceType) {
        let cat_idx = cat as usize;
        if self.combined[cat_idx].is_some() {
            self.refresh_combined_tab(cat);
            self.active_combined = Some(cat);
        }
    }
}
