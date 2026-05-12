//
// Visual chain builder. Renders a 2D canvas of element blocks connected by
// orthogonal line segments. Mouse-driven: blocks drag, ports rubber-band
// connections, the canvas pans. Inline edits land in the properties strip
// below the canvas.
//

use crate::app::{
    input_port_count, output_port_count, BlockField, ChainElementDraft, ChainForm,
    ChainFormEditor, ConditionKind, ConnectionDraft, Drag, EditTarget, ElementKind, PortSide,
    Selected,
};
use crate::ui::theme::{
    ACCENT, BG, BG_ELEMENT, BG_MENU, BG_SELECTED, BORDER_SUBTLE, DIM, ERROR, MUTED, OK,
    STATUS_RUNNING, TEXT, TEXT_BRIGHT,
};
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

pub const BLOCK_W: u16 = 22;
pub const BLOCK_H: u16 = 5;

//
// Rect helper that stores i32-ish viewport rects. We use this rather than
// ratatui::Rect because some hit areas are zero-width when not active and
// it's easier to reason about with signed ints.
//

#[derive(Default, Clone, Copy)]
pub struct HitRect {
    pub x: u16,
    pub y: u16,
    pub w: u16,
    pub h: u16,
}

impl HitRect {
    pub fn new(x: u16, y: u16, w: u16, h: u16) -> Self {
        Self { x, y, w, h }
    }
    pub fn contains(&self, col: u16, row: u16) -> bool {
        self.w > 0
            && self.h > 0
            && col >= self.x
            && col < self.x.saturating_add(self.w)
            && row >= self.y
            && row < self.y.saturating_add(self.h)
    }
    pub fn is_empty(&self) -> bool {
        self.w == 0 || self.h == 0
    }
}

//
// Hit-test geometry stashed by the renderer for the mouse handler.
//

#[derive(Default, Clone)]
pub struct ChainFormHitMap {
    pub canvas: HitRect,
    pub header_fields: Vec<(EditTarget, HitRect)>,
    pub property_fields: Vec<(EditTarget, HitRect)>,
    pub kind_cycle_button: HitRect,
    pub delete_element_button: HitRect,
    pub cycle_condition_button: HitRect,
    pub delete_connection_button: HitRect,
    pub pick_op_button: HitRect,
    pub palette_buttons: Vec<(ElementKind, HitRect)>,
    pub auto_layout_button: HitRect,
    pub save_button: HitRect,
    pub cancel_button: HitRect,
}

impl ChainFormHitMap {
    //
    // Centre of the visible canvas, in canvas coordinates, used when the
    // user clicks a [+ Kind] palette button to drop a new block.
    //
    pub fn canvas_centre_for_new_block(&self) -> Option<(i32, i32)> {
        if self.canvas.is_empty() {
            return None;
        }
        let cx = self.canvas.x as i32 + self.canvas.w as i32 / 2 - BLOCK_W as i32 / 2;
        let cy = self.canvas.y as i32 + self.canvas.h as i32 / 2 - BLOCK_H as i32 / 2;
        Some((cx, cy))
    }
}

pub fn render_chain_form(f: &mut Frame, area: Rect, form: &ChainForm) -> ChainFormHitMap {
    let chunks = Layout::vertical([
        Constraint::Length(1), // title
        Constraint::Length(1), // divider
        Constraint::Length(3), // header strip
        Constraint::Min(8),    // canvas
        Constraint::Length(4), // properties strip
        Constraint::Length(2), // palette + buttons
        Constraint::Length(1), // hints / error
    ])
    .split(area);

    render_title(f, chunks[0], form);
    render_divider(f, chunks[1]);

    let mut hit = ChainFormHitMap::default();
    render_header_strip(f, chunks[2], form, &mut hit);
    render_canvas(f, chunks[3], form, &mut hit);
    render_properties_strip(f, chunks[4], form, &mut hit);
    render_palette_and_buttons(f, chunks[5], form, &mut hit);
    render_hints(f, chunks[6], form);

    //
    // Overlay editors (op-name picker only — kind/connection edit moved
    // into the canvas itself).
    //
    if let Some(editor) = form.editor.as_ref() {
        render_op_picker(f, area, form, editor);
    }

    hit
}

fn render_title(f: &mut Frame, area: Rect, form: &ChainForm) {
    let title = if form.editing_id.is_some() {
        "Edit Chain"
    } else {
        "New Chain"
    };
    f.render_widget(
        Paragraph::new(Line::from(Span::styled(
            title,
            Style::default()
                .fg(TEXT_BRIGHT)
                .add_modifier(Modifier::BOLD),
        ))),
        area,
    );
}

fn render_divider(f: &mut Frame, area: Rect) {
    f.render_widget(
        Paragraph::new(Line::from(Span::styled(
            "\u{2500}".repeat(area.width as usize),
            Style::default().fg(BORDER_SUBTLE),
        ))),
        area,
    );
}

