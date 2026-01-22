//! Viewport manages the relationship between selection and scroll position.
//!
//! Uses vim-like scrolling: selection moves freely within the visible area,
//! and the viewport only scrolls when selection hits the edge padding.
//! The anchor_line (file line number) is stable across filter changes.

/// Result of resolving the viewport against current content
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResolvedView {
    /// Index into line_indices for the selected line
    pub selected_index: usize,
    /// First visible line index (for rendering)
    pub scroll_position: usize,
}

/// Viewport manages selection and scrolling with vim-like behavior
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct Viewport {
    /// The file line number that is selected (stable across filter changes)
    anchor_line: usize,

    /// Current scroll position (index into line_indices)
    scroll_position: usize,

    /// Viewport height in lines
    height: usize,

    /// Padding to keep at edges (vim's scrolloff)
    edge_padding: usize,

    /// Cached resolved values (valid after resolve() call)
    cache: Option<ResolvedView>,
}

#[allow(dead_code)]
impl Viewport {
    /// Create a new viewport anchored to the given line
    pub fn new(initial_line: usize) -> Self {
        Self {
            anchor_line: initial_line,
            scroll_position: 0,
            height: 0,
            edge_padding: 3,
            cache: None,
        }
    }

    /// Resolve the viewport against current content
    ///
    /// Finds where the anchor line is in the current view and ensures
    /// scroll position keeps selection within the comfort zone.
    pub fn resolve(&mut self, line_indices: &[usize], height: usize) -> ResolvedView {
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
                // Line not in view, find nearest
                let idx = if insert_pos >= line_indices.len() {
                    line_indices.len() - 1
                } else if insert_pos == 0 {
                    0
                } else {
                    // Pick closer of insert_pos-1 or insert_pos
                    let before = line_indices[insert_pos - 1];
                    let after = line_indices[insert_pos];
                    if self.anchor_line - before <= after - self.anchor_line {
                        insert_pos - 1
                    } else {
                        insert_pos
                    }
                };
                // Update anchor to the line we actually found
                self.anchor_line = line_indices[idx];
                idx
            }
        };

        // Ensure selection is visible within comfort zone
        self.ensure_visible(selected_index, line_indices.len());

        let view = ResolvedView {
            selected_index,
            scroll_position: self.scroll_position,
        };
        self.cache = Some(view);
        view
    }

    /// Ensure selection index is visible within the comfort zone
    /// Only scrolls if selection is outside the padding boundaries
    fn ensure_visible(&mut self, selected_index: usize, total_lines: usize) {
        if self.height == 0 {
            return;
        }

        let padding = self.edge_padding.min(self.height / 4);
        let max_scroll = total_lines.saturating_sub(self.height);

        // If selection is above comfort zone (too close to top), scroll up
        if selected_index < self.scroll_position + padding {
            self.scroll_position = selected_index.saturating_sub(padding);
        }
        // If selection is below comfort zone (too close to bottom), scroll down
        else if selected_index + padding >= self.scroll_position + self.height {
            self.scroll_position = (selected_index + padding + 1).saturating_sub(self.height);
        }

        // Clamp scroll position
        self.scroll_position = self.scroll_position.min(max_scroll);
    }

    /// Move selection by delta lines (positive = down, negative = up)
    /// Selection moves freely; viewport only scrolls at edges
    pub fn move_selection(&mut self, delta: i32, line_indices: &[usize]) {
        if line_indices.is_empty() {
            return;
        }

        // Get current index
        let current_idx = self.find_index(line_indices);

        // Compute new index
        let new_idx = if delta >= 0 {
            (current_idx + delta as usize).min(line_indices.len() - 1)
        } else {
            current_idx.saturating_sub((-delta) as usize)
        };

        // Update anchor to new line
        self.anchor_line = line_indices[new_idx];

        // Ensure visible (vim-like: only scroll if hitting edge)
        self.ensure_visible(new_idx, line_indices.len());

        self.cache = None;
    }

    /// Move viewport by delta lines without moving selection
    /// (selection stays on same line, but moves on screen)
    pub fn move_viewport(&mut self, delta: i32, line_indices: &[usize]) {
        if line_indices.is_empty() || self.height == 0 {
            return;
        }

        let max_scroll = line_indices.len().saturating_sub(self.height);
        let current_idx = self.find_index(line_indices);

        if delta > 0 {
            // Scroll down
            self.scroll_position = (self.scroll_position + delta as usize).min(max_scroll);
            // If selection goes above viewport, move it down
            if current_idx < self.scroll_position {
                self.anchor_line = line_indices[self.scroll_position];
            }
        } else {
            // Scroll up
            self.scroll_position = self.scroll_position.saturating_sub((-delta) as usize);
            // If selection goes below viewport, move it up
            let bottom = self.scroll_position + self.height - 1;
            if current_idx > bottom {
                self.anchor_line = line_indices[bottom.min(line_indices.len() - 1)];
            }
        }

        self.cache = None;
    }

    /// Move both selection and viewport together (for mouse scroll)
    pub fn scroll_with_selection(&mut self, delta: i32, line_indices: &[usize]) {
        if line_indices.is_empty() {
            return;
        }

        let max_scroll = line_indices.len().saturating_sub(self.height.max(1));
        let current_idx = self.find_index(line_indices);

        // Move both scroll and selection by the same amount
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

        // Set new anchor
        self.anchor_line = line;

        // Find where new line is in indices
        let new_idx = self.find_index(line_indices);

        // Set scroll to maintain screen offset
        self.scroll_position = new_idx.saturating_sub(screen_offset);

        // Clamp scroll position
        let max_scroll = line_indices.len().saturating_sub(self.height.max(1));
        self.scroll_position = self.scroll_position.min(max_scroll);

        self.cache = None;
    }

    /// Jump to a specific index in the current view
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
            // Scroll to show last line at bottom
            self.scroll_position = line_indices.len().saturating_sub(self.height);
            self.cache = None;
        }
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
    /// Call this AFTER line_indices changes but with the new line_indices
    pub fn preserve_screen_offset(&mut self, new_line_indices: &[usize]) {
        if new_line_indices.is_empty() {
            return;
        }

        // Get current screen offset from cache (if available)
        let screen_offset = if let Some(cache) = self.cache {
            cache.selected_index.saturating_sub(cache.scroll_position)
        } else {
            // Fallback: use stored scroll_position
            let old_idx = self.find_index(new_line_indices);
            old_idx.saturating_sub(self.scroll_position)
        };

        // Find where anchor_line is in new content
        let new_idx = self.find_index(new_line_indices);

        // Set scroll to maintain same screen offset
        self.scroll_position = new_idx.saturating_sub(screen_offset);

        // Clamp scroll position
        let max_scroll = new_line_indices.len().saturating_sub(self.height.max(1));
        self.scroll_position = self.scroll_position.min(max_scroll);

        self.cache = None;
    }

    /// Get the currently selected file line number
    pub fn selected_line(&self) -> usize {
        self.anchor_line
    }

    /// Get the cached selected index (call resolve() first)
    pub fn selected_index(&self) -> usize {
        self.cache.map(|c| c.selected_index).unwrap_or(0)
    }

    /// Get the cached scroll position (call resolve() first)
    pub fn scroll_position(&self) -> usize {
        self.cache.map(|c| c.scroll_position).unwrap_or(0)
    }

    /// Get current height
    pub fn height(&self) -> usize {
        self.height
    }

    /// Set height (usually called during resolve, but can be set explicitly)
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
        // With selection at index 5 and height 5, scroll should put selection in comfort zone
    }

    #[test]
    fn test_resolve_line_not_found_finds_nearest() {
        let mut vp = Viewport::new(50);
        let lines = make_lines(&[10, 20, 30, 40, 60, 70]); // 50 not in list

        let view = vp.resolve(&lines, 5);

        // Should find nearest (40 or 60, 40 is closer to 50)
        assert_eq!(vp.selected_line(), 40);
        assert_eq!(view.selected_index, 3);
    }

    #[test]
    fn test_resolve_line_not_found_after_all() {
        let mut vp = Viewport::new(100);
        let lines = make_lines(&[10, 20, 30]);

        let view = vp.resolve(&lines, 5);

        // Should snap to last line
        assert_eq!(vp.selected_line(), 30);
        assert_eq!(view.selected_index, 2);
    }

    #[test]
    fn test_resolve_line_not_found_before_all() {
        let mut vp = Viewport::new(5);
        let lines = make_lines(&[10, 20, 30]);

        let view = vp.resolve(&lines, 5);

        // Should snap to first line
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
        // Selection should move within viewport without scrolling
        let mut vp = Viewport::new(0);
        let lines: Vec<usize> = (0..50).collect();
        vp.height = 20;
        vp.scroll_position = 0;

        // Move down within comfort zone - should not scroll
        vp.move_selection(5, &lines);
        assert_eq!(vp.selected_line(), 5);
        assert_eq!(vp.scroll_position, 0); // No scroll yet

        // Move down more but still in comfort zone
        vp.move_selection(5, &lines);
        assert_eq!(vp.selected_line(), 10);
        assert_eq!(vp.scroll_position, 0); // Still no scroll
    }

    #[test]
    fn test_vim_like_scrolling_scroll_at_bottom() {
        // Selection should trigger scroll when hitting bottom edge
        let mut vp = Viewport::new(0);
        let lines: Vec<usize> = (0..50).collect();
        vp.height = 20;
        vp.edge_padding = 3;
        vp.scroll_position = 0;

        // Move to near bottom (height - padding = 17)
        vp.move_selection(17, &lines);
        assert_eq!(vp.selected_line(), 17);
        // Should have scrolled to keep selection in comfort zone
        assert!(vp.scroll_position > 0);
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
        // Scroll should put last line at bottom
        assert_eq!(vp.scroll_position, 5); // 10 lines - 5 height = 5
    }

    #[test]
    fn test_center() {
        let mut vp = Viewport::new(25);
        let lines: Vec<usize> = (0..50).collect();
        vp.height = 10;
        vp.scroll_position = 0;

        vp.center(&lines);

        // Selection at 25 should be centered, so scroll = 25 - 5 = 20
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
        // Simulate: user is on line 500, filter shows subset including 500
        let mut vp = Viewport::new(500);
        vp.height = 20;

        // Unfiltered view
        let unfiltered: Vec<usize> = (0..1000).collect();
        let view1 = vp.resolve(&unfiltered, 20);
        assert_eq!(view1.selected_index, 500);

        // Filtered view that includes line 500
        let filtered = make_lines(&[100, 250, 400, 500, 600, 750, 900]);
        let view2 = vp.resolve(&filtered, 20);

        // Line 500 should still be selected
        assert_eq!(vp.selected_line(), 500);
        assert_eq!(view2.selected_index, 3); // Index in filtered list
    }

    #[test]
    fn test_filter_snaps_to_nearest_when_line_removed() {
        let mut vp = Viewport::new(500);
        vp.height = 20;

        // Filtered view that does NOT include line 500
        let filtered = make_lines(&[100, 250, 400, 600, 750, 900]);
        let view = vp.resolve(&filtered, 20);

        // Should snap to nearest (400 or 600)
        assert!(vp.selected_line() == 400 || vp.selected_line() == 600);
        assert!(view.selected_index <= filtered.len());
    }

    #[test]
    fn test_move_viewport_down() {
        let mut vp = Viewport::new(50);
        vp.height = 20;
        vp.scroll_position = 45;
        let lines: Vec<usize> = (0..100).collect();

        vp.move_viewport(2, &lines); // Scroll down 2

        assert_eq!(vp.scroll_position, 47);
        // Selection stays on line 50 (still visible)
        assert_eq!(vp.selected_line(), 50);
    }

    #[test]
    fn test_move_viewport_up() {
        let mut vp = Viewport::new(50);
        vp.height = 20;
        vp.scroll_position = 45;
        let lines: Vec<usize> = (0..100).collect();

        vp.move_viewport(-2, &lines); // Scroll up 2

        assert_eq!(vp.scroll_position, 43);
        assert_eq!(vp.selected_line(), 50);
    }
}
