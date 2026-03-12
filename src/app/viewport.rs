//! Viewport manages the relationship between selection and scroll position.
//!
//! Uses vim-like scrolling: selection moves freely within the visible area,
//! and the viewport only scrolls when selection hits the edge.
//! The anchor_line (file line number) is stable across filter changes.
//!
//! All scrolling math works in visual rows via a caller-provided height
//! function. For non-wrap mode the caller passes `|_| 1`; for wrap mode
//! it returns the actual wrapped height of each line. This keeps viewport
//! agnostic of content while handling both modes in a single code path.

/// Default edge padding (vim's scrolloff equivalent)
const DEFAULT_EDGE_PADDING: usize = 0;

/// Result of resolving the viewport against current content
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResolvedView {
    /// Index into line_indices for the selected line
    pub selected_index: usize,
    /// First visible line index (for rendering)
    pub scroll_position: usize,
}

/// Viewport manages selection and scrolling with vim-like behavior
#[derive(Debug, Clone)]
pub struct Viewport {
    /// The file line number that is selected (stable across filter changes)
    anchor_line: usize,

    /// Current scroll position (index into line_indices)
    scroll_position: usize,

    /// Viewport height in visual rows
    height: usize,

    /// Padding to keep at edges (vim's scrolloff)
    edge_padding: usize,

    /// Cached resolved values (valid after resolve() call)
    cache: Option<ResolvedView>,
}

impl Viewport {
    /// Create a new viewport anchored to the given line
    pub fn new(initial_line: usize) -> Self {
        Self {
            anchor_line: initial_line,
            scroll_position: 0,
            height: 0,
            edge_padding: DEFAULT_EDGE_PADDING,
            cache: None,
        }
    }

    /// Resolve the viewport against current content.
    /// Each line has visual height 1 (no wrapping).
    #[allow(dead_code)]
    pub fn resolve(&mut self, line_indices: &[usize], height: usize) -> ResolvedView {
        self.resolve_with_heights(line_indices, height, &mut |_| 1)
    }

    /// Resolve viewport with a caller-provided height function.
    ///
    /// `line_height(index)` returns the visual row count for the line at
    /// the given index into `line_indices`. For non-wrap mode pass `|_| 1`.
    ///
    /// Finds where the anchor line is in the current view and ensures
    /// scroll position keeps selection visible.
    pub fn resolve_with_heights(
        &mut self,
        line_indices: &[usize],
        height: usize,
        line_height: &mut dyn FnMut(usize) -> usize,
    ) -> ResolvedView {
        self.height = height;

        if line_indices.is_empty() {
            let view = ResolvedView {
                selected_index: 0,
                scroll_position: 0,
            };
            self.cache = Some(view);
            return view;
        }

        // Find anchor line in current view
        let selected_index = match line_indices.binary_search(&self.anchor_line) {
            Ok(idx) => idx,
            Err(insert_pos) => {
                let idx = if insert_pos >= line_indices.len() {
                    line_indices.len() - 1
                } else if insert_pos == 0 {
                    0
                } else {
                    let before = line_indices[insert_pos - 1];
                    let after = line_indices[insert_pos];
                    if self.anchor_line - before <= after - self.anchor_line {
                        insert_pos - 1
                    } else {
                        insert_pos
                    }
                };
                self.anchor_line = line_indices[idx];
                idx
            }
        };

        // Ensure selection is visible (works in visual rows)
        self.ensure_visible(selected_index, line_indices.len(), &mut *line_height);

        let view = ResolvedView {
            selected_index,
            scroll_position: self.scroll_position,
        };
        self.cache = Some(view);
        view
    }