fn render_header_strip(
    f: &mut Frame,
    area: Rect,
    form: &ChainForm,
    hit: &mut ChainFormHitMap,
) {
    let rows = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
    ])
    .split(area);

    //
    // Row 0: Name (wide) + Save/Cancel right-aligned.
    //
    let name_w = rows[0].width.saturating_sub(40);
    let name_rect = HitRect::new(rows[0].x, rows[0].y, name_w, 1);
    let layout_rect = HitRect::new(
        rows[0].x + rows[0].width.saturating_sub(32),
        rows[0].y,
        11,
        1,
    );
    let save_rect = HitRect::new(
        rows[0].x + rows[0].width.saturating_sub(20),
        rows[0].y,
        9,
        1,
    );
    let cancel_rect = HitRect::new(
        rows[0].x + rows[0].width.saturating_sub(10),
        rows[0].y,
        9,
        1,
    );
    let editing_name = form.editing == Some(EditTarget::HeaderName);
    f.render_widget(
        Paragraph::new(field_line(
            "Name:",
            &form.name,
            editing_name,
            FieldKind::Text,
        )),
        rect_from(name_rect),
    );
    hit.header_fields.push((EditTarget::HeaderName, name_rect));

    f.render_widget(
        Paragraph::new(button_line(" Layout ", false)),
        rect_from(layout_rect),
    );
    f.render_widget(
        Paragraph::new(button_line("  Save  ", true)),
        rect_from(save_rect),
    );
    f.render_widget(
        Paragraph::new(button_line(" Cancel ", false)),
        rect_from(cancel_rect),
    );
    hit.auto_layout_button = layout_rect;
    hit.save_button = save_rect;
    hit.cancel_button = cancel_rect;

    //
    // Row 1: Category + Timeout.
    //
    let cat_w = (rows[1].width / 2).max(10);
    let cat_rect = HitRect::new(rows[1].x, rows[1].y, cat_w, 1);
    let to_rect = HitRect::new(
        rows[1].x + cat_w,
        rows[1].y,
        rows[1].width.saturating_sub(cat_w),
        1,
    );
    let editing_cat = form.editing == Some(EditTarget::HeaderCategory);
    let editing_to = form.editing == Some(EditTarget::HeaderTimeout);
    f.render_widget(
        Paragraph::new(field_line(
            "Category:",
            &form.category,
            editing_cat,
            FieldKind::Text,
        )),
        rect_from(cat_rect),
    );
    f.render_widget(
        Paragraph::new(field_line(
            "Timeout (s):",
            &form.timeout,
            editing_to,
            FieldKind::Number,
        )),
        rect_from(to_rect),
    );
    hit.header_fields.push((EditTarget::HeaderCategory, cat_rect));
    hit.header_fields.push((EditTarget::HeaderTimeout, to_rect));

    //
    // Row 2: Description (single line).
    //
    let desc_rect = HitRect::new(rows[2].x, rows[2].y, rows[2].width, 1);
    let editing_desc = form.editing == Some(EditTarget::HeaderDescription);
    f.render_widget(
        Paragraph::new(field_line(
            "Description:",
            &form.description,
            editing_desc,
            FieldKind::Text,
        )),
        rect_from(desc_rect),
    );
    hit.header_fields.push((EditTarget::HeaderDescription, desc_rect));
}

enum FieldKind {
    Text,
    Number,
}

fn field_line(label: &str, value: &str, editing: bool, _kind: FieldKind) -> Line<'static> {
    let label_style = if editing {
        Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(MUTED)
    };
    let value_style = if editing {
        Style::default().fg(TEXT_BRIGHT).bg(BG_SELECTED)
    } else {
        Style::default().fg(TEXT_BRIGHT)
    };
    let placeholder = if value.is_empty() {
        Some("(click to edit)".to_string())
    } else {
        None
    };
    let cursor = if editing {
        Span::styled("\u{2588}", Style::default().fg(ACCENT))
    } else {
        Span::raw("")
    };
    let value_span = if let Some(p) = placeholder {
        if editing {
            Span::styled("", value_style)
        } else {
            Span::styled(p, Style::default().fg(DIM).add_modifier(Modifier::ITALIC))
        }
    } else {
        Span::styled(value.to_string(), value_style)
    };
    Line::from(vec![
        Span::styled(format!("{} ", label), label_style),
        value_span,
        cursor,
    ])
}

fn button_line(text: &str, primary: bool) -> Line<'static> {
    let style = if primary {
        Style::default().fg(BG).bg(OK).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(MUTED).bg(BG_ELEMENT)
    };
    Line::from(Span::styled(text.to_string(), style))
}

//
// Canvas. Render order: clear, connectors, blocks, rubber-band.
//

