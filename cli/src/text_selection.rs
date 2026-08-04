use ratatui::buffer::Buffer;
use ratatui::layout::{Position, Rect};
use ratatui::style::Style;

use crate::ui::theme::{BG_TEXT_SELECTION, TEXT_BRIGHT};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TextSelection {
    pub anchor: Position,
    pub head: Position,
    pub bounds: Rect,
}

impl TextSelection {
    pub fn new(anchor: Position, head: Position, bounds: Rect) -> Self {
        Self {
            anchor: clamp_position(anchor, bounds),
            head: clamp_position(head, bounds),
            bounds,
        }
    }

    fn ordered(self) -> (Position, Position) {
        if (self.anchor.y, self.anchor.x) <= (self.head.y, self.head.x) {
            (self.anchor, self.head)
        } else {
            (self.head, self.anchor)
        }
    }

    fn row_bounds(self, row: u16, area: Rect) -> Option<(u16, u16)> {
        let (start, end) = self.ordered();
        let area_left = area.left().max(self.bounds.left());
        let area_right = area.right().min(self.bounds.right());
        let area_top = area.top().max(self.bounds.top());
        let area_bottom = area.bottom().min(self.bounds.bottom());
        if row < start.y
            || row > end.y
            || row < area_top
            || row >= area_bottom
            || area_left >= area_right
        {
            return None;
        }

        let first = if row == start.y { start.x } else { area_left };
        let last = if row == end.y {
            end.x
        } else {
            area_right.saturating_sub(1)
        };
        let first = first.max(area_left);
        let last = last.min(area_right.saturating_sub(1));
        (first <= last).then_some((first, last))
    }

    pub fn selected_text(self, buffer: &Buffer) -> String {
        let area = buffer.area;
        if area.is_empty() {
            return String::new();
        }

        let (_, end) = self.ordered();
        let mut lines = Vec::new();
        for row in area.top()..area.bottom() {
            let Some((first, last)) = self.row_bounds(row, area) else {
                continue;
            };
            let mut line = String::new();
            for col in first..=last {
                if let Some(cell) = buffer.cell((col, row)) {
                    line.push_str(cell.symbol());
                }
            }
            while line.ends_with(' ') {
                line.pop();
            }
            lines.push(line);
            if row >= end.y {
                break;
            }
        }
        lines.join("\n")
    }

    pub fn render(self, buffer: &mut Buffer) {
        let area = buffer.area;
        let style = Style::default().fg(TEXT_BRIGHT).bg(BG_TEXT_SELECTION);
        for row in area.top()..area.bottom() {
            let Some((first, last)) = self.row_bounds(row, area) else {
                continue;
            };
            for col in first..=last {
                if let Some(cell) = buffer.cell_mut((col, row)) {
                    cell.set_style(style);
                }
            }
        }
    }
}

fn clamp_position(position: Position, bounds: Rect) -> Position {
    if bounds.is_empty() {
        return position;
    }
    Position::new(
        position
            .x
            .clamp(bounds.left(), bounds.right().saturating_sub(1)),
        position
            .y
            .clamp(bounds.top(), bounds.bottom().saturating_sub(1)),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::style::Color;

    #[test]
    fn extracts_forward_and_reverse_selections() {
        let buffer = Buffer::with_lines(["alpha", "bravo", "charlie"]);
        let forward = TextSelection::new(Position::new(2, 0), Position::new(2, 1), buffer.area);
        let reverse = TextSelection::new(Position::new(2, 1), Position::new(2, 0), buffer.area);

        assert_eq!(forward.selected_text(&buffer), "pha\nbra");
        assert_eq!(reverse.selected_text(&buffer), "pha\nbra");
    }

    #[test]
    fn trims_padding_at_line_ends() {
        let mut buffer = Buffer::empty(ratatui::layout::Rect::new(0, 0, 8, 2));
        buffer.set_string(0, 0, "one", Style::default());
        buffer.set_string(0, 1, "two", Style::default());
        let selection = TextSelection::new(Position::new(0, 0), Position::new(7, 1), buffer.area);

        assert_eq!(selection.selected_text(&buffer), "one\ntwo");
    }

    #[test]
    fn renders_only_selected_cells() {
        let mut buffer = Buffer::with_lines(["abcd"]);
        let selection = TextSelection::new(Position::new(1, 0), Position::new(2, 0), buffer.area);
        selection.render(&mut buffer);

        assert_eq!(buffer[(0, 0)].bg, Color::Reset);
        assert_eq!(buffer[(1, 0)].bg, BG_TEXT_SELECTION);
        assert_eq!(buffer[(2, 0)].bg, BG_TEXT_SELECTION);
        assert_eq!(buffer[(3, 0)].bg, Color::Reset);
    }

    #[test]
    fn clamps_selection_to_its_starting_pane() {
        let buffer = Buffer::with_lines(["leftRIGHT", "leftRIGHT", "leftRIGHT"]);
        let right_pane = Rect::new(4, 0, 5, 3);
        let selection = TextSelection::new(Position::new(6, 0), Position::new(0, 2), right_pane);

        assert_eq!(selection.head, Position::new(4, 2));
        assert_eq!(selection.selected_text(&buffer), "GHT\nRIGHT\nR");
    }
}