    /// Ensure selection index is visible within the comfort zone.
    /// Counts visual rows using `line_height` so it works correctly
    /// for both wrapped and non-wrapped lines.
    fn ensure_visible(
        &mut self,
        selected_index: usize,
        total_lines: usize,
        line_height: &mut dyn FnMut(usize) -> usize,
    ) {
        if self.height == 0 || total_lines == 0 {
            return;
        }

        let padding = self.edge_padding.min(self.height / 4);

        // Selection is above the viewport — scroll up
        if selected_index < self.scroll_position {
            self.scroll_position = selected_index;
            let mut pad_rows = 0;
            while self.scroll_position > 0 && pad_rows < padding {
                self.scroll_position -= 1;
                pad_rows += line_height(self.scroll_position);
            }
            return;
        }

        // Fast path: if selection is far below scroll_position, jump directly
        // near the selection instead of counting millions of rows.
        let gap = selected_index - self.scroll_position;
        if gap > self.height * 2 {
            // Jump scroll_position close to selection, then fine-tune below
            self.scroll_position = selected_index.saturating_sub(self.height.saturating_sub(1));
        }

        // Count visual rows from scroll_position to selected_index
        let mut rows_above: usize = 0;
        let end = selected_index.min(total_lines);
        for i in self.scroll_position..end {
            rows_above += line_height(i);
        }
        let selected_h = line_height(selected_index.min(total_lines - 1));
        let total_rows_through_selected = rows_above + selected_h;

        // Selection is below the viewport — scroll down
        if total_rows_through_selected > self.height {
            // Walk scroll_position forward until selected line fits on screen
            let mut rows = total_rows_through_selected;
            while rows > self.height && self.scroll_position < selected_index {
                rows -= line_height(self.scroll_position);
                self.scroll_position += 1;
            }
            // Apply bottom padding
            let mut pad_rows = 0;
            let mut pad_idx = selected_index + 1;
            while pad_idx < total_lines && pad_rows < padding {
                pad_rows += line_height(pad_idx);
                pad_idx += 1;
            }
            if pad_rows > 0 {
                let mut rows: usize = 0;
                for i in self.scroll_position..pad_idx.min(total_lines) {
                    rows += line_height(i);
                }
                while rows > self.height && self.scroll_position < selected_index {
                    rows -= line_height(self.scroll_position);
                    self.scroll_position += 1;
                }
            }
        }
        // Selection is within the visible area — check edge padding
        else if padding > 0 {
            // Top padding
            let mut top_visual = 0;
            for i in self.scroll_position..selected_index {
                top_visual += line_height(i);
                if top_visual >= padding {
                    break;
                }
            }
            if top_visual < padding && self.scroll_position > 0 {
                let mut needed = padding - top_visual;
                while needed > 0 && self.scroll_position > 0 {
                    self.scroll_position -= 1;
                    needed = needed.saturating_sub(line_height(self.scroll_position));
                }
            }

            // Bottom padding
            let rows_after = self.height.saturating_sub(total_rows_through_selected);
            let mut pad_visual = 0;
            let mut i = selected_index + 1;
            let mut pad_count = 0;
            while pad_count < padding.max(1) && i < total_lines {
                pad_visual += line_height(i);
                pad_count += 1;
                i += 1;
            }
            if rows_after < pad_visual.min(padding) {
                let mut excess = pad_visual.min(padding) - rows_after;
                while excess > 0 && self.scroll_position < selected_index {
                    let h = line_height(self.scroll_position);
                    excess = excess.saturating_sub(h);
                    self.scroll_position += 1;
                }
            }
        }

        // Clamp scroll_position
        if self.scroll_position >= total_lines {
            self.scroll_position = total_lines.saturating_sub(1);
        }
    }

    /// Move selection by delta lines (positive = down, negative = up)
    /// Only updates anchor; scroll adjustment is deferred to resolve.
    pub fn move_selection(&mut self, delta: i32, line_indices: &[usize]) {
        if line_indices.is_empty() {
            return;
        }

        let current_idx = self.find_index(line_indices);

        let new_idx = if delta >= 0 {
            (current_idx + delta as usize).min(line_indices.len() - 1)
        } else {
            current_idx.saturating_sub((-delta) as usize)
        };

        self.anchor_line = line_indices[new_idx];
        self.cache = None;
    }