fn render_canvas(f: &mut Frame, area: Rect, form: &ChainForm, hit: &mut ChainFormHitMap) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(BORDER_SUBTLE))
        .title(Span::styled(
            " Canvas ",
            Style::default().fg(MUTED).add_modifier(Modifier::BOLD),
        ));
    let inner = block.inner(area);
    f.render_widget(block, area);
    hit.canvas = HitRect::new(inner.x, inner.y, inner.width, inner.height);

    //
    // Fill the canvas background.
    //
    let bg_style = Style::default().bg(BG_MENU);
    let buf = f.buffer_mut();
    for y in inner.y..inner.y + inner.height {
        for x in inner.x..inner.x + inner.width {
            if let Some(cell) = buf.cell_mut((x, y)) {
                cell.set_char(' ');
                cell.set_style(bg_style);
            }
        }
    }

    //
    // Draw connectors first so blocks paint over them.
    //
    for (idx, conn) in form.connections.iter().enumerate() {
        let selected = matches!(form.selected, Selected::Connection(i) if i == idx);
        draw_connector(buf, inner, form, conn, selected);
    }

    //
    // Blocks.
    //
    for el in &form.elements {
        let (cx, cy) = form.block_pos(&el.id);
        let selected = matches!(&form.selected, Selected::Block(id) if id == &el.id);
        draw_block(buf, inner, form, el, cx, cy, selected);
    }

    //
    // Active rubber-band for port drag.
    //
    if let Drag::Port {
        from_id,
        from_port,
        cursor_col,
        cursor_row,
    } = &form.drag
    {
        let from_pos = form.block_pos(from_id);
        let from_x = from_pos.0 + BLOCK_W as i32;
        let from_y = from_pos.1 + 1 + *from_port as i32;
        let to_col = *cursor_col as i32;
        let to_row = *cursor_row as i32;
        let to_canvas_x = (to_col - inner.x as i32) + form.camera_x;
        let to_canvas_y = (to_row - inner.y as i32) + form.camera_y;
        draw_rubberband(
            buf,
            inner,
            form,
            (from_x, from_y),
            (to_canvas_x, to_canvas_y),
        );
    }
}

fn draw_block(
    buf: &mut ratatui::buffer::Buffer,
    canvas: Rect,
    form: &ChainForm,
    el: &ChainElementDraft,
    cx: i32,
    cy: i32,
    selected: bool,
) {
    //
    // Translate canvas coords to viewport cells.
    //
    let vx = cx - form.camera_x + canvas.x as i32;
    let vy = cy - form.camera_y + canvas.y as i32;

    let border_color = if selected {
        ACCENT
    } else {
        kind_color(el.kind)
    };
    let bg_color = if selected { BG_SELECTED } else { BG_ELEMENT };

    let w = BLOCK_W as i32;
    let h = BLOCK_H as i32;

    for j in 0..h {
        for i in 0..w {
            let x = vx + i;
            let y = vy + j;
            if !inside(canvas, x, y) {
                continue;
            }
            let is_top = j == 0;
            let is_bot = j == h - 1;
            let is_left = i == 0;
            let is_right = i == w - 1;
            let ch = if is_top && is_left {
                '\u{256D}' // ╭
            } else if is_top && is_right {
                '\u{256E}' // ╮
            } else if is_bot && is_left {
                '\u{2570}' // ╰
            } else if is_bot && is_right {
                '\u{256F}' // ╯
            } else if is_top || is_bot {
                '\u{2500}' // ─
            } else if is_left || is_right {
                '\u{2502}' // │
            } else {
                ' '
            };
            if let Some(cell) = buf.cell_mut((x as u16, y as u16)) {
                cell.set_char(ch);
                let style = if is_top || is_bot || is_left || is_right {
                    Style::default().fg(border_color).bg(bg_color)
                } else {
                    Style::default().bg(bg_color)
                };
                cell.set_style(style);
            }
        }
    }

    //
    // Header: " KIND  id " (with pill).
    //
    let header_y = vy + 1;
    let pill_text = format!(" {} ", el.kind.short());
    set_text(
        buf,
        canvas,
        vx + 2,
        header_y,
        &pill_text,
        Style::default()
            .fg(BG)
            .bg(kind_color(el.kind))
            .add_modifier(Modifier::BOLD),
    );
    let id_start = vx + 2 + pill_text.chars().count() as i32 + 1;
    let max_id_len = (vx + w - 2 - id_start).max(0) as usize;
    let truncated = truncate(&el.id, max_id_len);
    set_text(
        buf,
        canvas,
        id_start,
        header_y,
        &truncated,
        Style::default()
            .fg(TEXT_BRIGHT)
            .bg(bg_color)
            .add_modifier(Modifier::BOLD),
    );

    //
    // Body: a one-line summary.
    //
    let summary = element_summary(el);
    set_text(
        buf,
        canvas,
        vx + 2,
        vy + 2,
        &truncate(&summary, (w - 4) as usize),
        Style::default().fg(if el.kind == ElementKind::Trigger {
            TEXT_BRIGHT
        } else {
            TEXT
        })
        .bg(bg_color),
    );

    //
    // Ports.
    //
    if input_port_count(el.kind) > 0 {
        let px = vx - 1;
        let py = vy + (h / 2);
        set_cell(
            buf,
            canvas,
            px,
            py,
            '\u{25CF}', // ●
            Style::default().fg(border_color),
        );
        //
        // Connector stub into block edge.
        //
        set_cell(
            buf,
            canvas,
            vx,
            py,
            '\u{2524}', // ┤
            Style::default().fg(border_color).bg(bg_color),
        );
    }
    let outputs = output_port_count(el.kind);
    for port in 0..outputs {
        let px = vx + w;
        let py = vy + 1 + port as i32;
        set_cell(
            buf,
            canvas,
            px,
            py,
            '\u{25CF}',
            Style::default().fg(border_color),
        );
        set_cell(
            buf,
            canvas,
            vx + w - 1,
            py,
            '\u{251C}', // ├
            Style::default().fg(border_color).bg(bg_color),
        );
    }
}

