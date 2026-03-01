use crate::app::tab::TabState;
use crate::theme::UiColors;
use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem},
    Frame,
};

pub(super) fn render_aggregation_view(
    f: &mut Frame,
    area: Rect,
    tab: &mut TabState,
    ui: &UiColors,
) {
    let result = match &tab.source.aggregation_result {
        Some(r) => r,
        None => {
            // No result yet — show empty block
            let block = Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(ui.highlight))
                .title("Aggregation: computing...");
            f.render_widget(block, area);
            return;
        }
    };

    let fields_label = result.aggregation.fields.join(", ");
    let title = format!(
        " Aggregation: count by ({}) | {} groups | {} total ",
        fields_label,
        result.groups.len(),
        result.total_matches
    );

    let inner_height = area.height.saturating_sub(2) as usize; // borders
    let inner_width = area.width.saturating_sub(2) as usize;
    let data_rows = inner_height.saturating_sub(1); // -1 for header
    tab.aggregation_view.visible_rows = data_rows;
    let scroll = tab.aggregation_view.scroll_offset;
    let selected = tab.aggregation_view.selected_row;

    // Find max count for bar scaling
    let max_count = result
        .groups
        .iter()
        .map(|g| g.count)
        .max()
        .unwrap_or(1)
        .max(1);

    // Build header
    let header_spans = build_header(&result.aggregation.fields, inner_width, ui);
    let mut items: Vec<ListItem> = vec![ListItem::new(Line::from(header_spans))];

    // Build data rows
    let visible_groups = result
        .groups
        .iter()
        .enumerate()
        .skip(scroll)
        .take(data_rows);

    for (idx, group) in visible_groups {
        let is_selected = idx == selected;
        let spans = build_row(group, max_count, inner_width, ui);
        let mut item = ListItem::new(Line::from(spans));
        if is_selected {
            item = item.style(
                Style::default()
                    .bg(ui.selection_bg)
                    .add_modifier(Modifier::BOLD),
            );
        }
        items.push(item);
    }

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(ui.highlight))
        .title(title);

    let list = List::new(items).block(block);
    f.render_widget(list, area);
}

fn build_header(fields: &[String], width: usize, ui: &UiColors) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    for field in fields {
        spans.push(Span::styled(
            format!(" {:<15}", field),
            Style::default().fg(ui.accent).add_modifier(Modifier::BOLD),
        ));
    }
    // Count column
    let remaining = width.saturating_sub(fields.len() * 16 + 8);
    spans.push(Span::styled(
        format!(" {:>7}", "Count"),
        Style::default().fg(ui.accent).add_modifier(Modifier::BOLD),
    ));
    // Bar header (empty space)
    if remaining > 0 {
        spans.push(Span::raw(" ".repeat(remaining)));
    }
    spans
}

fn build_row(
    group: &crate::filter::aggregation::AggregationGroup,
    max_count: usize,
    width: usize,
    ui: &UiColors,
) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    let field_cols = group.key.len() * 16;

    // Field values
    for (_name, value) in &group.key {
        let display = if value.len() > 14 {
            format!(" {:.14}", value)
        } else {
            format!(" {:<14}", value)
        };
        spans.push(Span::styled(display, Style::default().fg(ui.fg)));
        spans.push(Span::raw(" "));
    }

    // Count
    let count_str = format!("{:>7}", group.count);
    spans.push(Span::styled(count_str, Style::default().fg(ui.primary)));

    // Bar chart
    let bar_space = width.saturating_sub(field_cols + 8 + 1);
    if bar_space > 2 {
        let bar_max = bar_space.min(20);
        let filled = ((group.count as u64 * bar_max as u64) / max_count as u64) as usize;
        let filled = filled.max(1);
        let empty = bar_max.saturating_sub(filled);
        let bar_filled: String = "\u{2588}".repeat(filled);
        let bar_empty: String = "\u{2591}".repeat(empty);
        spans.push(Span::raw(" "));
        spans.push(Span::styled(bar_filled, Style::default().fg(ui.highlight)));
        spans.push(Span::styled(bar_empty, Style::default().fg(ui.muted)));
    }

    spans
}