    /// Move viewport by delta lines, keeping selection at same screen position
    /// Used by Ctrl+E (down) and Ctrl+Y (up) vim commands
    ///
    /// Both viewport and selection move together, so selection stays at
    /// the same position on screen.
    pub fn move_viewport(&mut self, delta: i32, line_indices: &[usize]) {
        if line_indices.is_empty() || self.height == 0 {
            return;
        }

        let max_scroll = line_indices.len().saturating_sub(1);
        let current_idx = self.find_index(line_indices);

        if delta > 0 {
            let delta_usize = delta as usize;
            let new_scroll = (self.scroll_position + delta_usize).min(max_scroll);
            let scroll_delta = new_scroll - self.scroll_position;
            self.scroll_position = new_scroll;

            let new_idx = (current_idx + scroll_delta).min(line_indices.len() - 1);
            self.anchor_line = line_indices[new_idx];

            if scroll_delta == 0 && current_idx < line_indices.len() - 1 {
                let new_idx = (current_idx + delta_usize).min(line_indices.len() - 1);
                self.anchor_line = line_indices[new_idx];
            }
        } else {
            let delta_usize = (-delta) as usize;
            let new_scroll = self.scroll_position.saturating_sub(delta_usize);
            let scroll_delta = self.scroll_position - new_scroll;
            self.scroll_position = new_scroll;

            let new_idx = current_idx.saturating_sub(scroll_delta);
            self.anchor_line = line_indices[new_idx];

            if scroll_delta == 0 && current_idx > 0 {
                let new_idx = current_idx.saturating_sub(delta_usize);
                self.anchor_line = line_indices[new_idx];
            }
        }

        self.cache = None;
    }

    /// Move both selection and viewport together (for mouse scroll)
    pub fn scroll_with_selection(&mut self, delta: i32, line_indices: &[usize]) {
        if line_indices.is_empty() {
            return;
        }

        let max_scroll = line_indices.len().saturating_sub(1);
        let current_idx = self.find_index(line_indices);

        if delta >= 0 {
            let actual_delta = delta as usize;
            self.scroll_position = (self.scroll_position + actual_delta).min(max_scroll);
            let new_idx = (current_idx + actual_delta).min(line_indices.len() - 1);
            self.anchor_line = line_indices[new_idx];
        } else {
            let actual_delta = (-delta) as usize;
            self.scroll_position = self.scroll_position.saturating_sub(actual_delta);
            let new_idx = current_idx.saturating_sub(actual_delta);
            self.anchor_line = line_indices[new_idx];
        }

        self.cache = None;
    }

    /// Jump to a specific file line number
    pub fn jump_to_line(&mut self, line: usize) {
        self.anchor_line = line;
        self.cache = None;
    }

    /// Get current screen offset (rows from top of viewport to selection)
    pub fn get_screen_offset(&self, line_indices: &[usize]) -> usize {
        let idx = self.find_index(line_indices);
        idx.saturating_sub(self.scroll_position)
    }

    /// Jump to a specific file line number at a given screen offset
    /// Used to maintain visual position when content changes (e.g., filtering)
    pub fn jump_to_line_at_offset(
        &mut self,
        line: usize,
        screen_offset: usize,
        line_indices: &[usize],
    ) {
        if line_indices.is_empty() {
            return;
        }

        self.anchor_line = line;
        let new_idx = self.find_index(line_indices);

        self.scroll_position = new_idx.saturating_sub(screen_offset);

        let max_scroll = line_indices.len().saturating_sub(self.height.max(1));
        self.scroll_position = self.scroll_position.min(max_scroll);

        self.cache = None;
    }

    /// Jump to a specific index in the current view
    #[allow(dead_code)]
    pub fn jump_to_index(&mut self, index: usize, line_indices: &[usize]) {
        if line_indices.is_empty() {
            return;
        }
        let index = index.min(line_indices.len() - 1);
        self.anchor_line = line_indices[index];
        self.cache = None;
    }