fn element_summary(el: &ChainElementDraft) -> String {
    match el.kind {
        ElementKind::Trigger => "manual trigger".to_string(),
        ElementKind::Operation => {
            if el.op_name.is_empty() {
                "(no op selected)".to_string()
            } else {
                el.op_name.clone()
            }
        }
        ElementKind::Transform => {
            if el.prompt.is_empty() {
                "(no prompt)".to_string()
            } else {
                first_line(&el.prompt)
            }
        }
        ElementKind::GenericPrompt => {
            if el.prompt.is_empty() {
                "(no prompt)".to_string()
            } else {
                first_line(&el.prompt)
            }
        }
        ElementKind::Memory => {
            let mode = if el.memory_mode == 0 { "store" } else { "load" };
            if el.memory_key.is_empty() {
                format!("{} (no key)", mode)
            } else {
                format!("{} {}", mode, el.memory_key)
            }
        }
        ElementKind::Loop => format!("max {} iters", el.max_iterations),
        ElementKind::Tool => {
            if el.tool_name.is_empty() {
                "(no tool)".to_string()
            } else {
                el.tool_name.clone()
            }
        }
        ElementKind::Payload => {
            if el.payload_id.is_empty() {
                "(no payload)".to_string()
            } else {
                el.payload_id.clone()
            }
        }
        ElementKind::Termination => "end".to_string(),
    }
}

fn first_line(s: &str) -> String {
    s.lines().next().unwrap_or("").to_string()
}

fn truncate(s: &str, max: usize) -> String {
    if max == 0 {
        return String::new();
    }
    let mut out: String = s.chars().take(max).collect();
    if s.chars().count() > max && max > 1 {
        out.truncate(out.len().saturating_sub(1));
        out.push('\u{2026}'); // …
    }
    out
}

//
// Connector routing. Produces a sequence of canvas-coord cells the line
// passes through. The renderer walks the path and sets box-drawing
// characters along it.
//

pub fn route_connection(form: &ChainForm, conn: &ConnectionDraft) -> Vec<(i32, i32)> {
    let from_pos = form.block_pos(&conn.from_element);
    let to_pos = form.block_pos(&conn.to_element);
    let from = (
        from_pos.0 + BLOCK_W as i32,
        from_pos.1 + 1 + conn.from_port as i32,
    );
    let to = (to_pos.0 - 1, to_pos.1 + (BLOCK_H as i32 / 2));
    orthogonal_path(from, to)
}

//
// Simple L-shape Manhattan path. Forward: right → vert → right. Back:
// right → down → left → down → right, routed below both blocks.
//

fn orthogonal_path(from: (i32, i32), to: (i32, i32)) -> Vec<(i32, i32)> {
    let mut path = Vec::new();
    let (fx, fy) = from;
    let (tx, ty) = to;
    if tx > fx + 1 {
        let mid_x = (fx + tx) / 2;
        push_h(&mut path, fy, fx, mid_x);
        push_v(&mut path, mid_x, fy, ty);
        push_h(&mut path, ty, mid_x, tx);
    } else {
        //
        // Back edge — go right, drop down past both, left, then up.
        //
        let detour_y = fy.max(ty) + (BLOCK_H as i32 / 2) + 2;
        let right_x = fx + 4;
        let left_x = tx - 4;
        push_h(&mut path, fy, fx, right_x);
        push_v(&mut path, right_x, fy, detour_y);
        push_h(&mut path, detour_y, left_x, right_x);
        push_v(&mut path, left_x, detour_y, ty);
        push_h(&mut path, ty, left_x, tx);
    }
    path
}

fn push_h(path: &mut Vec<(i32, i32)>, y: i32, x1: i32, x2: i32) {
    let (a, b) = if x1 <= x2 { (x1, x2) } else { (x2, x1) };
    for x in a..=b {
        path.push((x, y));
    }
}

fn push_v(path: &mut Vec<(i32, i32)>, x: i32, y1: i32, y2: i32) {
    let (a, b) = if y1 <= y2 { (y1, y2) } else { (y2, y1) };
    for y in a..=b {
        path.push((x, y));
    }
}

fn draw_connector(
    buf: &mut ratatui::buffer::Buffer,
    canvas: Rect,
    form: &ChainForm,
    conn: &ConnectionDraft,
    selected: bool,
) {
    let path = route_connection(form, conn);
    let color = if selected {
        ACCENT
    } else {
        match conn.condition {
            ConditionKind::OnSuccess => OK,
            ConditionKind::OnFailure => ERROR,
            ConditionKind::None => MUTED,
        }
    };
    for (i, &(x, y)) in path.iter().enumerate() {
        let vx = x - form.camera_x + canvas.x as i32;
        let vy = y - form.camera_y + canvas.y as i32;
        if !inside(canvas, vx, vy) {
            continue;
        }
        //
        // Decide char by looking at prev/next neighbours so corners join.
        //
        let prev = if i > 0 { Some(path[i - 1]) } else { None };
        let next = path.get(i + 1).copied();
        let ch = path_char((x, y), prev, next);
        if let Some(cell) = buf.cell_mut((vx as u16, vy as u16)) {
            cell.set_char(ch);
            cell.set_style(Style::default().fg(color).bg(BG_MENU));
        }
    }
    //
    // Arrowhead at the end.
    //
    if let Some(&(x, y)) = path.last() {
        let vx = x - form.camera_x + canvas.x as i32;
        let vy = y - form.camera_y + canvas.y as i32;
        if inside(canvas, vx, vy) {
            if let Some(cell) = buf.cell_mut((vx as u16, vy as u16)) {
                cell.set_char('\u{25B6}'); // ▶
                cell.set_style(Style::default().fg(color).bg(BG_MENU));
            }
        }
    }
}

fn path_char(
    here: (i32, i32),
    prev: Option<(i32, i32)>,
    next: Option<(i32, i32)>,
) -> char {
    let (x, y) = here;
    let dx_p = prev.map(|p| p.0 - x).unwrap_or(0);
    let dy_p = prev.map(|p| p.1 - y).unwrap_or(0);
    let dx_n = next.map(|p| p.0 - x).unwrap_or(0);
    let dy_n = next.map(|p| p.1 - y).unwrap_or(0);
    //
    // Direction set of incoming + outgoing.
    //
    let mut h = false;
    let mut v = false;
    if dx_p.abs() == 1 || dx_n.abs() == 1 {
        h = true;
    }
    if dy_p.abs() == 1 || dy_n.abs() == 1 {
        v = true;
    }
    if h && v {
        let goes_right = dx_n == 1 || dx_p == -1;
        let goes_left = dx_n == -1 || dx_p == 1;
        let goes_down = dy_n == 1 || dy_p == -1;
        let goes_up = dy_n == -1 || dy_p == 1;
        if goes_right && goes_down {
            return '\u{256D}'; // ╭
        }
        if goes_left && goes_down {
            return '\u{256E}'; // ╮
        }
        if goes_right && goes_up {
            return '\u{2570}'; // ╰
        }
        if goes_left && goes_up {
            return '\u{256F}'; // ╯
        }
    }
    if h {
        '\u{2500}' // ─
    } else if v {
        '\u{2502}' // │
    } else {
        '\u{2022}' // •
    }
}

//
// Rubber-band drawn while dragging a connection out of a port.
//

fn draw_rubberband(
    buf: &mut ratatui::buffer::Buffer,
    canvas: Rect,
    form: &ChainForm,
    from: (i32, i32),
    to: (i32, i32),
) {
    let path = orthogonal_path(from, to);
    for (i, &(x, y)) in path.iter().enumerate() {
        let vx = x - form.camera_x + canvas.x as i32;
        let vy = y - form.camera_y + canvas.y as i32;
        if !inside(canvas, vx, vy) {
            continue;
        }
        let prev = if i > 0 { Some(path[i - 1]) } else { None };
        let next = path.get(i + 1).copied();
        let ch = path_char((x, y), prev, next);
        if let Some(cell) = buf.cell_mut((vx as u16, vy as u16)) {
            cell.set_char(ch);
            cell.set_style(Style::default().fg(ACCENT).bg(BG_MENU));
        }
    }
    if let Some(&(x, y)) = path.last() {
        let vx = x - form.camera_x + canvas.x as i32;
        let vy = y - form.camera_y + canvas.y as i32;
        if inside(canvas, vx, vy) {
            if let Some(cell) = buf.cell_mut((vx as u16, vy as u16)) {
                cell.set_char('\u{2715}'); // ✕ tentative end
                cell.set_style(Style::default().fg(ACCENT).bg(BG_MENU));
            }
        }
    }
}

//
// Properties strip — fields for the currently selected block (or
// connection), with click-to-edit hit rects.
//