    /// Jump to start (first line)
    pub fn jump_to_start(&mut self, line_indices: &[usize]) {
        if !line_indices.is_empty() {
            self.anchor_line = line_indices[0];
            self.scroll_position = 0;
            self.cache = None;
        }
    }

    /// Jump to end (last line)
    pub fn jump_to_end(&mut self, line_indices: &[usize]) {
        if !line_indices.is_empty() {
            self.anchor_line = line_indices[line_indices.len() - 1];
            // Approximate: resolve will fix scroll_position precisely
            self.scroll_position = line_indices.len().saturating_sub(self.height);
            self.cache = None;
        }
    }

    /// Adjust scroll position when items are prepended to line_indices.
    pub fn adjust_scroll_for_prepend(&mut self, prepended_count: usize) {
        self.scroll_position = self.scroll_position.saturating_add(prepended_count);
        self.cache = None;
    }

    /// Center the current selection on screen
    pub fn center(&mut self, line_indices: &[usize]) {
        if line_indices.is_empty() || self.height == 0 {
            return;
        }
        let current_idx = self.find_index(line_indices);
        let half_height = self.height / 2;
        self.scroll_position = current_idx.saturating_sub(half_height);
        let max_scroll = line_indices.len().saturating_sub(self.height);
        self.scroll_position = self.scroll_position.min(max_scroll);
        self.cache = None;
    }

    /// Move selection to top of viewport (with padding)
    pub fn anchor_to_top(&mut self, line_indices: &[usize]) {
        if line_indices.is_empty() {
            return;
        }
        let padding = self.edge_padding.min(self.height / 4);
        let target_idx = self.scroll_position + padding;
        let target_idx = target_idx.min(line_indices.len() - 1);
        self.anchor_line = line_indices[target_idx];
        self.cache = None;
    }

    /// Move selection to bottom of viewport (with padding)
    pub fn anchor_to_bottom(&mut self, line_indices: &[usize]) {
        if line_indices.is_empty() || self.height == 0 {
            return;
        }
        let padding = self.edge_padding.min(self.height / 4);
        let target_idx = (self.scroll_position + self.height).saturating_sub(1 + padding);
        let target_idx = target_idx.min(line_indices.len() - 1);
        self.anchor_line = line_indices[target_idx];
        self.cache = None;
    }

    /// Preserve screen offset when content changes (e.g., filter cleared)
    pub fn preserve_screen_offset(&mut self, new_line_indices: &[usize]) {
        if new_line_indices.is_empty() {
            return;
        }

        let screen_offset = if let Some(cache) = self.cache {
            cache.selected_index.saturating_sub(cache.scroll_position)
        } else {
            let old_idx = self.find_index(new_line_indices);
            old_idx.saturating_sub(self.scroll_position)
        };

        let new_idx = self.find_index(new_line_indices);
        self.scroll_position = new_idx.saturating_sub(screen_offset);

        let max_scroll = new_line_indices.len().saturating_sub(self.height.max(1));
        self.scroll_position = self.scroll_position.min(max_scroll);

        self.cache = None;
    }

    /// Get the currently selected file line number
    pub fn selected_line(&self) -> usize {
        self.anchor_line
    }

    /// Get the cached selected index (call resolve() first)
    #[allow(dead_code)]
    pub fn selected_index(&self) -> usize {
        self.cache.map(|c| c.selected_index).unwrap_or(0)
    }

    /// Get the cached scroll position (call resolve() first)
    #[allow(dead_code)]
    pub fn scroll_position(&self) -> usize {
        self.cache.map(|c| c.scroll_position).unwrap_or(0)
    }

    /// Get current height
    #[allow(dead_code)]
    pub fn height(&self) -> usize {
        self.height
    }

    /// Set height (usually called during resolve, but can be set explicitly)
    #[allow(dead_code)]
    pub fn set_height(&mut self, height: usize) {
        if self.height != height {
            self.height = height;
            self.cache = None;
        }
    }