fn render_properties_strip(
    f: &mut Frame,
    area: Rect,
    form: &ChainForm,
    hit: &mut ChainFormHitMap,
) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(BORDER_SUBTLE))
        .title(Span::styled(
            " Properties ",
            Style::default().fg(MUTED).add_modifier(Modifier::BOLD),
        ));
    let inner = block.inner(area);
    f.render_widget(block, area);

    match form.selected.clone() {
        Selected::None => {
            f.render_widget(
                Paragraph::new(Span::styled(
                    "(click a block or connection to edit)",
                    Style::default().fg(MUTED).add_modifier(Modifier::ITALIC),
                )),
                inner,
            );
        }
        Selected::Block(id) => {
            let Some(el) = form.elements.iter().find(|e| e.id == id).cloned() else {
                return;
            };
            render_block_properties(f, inner, form, &el, hit);
        }
        Selected::Connection(idx) => {
            let Some(conn) = form.connections.get(idx).cloned() else {
                return;
            };
            render_connection_properties(f, inner, form, &conn, idx, hit);
        }
    }
}

fn render_block_properties(
    f: &mut Frame,
    inner: Rect,
    form: &ChainForm,
    el: &ChainElementDraft,
    hit: &mut ChainFormHitMap,
) {
    //
    // Row 0: pill + id, [Kind ◂▸], [Delete]
    //
    let mut x = inner.x;
    let pill = format!(" {} ", el.kind.short());
    f.render_widget(
        Paragraph::new(Span::styled(
            pill.clone(),
            Style::default()
                .fg(BG)
                .bg(kind_color(el.kind))
                .add_modifier(Modifier::BOLD),
        )),
        Rect::new(x, inner.y, pill.chars().count() as u16, 1),
    );
    x += pill.chars().count() as u16 + 1;
    let id_w = (el.id.chars().count() as u16).min(inner.width / 3);
    f.render_widget(
        Paragraph::new(Span::styled(
            el.id.clone(),
            Style::default().fg(TEXT_BRIGHT).add_modifier(Modifier::BOLD),
        )),
        Rect::new(x, inner.y, id_w, 1),
    );
    x += id_w + 2;
    let kind_cycle_text = format!("\u{25C0} {} \u{25B6}", el.kind.label());
    let kw = kind_cycle_text.chars().count() as u16;
    let kind_cycle_rect = HitRect::new(x, inner.y, kw, 1);
    f.render_widget(
        Paragraph::new(Span::styled(
            kind_cycle_text,
            Style::default().fg(ACCENT),
        )),
        rect_from(kind_cycle_rect),
    );
    hit.kind_cycle_button = kind_cycle_rect;
    let del_rect = HitRect::new(inner.x + inner.width.saturating_sub(11), inner.y, 10, 1);
    f.render_widget(
        Paragraph::new(Span::styled(
            "[Delete]",
            Style::default().fg(ERROR),
        )),
        rect_from(del_rect),
    );
    hit.delete_element_button = del_rect;

    //
    // Row 1+: per-kind fields, each clickable.
    //
    let mut row = inner.y + 1;
    let mut fields: Vec<(BlockField, &str, &str)> = Vec::new();
    match el.kind {
        ElementKind::Operation => {
            fields.push((BlockField::OpName, "Op", el.op_name.as_str()));
            fields.push((BlockField::ModelRef, "Model", el.model_ref.as_str()));
        }
        ElementKind::Transform => {
            fields.push((BlockField::Prompt, "Prompt", el.prompt.as_str()));
            fields.push((BlockField::ModelRef, "Model", el.model_ref.as_str()));
        }
        ElementKind::GenericPrompt => {
            fields.push((BlockField::Prompt, "Prompt", el.prompt.as_str()));
        }
        ElementKind::Memory => {
            fields.push((BlockField::MemoryKey, "Key", el.memory_key.as_str()));
        }
        ElementKind::Loop => {
            fields.push((
                BlockField::MaxIterations,
                "Max iters",
                el.max_iterations.as_str(),
            ));
        }
        ElementKind::Tool => {
            fields.push((BlockField::ToolName, "Tool", el.tool_name.as_str()));
            fields.push((BlockField::ToolParams, "Params", el.tool_params.as_str()));
        }
        ElementKind::Payload => {
            fields.push((BlockField::PayloadId, "Payload", el.payload_id.as_str()));
        }
        ElementKind::Trigger | ElementKind::Termination => {}
    }

    for (field, label, value) in fields {
        if row >= inner.y + inner.height {
            break;
        }
        let rect = HitRect::new(inner.x, row, inner.width, 1);
        let editing = form.editing
            == Some(EditTarget::BlockProp {
                id: el.id.clone(),
                field,
            });
        let kind = if matches!(field, BlockField::MaxIterations) {
            FieldKind::Number
        } else {
            FieldKind::Text
        };
        let mut line = field_line(&format!("{}:", label), value, editing, kind);
        if matches!(field, BlockField::OpName) {
            line.spans.push(Span::styled(
                "  [pick]",
                Style::default().fg(OK).add_modifier(Modifier::BOLD),
            ));
        }
        f.render_widget(Paragraph::new(line), rect_from(rect));
        hit.property_fields.push((
            EditTarget::BlockProp {
                id: el.id.clone(),
                field,
            },
            rect,
        ));
        if matches!(field, BlockField::OpName) {
            //
            // The "[pick]" suffix is its own hit rect on the right side.
            //
            let pick_rect = HitRect::new(
                inner.x + inner.width.saturating_sub(8),
                row,
                8,
                1,
            );
            hit.pick_op_button = pick_rect;
        }
        row += 1;
    }
}

fn render_connection_properties(
    f: &mut Frame,
    inner: Rect,
    form: &ChainForm,
    conn: &ConnectionDraft,
    idx: usize,
    hit: &mut ChainFormHitMap,
) {
    let from_label = element_label_for(form, &conn.from_element);
    let to_label = element_label_for(form, &conn.to_element);
    let header = format!(
        "{}  :{}  →  {}  :{}",
        from_label, conn.from_port, to_label, conn.to_port
    );
    f.render_widget(
        Paragraph::new(Span::styled(
            header,
            Style::default().fg(TEXT_BRIGHT).add_modifier(Modifier::BOLD),
        )),
        Rect::new(inner.x, inner.y, inner.width, 1),
    );

    let row = inner.y + 1;
    let cond_text = format!(
        "Condition: \u{25C0} {} \u{25B6}",
        condition_label(conn.condition)
    );
    let cw = cond_text.chars().count() as u16;
    let cond_rect = HitRect::new(inner.x, row, cw, 1);
    f.render_widget(
        Paragraph::new(Span::styled(
            cond_text,
            Style::default().fg(condition_color(conn.condition)),
        )),
        rect_from(cond_rect),
    );
    hit.cycle_condition_button = cond_rect;

    let port_x = inner.x + cw + 4;
    let from_port_text = format!("from port [{}]", conn.from_port);
    let fpw = from_port_text.chars().count() as u16;
    let fp_rect = HitRect::new(port_x, row, fpw, 1);
    let editing_from = form.editing
        == Some(EditTarget::ConnectionPort {
            idx,
            side: PortSide::From,
        });
    f.render_widget(
        Paragraph::new(field_line(
            "from port:",
            &conn.from_port.to_string(),
            editing_from,
            FieldKind::Number,
        )),
        rect_from(fp_rect),
    );
    hit.property_fields.push((
        EditTarget::ConnectionPort {
            idx,
            side: PortSide::From,
        },
        fp_rect,
    ));

    let to_port_text = format!("to port [{}]", conn.to_port);
    let tp_x = port_x + fpw + 4;
    let tpw = to_port_text.chars().count() as u16;
    let tp_rect = HitRect::new(tp_x, row, tpw, 1);
    let editing_to = form.editing
        == Some(EditTarget::ConnectionPort {
            idx,
            side: PortSide::To,
        });
    f.render_widget(
        Paragraph::new(field_line(
            "to port:",
            &conn.to_port.to_string(),
            editing_to,
            FieldKind::Number,
        )),
        rect_from(tp_rect),
    );
    hit.property_fields.push((
        EditTarget::ConnectionPort {
            idx,
            side: PortSide::To,
        },
        tp_rect,
    ));

    let del_rect = HitRect::new(inner.x + inner.width.saturating_sub(11), row, 10, 1);
    f.render_widget(
        Paragraph::new(Span::styled(
            "[Delete]",
            Style::default().fg(ERROR),
        )),
        rect_from(del_rect),
    );
    hit.delete_connection_button = del_rect;
}

fn element_label_for(form: &ChainForm, id: &str) -> String {
    form.elements
        .iter()
        .find(|e| e.id == id)
        .map(|e| e.id.clone())
        .unwrap_or_else(|| id.to_string())
}

//
// Palette + (mirrored) save/cancel.
//

fn render_palette_and_buttons(
    f: &mut Frame,
    area: Rect,
    _form: &ChainForm,
    hit: &mut ChainFormHitMap,
) {
    let rows = Layout::vertical([Constraint::Length(1), Constraint::Length(1)]).split(area);
    let mut x = rows[0].x;
    let y = rows[0].y;
    for k in ElementKind::ALL.iter() {
        let label = format!("[+ {}]", k.short());
        let w = label.chars().count() as u16;
        if x + w >= rows[0].x + rows[0].width {
            break;
        }
        let rect = HitRect::new(x, y, w, 1);
        f.render_widget(
            Paragraph::new(Span::styled(
                label,
                Style::default()
                    .fg(BG)
                    .bg(kind_color(*k))
                    .add_modifier(Modifier::BOLD),
            )),
            rect_from(rect),
        );
        hit.palette_buttons.push((*k, rect));
        x += w + 1;
    }
}