    // --- Private helpers ---

    /// Find current anchor_line index in line_indices
    fn find_index(&self, line_indices: &[usize]) -> usize {
        match line_indices.binary_search(&self.anchor_line) {
            Ok(idx) => idx,
            Err(insert_pos) => insert_pos.min(line_indices.len().saturating_sub(1)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_lines(lines: &[usize]) -> Vec<usize> {
        lines.to_vec()
    }

    #[test]
    fn test_new_viewport() {
        let vp = Viewport::new(100);
        assert_eq!(vp.selected_line(), 100);
    }

    #[test]
    fn test_resolve_basic() {
        let mut vp = Viewport::new(5);
        let lines = make_lines(&[0, 1, 2, 3, 4, 5, 6, 7, 8, 9]);

        let view = vp.resolve(&lines, 5);

        assert_eq!(view.selected_index, 5);
    }

    #[test]
    fn test_resolve_line_not_found_finds_nearest() {
        let mut vp = Viewport::new(50);
        let lines = make_lines(&[10, 20, 30, 40, 60, 70]);

        let view = vp.resolve(&lines, 5);

        assert_eq!(vp.selected_line(), 40);
        assert_eq!(view.selected_index, 3);
    }

    #[test]
    fn test_resolve_line_not_found_after_all() {
        let mut vp = Viewport::new(100);
        let lines = make_lines(&[10, 20, 30]);

        let view = vp.resolve(&lines, 5);

        assert_eq!(vp.selected_line(), 30);
        assert_eq!(view.selected_index, 2);
    }

    #[test]
    fn test_resolve_line_not_found_before_all() {
        let mut vp = Viewport::new(5);
        let lines = make_lines(&[10, 20, 30]);

        let view = vp.resolve(&lines, 5);

        assert_eq!(vp.selected_line(), 10);
        assert_eq!(view.selected_index, 0);
    }

    #[test]
    fn test_resolve_empty_lines() {
        let mut vp = Viewport::new(5);
        let lines: Vec<usize> = vec![];

        let view = vp.resolve(&lines, 5);

        assert_eq!(view.selected_index, 0);
        assert_eq!(view.scroll_position, 0);
    }

    #[test]
    fn test_move_selection_down() {
        let mut vp = Viewport::new(5);
        let lines = make_lines(&[0, 1, 2, 3, 4, 5, 6, 7, 8, 9]);
        vp.height = 5;

        vp.move_selection(1, &lines);

        assert_eq!(vp.selected_line(), 6);
    }

    #[test]
    fn test_move_selection_up() {
        let mut vp = Viewport::new(5);
        let lines = make_lines(&[0, 1, 2, 3, 4, 5, 6, 7, 8, 9]);
        vp.height = 5;

        vp.move_selection(-1, &lines);

        assert_eq!(vp.selected_line(), 4);
    }

    #[test]
    fn test_move_selection_clamps_at_start() {
        let mut vp = Viewport::new(1);
        let lines = make_lines(&[0, 1, 2, 3, 4]);
        vp.height = 5;

        vp.move_selection(-10, &lines);

        assert_eq!(vp.selected_line(), 0);
    }

    #[test]
    fn test_move_selection_clamps_at_end() {
        let mut vp = Viewport::new(8);
        let lines = make_lines(&[0, 1, 2, 3, 4, 5, 6, 7, 8, 9]);
        vp.height = 5;

        vp.move_selection(10, &lines);

        assert_eq!(vp.selected_line(), 9);
    }

    #[test]
    fn test_vim_like_scrolling_no_scroll_in_middle() {
        let mut vp = Viewport::new(0);
        let lines: Vec<usize> = (0..50).collect();
        vp.height = 20;
        vp.scroll_position = 0;

        vp.move_selection(5, &lines);
        assert_eq!(vp.selected_line(), 5);

        vp.move_selection(5, &lines);
        assert_eq!(vp.selected_line(), 10);
    }

    #[test]
    fn test_resolve_scrolls_when_selection_past_bottom() {
        let mut vp = Viewport::new(0);
        let lines: Vec<usize> = (0..50).collect();

        // Selection at 25, viewport height 10: should scroll
        vp.anchor_line = 25;
        vp.scroll_position = 0;
        let view = vp.resolve(&lines, 10);

        assert_eq!(view.selected_index, 25);
        // scroll_position should have advanced so selection is visible
        assert!(view.scroll_position > 0);
        assert!(view.scroll_position <= 25);
    }

    #[test]
    fn test_resolve_scrolls_when_selection_above_top() {
        let mut vp = Viewport::new(0);
        let lines: Vec<usize> = (0..50).collect();

        // scroll_position at 20, selection at 5: should scroll up
        vp.anchor_line = 5;
        vp.scroll_position = 20;
        let view = vp.resolve(&lines, 10);

        assert_eq!(view.selected_index, 5);
        assert_eq!(view.scroll_position, 5);
    }

    #[test]
    fn test_resolve_with_wrapped_heights() {
        let mut vp = Viewport::new(0);
        let lines: Vec<usize> = (0..20).collect();

        // Each line is 3 visual rows. Height=12 means only 4 lines fit.
        vp.anchor_line = 6;
        vp.scroll_position = 0;
        let view = vp.resolve_with_heights(&lines, 12, &mut |_| 3);

        assert_eq!(view.selected_index, 6);
        // Lines 0-6 = 7*3 = 21 rows > 12, so must scroll.
        // Need scroll_position where lines scroll..6 fit in 12 rows.
        // 4 lines * 3 = 12. So scroll_position = 6-3 = 3.
        assert_eq!(view.scroll_position, 3);
    }

    #[test]
    fn test_resolve_wrapped_selection_at_top() {
        let mut vp = Viewport::new(0);
        let lines: Vec<usize> = (0..20).collect();

        // Each line 2 rows, height 10. Selection at 2, scroll at 5.
        // Selection is above viewport — should scroll up.
        vp.anchor_line = 2;
        vp.scroll_position = 5;
        let view = vp.resolve_with_heights(&lines, 10, &mut |_| 2);

        assert_eq!(view.selected_index, 2);
        assert_eq!(view.scroll_position, 2);
    }

    #[test]
    fn test_jump_to_start() {
        let mut vp = Viewport::new(50);
        let lines = make_lines(&[0, 10, 20, 30, 40, 50, 60, 70, 80, 90]);
        vp.height = 5;

        vp.jump_to_start(&lines);

        assert_eq!(vp.selected_line(), 0);
        assert_eq!(vp.scroll_position, 0);
    }

    #[test]
    fn test_jump_to_end() {
        let mut vp = Viewport::new(0);
        let lines = make_lines(&[0, 10, 20, 30, 40, 50, 60, 70, 80, 90]);
        vp.height = 5;

        vp.jump_to_end(&lines);

        assert_eq!(vp.selected_line(), 90);
        assert_eq!(vp.scroll_position, 5); // 10 lines - 5 height = 5
    }

    #[test]
    fn test_center() {
        let mut vp = Viewport::new(25);
        let lines: Vec<usize> = (0..50).collect();
        vp.height = 10;
        vp.scroll_position = 0;

        vp.center(&lines);

        assert_eq!(vp.scroll_position, 20);
    }

    #[test]
    fn test_scroll_with_selection() {
        let mut vp = Viewport::new(5);
        let lines = make_lines(&[0, 1, 2, 3, 4, 5, 6, 7, 8, 9]);
        vp.height = 5;
        vp.scroll_position = 0;

        vp.scroll_with_selection(3, &lines);

        assert_eq!(vp.selected_line(), 8);
        assert_eq!(vp.scroll_position, 3);
    }

    #[test]
    fn test_filter_preserves_position() {
        let mut vp = Viewport::new(500);
        vp.height = 20;

        let unfiltered: Vec<usize> = (0..1000).collect();
        let view1 = vp.resolve(&unfiltered, 20);
        assert_eq!(view1.selected_index, 500);

        let filtered = make_lines(&[100, 250, 400, 500, 600, 750, 900]);
        let view2 = vp.resolve(&filtered, 20);

        assert_eq!(vp.selected_line(), 500);
        assert_eq!(view2.selected_index, 3);
    }

    #[test]
    fn test_filter_snaps_to_nearest_when_line_removed() {
        let mut vp = Viewport::new(500);
        vp.height = 20;

        let filtered = make_lines(&[100, 250, 400, 600, 750, 900]);
        let view = vp.resolve(&filtered, 20);

        assert!(vp.selected_line() == 400 || vp.selected_line() == 600);
        assert!(view.selected_index <= filtered.len());
    }

    #[test]
    fn test_move_viewport_down() {
        let mut vp = Viewport::new(50);
        vp.height = 20;
        vp.scroll_position = 45;
        let lines: Vec<usize> = (0..100).collect();

        vp.move_viewport(2, &lines);

        assert_eq!(vp.scroll_position, 47);
        assert_eq!(vp.selected_line(), 52);
    }

    #[test]
    fn test_move_viewport_up() {
        let mut vp = Viewport::new(50);
        vp.height = 20;
        vp.scroll_position = 45;
        let lines: Vec<usize> = (0..100).collect();

        vp.move_viewport(-2, &lines);

        assert_eq!(vp.scroll_position, 43);
        assert_eq!(vp.selected_line(), 48);
    }

    #[test]
    fn test_move_viewport_down_selection_moves_with_scroll() {
        let mut vp = Viewport::new(10);
        vp.height = 20;
        vp.scroll_position = 10;
        let lines: Vec<usize> = (0..100).collect();

        assert_eq!(vp.selected_line(), 10);

        vp.move_viewport(1, &lines);

        assert_eq!(vp.scroll_position, 11);
        assert_eq!(vp.selected_line(), 11);
    }

    #[test]
    fn test_move_viewport_up_selection_moves_with_scroll() {
        let mut vp = Viewport::new(29);
        vp.height = 20;
        vp.scroll_position = 10;
        let lines: Vec<usize> = (0..100).collect();

        assert_eq!(vp.selected_line(), 29);

        vp.move_viewport(-1, &lines);

        assert_eq!(vp.scroll_position, 9);
        assert_eq!(vp.selected_line(), 28);
    }

    #[test]
    fn test_move_viewport_down_at_max_scroll_moves_selection() {
        let mut vp = Viewport::new(90);
        vp.height = 20;
        vp.scroll_position = 80;
        let lines: Vec<usize> = (0..100).collect();

        assert_eq!(vp.selected_line(), 90);

        vp.move_viewport(1, &lines);

        // Can't scroll past last line
        assert!(vp.scroll_position >= 80);
        // Selection should still move
        assert_eq!(vp.selected_line(), 91);
    }

    #[test]
    fn test_move_viewport_up_at_start_moves_selection() {
        let mut vp = Viewport::new(10);
        vp.height = 20;
        vp.scroll_position = 0;
        let lines: Vec<usize> = (0..100).collect();

        assert_eq!(vp.selected_line(), 10);

        vp.move_viewport(-1, &lines);

        assert_eq!(vp.scroll_position, 0);
        assert_eq!(vp.selected_line(), 9);
    }

    #[test]
    fn test_resolve_large_file_scroll_position_zero() {
        // Simulates opening a large file where anchor is at the end
        // but scroll_position starts at 0. Must not iterate millions of lines.
        let total = 50_000_000;
        let mut vp = Viewport::new(total - 1);
        vp.scroll_position = 0;
        let lines: Vec<usize> = (0..total).collect();

        let view = vp.resolve(&lines, 50);

        assert_eq!(view.selected_index, total - 1);
        // scroll_position should be near the end
        assert!(view.scroll_position >= total - 50);
    }
}