fn render_hints(f: &mut Frame, area: Rect, form: &ChainForm) {
    let key = Style::default().fg(TEXT_BRIGHT);
    let lbl = Style::default().fg(MUTED);
    let line = if let Some(ref err) = form.error {
        Line::from(vec![
            Span::styled("\u{26A0}", Style::default().fg(ERROR)),
            Span::raw(" "),
            Span::styled(err.clone(), Style::default().fg(ERROR)),
        ])
    } else {
        Line::from(vec![
            Span::styled("drag", key),
            Span::styled(" block / port", lbl),
            Span::raw("   "),
            Span::styled("click", key),
            Span::styled(" select / edit", lbl),
            Span::raw("   "),
            Span::styled("scroll", key),
            Span::styled(" pan", lbl),
            Span::raw("   "),
            Span::styled("del", key),
            Span::styled(" remove", lbl),
            Span::raw("   "),
            Span::styled("^s", key),
            Span::styled(" save", lbl),
            Span::raw("   "),
            Span::styled("esc", key),
            Span::styled(" cancel", lbl),
        ])
    };
    f.render_widget(Paragraph::new(line), area);
}

fn render_op_picker(f: &mut Frame, area: Rect, form: &ChainForm, editor: &ChainFormEditor) {
    let ChainFormEditor::PickOpName { cursor, filter } = editor;
    let popup_w = 60u16.min(area.width.saturating_sub(4));
    let popup_h = 16u16.min(area.height.saturating_sub(4));
    let x = area.x + (area.width - popup_w) / 2;
    let y = area.y + (area.height - popup_h) / 2;
    let rect = Rect::new(x, y, popup_w, popup_h);
    f.render_widget(Clear, rect);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(ACCENT))
        .style(Style::default().bg(BG_MENU));
    let inner = block.inner(rect);
    f.render_widget(block, rect);

    let mut lines = vec![Line::from(Span::styled(
        "Pick Operation",
        Style::default()
            .fg(TEXT_BRIGHT)
            .add_modifier(Modifier::BOLD),
    ))];
    lines.push(Line::from(vec![
        Span::styled("filter: ", Style::default().fg(MUTED)),
        Span::styled(filter.clone(), Style::default().fg(ACCENT)),
        Span::styled("\u{2588}", Style::default().fg(ACCENT)),
    ]));
    lines.push(Line::from(""));
    let filtered: Vec<&String> = form
        .available_op_names
        .iter()
        .filter(|n| filter.is_empty() || n.to_lowercase().contains(&filter.to_lowercase()))
        .collect();
    let max_rows = (inner.height as usize).saturating_sub(5);
    let start = if *cursor >= max_rows && max_rows > 0 {
        *cursor + 1 - max_rows
    } else {
        0
    };
    for (i, name) in filtered.iter().enumerate().skip(start).take(max_rows) {
        let selected = i == *cursor;
        let style = if selected {
            Style::default().fg(TEXT_BRIGHT).bg(BG_SELECTED)
        } else {
            Style::default().fg(TEXT)
        };
        lines.push(Line::from(vec![
            Span::raw(if selected { " \u{276F} " } else { "   " }),
            Span::styled((*name).clone(), style),
        ]));
    }
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "enter pick    esc cancel",
        Style::default().fg(DIM),
    )));
    f.render_widget(Paragraph::new(lines), inner);
}

//
// Utilities.
//

fn rect_from(hr: HitRect) -> Rect {
    Rect::new(hr.x, hr.y, hr.w, hr.h)
}

fn inside(canvas: Rect, x: i32, y: i32) -> bool {
    x >= canvas.x as i32
        && x < canvas.x as i32 + canvas.width as i32
        && y >= canvas.y as i32
        && y < canvas.y as i32 + canvas.height as i32
}

fn set_cell(
    buf: &mut ratatui::buffer::Buffer,
    canvas: Rect,
    x: i32,
    y: i32,
    ch: char,
    style: Style,
) {
    if !inside(canvas, x, y) {
        return;
    }
    if let Some(cell) = buf.cell_mut((x as u16, y as u16)) {
        cell.set_char(ch);
        cell.set_style(style);
    }
}

fn set_text(
    buf: &mut ratatui::buffer::Buffer,
    canvas: Rect,
    x: i32,
    y: i32,
    text: &str,
    style: Style,
) {
    let mut cur = x;
    for ch in text.chars() {
        set_cell(buf, canvas, cur, y, ch, style);
        cur += 1;
    }
}

fn kind_color(kind: ElementKind) -> Color {
    match kind {
        ElementKind::Trigger => STATUS_RUNNING,
        ElementKind::Operation => ACCENT,
        ElementKind::Transform => OK,
        ElementKind::GenericPrompt => MUTED,
        ElementKind::Memory => ACCENT,
        ElementKind::Loop => STATUS_RUNNING,
        ElementKind::Tool => OK,
        ElementKind::Payload => ACCENT,
        ElementKind::Termination => ERROR,
    }
}

fn condition_label(c: ConditionKind) -> &'static str {
    match c {
        ConditionKind::None => "any",
        ConditionKind::OnSuccess => "on success",
        ConditionKind::OnFailure => "on failure",
    }
}

fn condition_color(c: ConditionKind) -> Color {
    match c {
        ConditionKind::None => MUTED,
        ConditionKind::OnSuccess => OK,
        ConditionKind::OnFailure => ERROR,
    }
}
