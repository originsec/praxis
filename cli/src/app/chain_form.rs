use super::*;
use crate::ui::chain_form::{BLOCK_H, BLOCK_W};
use common::{
    ChainConnection, ChainDefinitionInput, ChainElement, ChainTriggerType, ConnectionCondition,
    ElementPosition, MemoryMode,
};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use std::collections::{HashMap, HashSet};

//
// Spacing for the initial auto-layout. A column is one block width plus a
// gap; a row is one block height plus a gap.
//

pub const LAYOUT_COL_GAP: i32 = BLOCK_W as i32 + 6;
pub const LAYOUT_ROW_GAP: i32 = BLOCK_H as i32 + 2;
pub const CANVAS_ORIGIN_X: i32 = 4;
pub const CANVAS_ORIGIN_Y: i32 = 2;

impl App {
    //
    // Open a fresh chain form. Seeds it with a Trigger + Termination pair
    // connected to each other so the user always has a valid scaffold to
    // extend.
    //

    pub(crate) fn open_new_chain_form(&mut self) {
        let ops: Vec<String> = self
            .operations
            .op_definitions
            .iter()
            .filter(|d| !d.disabled)
            .map(|d| d.full_name.clone())
            .collect();
        let mut form = ChainForm::new(ops);

        let trig_id = form.next_element_id(ElementKind::Trigger);
        let term_id = form.next_element_id(ElementKind::Termination);
        form.elements
            .push(ChainElementDraft::new(trig_id.clone(), ElementKind::Trigger));
        form.elements.push(ChainElementDraft::new(
            term_id.clone(),
            ElementKind::Termination,
        ));
        form.connections.push(ConnectionDraft {
            id: "c_1".to_string(),
            from_element: trig_id.clone(),
            to_element: term_id.clone(),
            from_port: 0,
            to_port: 0,
            condition: ConditionKind::None,
        });

        auto_layout(&mut form);
        self.chain_form = Some(form);
    }

    pub(crate) fn open_edit_chain_form_for(&mut self, chain: common::ChainDefinitionFull) {
        let ops: Vec<String> = self
            .operations
            .op_definitions
            .iter()
            .filter(|d| !d.disabled)
            .map(|d| d.full_name.clone())
            .collect();
        let mut form = ChainForm::new(ops);
        form.editing_id = Some(chain.id);
        form.name = chain.name;
        form.description = chain.description;
        form.category = chain.category;
        form.timeout = chain.timeout.map(|t| t.to_string()).unwrap_or_default();

        for el in &chain.elements {
            form.elements.push(element_to_draft(el));
            form.element_id_seq += 1;
        }
        for conn in &chain.connections {
            form.connections.push(ConnectionDraft {
                id: conn.id.clone(),
                from_element: conn.from_element.clone(),
                to_element: conn.to_element.clone(),
                from_port: conn.from_port,
                to_port: conn.to_port,
                condition: match conn.condition {
                    Some(ConnectionCondition::OnSuccess) => ConditionKind::OnSuccess,
                    Some(ConnectionCondition::OnFailure) => ConditionKind::OnFailure,
                    None => ConditionKind::None,
                },
            });
        }
        //
        // Use saved positions where available; auto-layout fills in any
        // elements that don't have a stored position (e.g. older chains).
        //
        for (id, pos) in &chain.positions {
            form.positions
                .insert(id.clone(), (pos.x as i32, pos.y as i32));
        }
        if form.positions.is_empty() {
            auto_layout(&mut form);
        } else {
            //
            // Even with positions, run layout for any newly added
            // elements that lack one.
            //
            let ids_missing: Vec<String> = form
                .elements
                .iter()
                .filter(|e| !form.positions.contains_key(&e.id))
                .map(|e| e.id.clone())
                .collect();
            for id in ids_missing {
                let pos = next_free_position(&form);
                form.positions.insert(id, pos);
            }
        }

        self.chain_form = Some(form);
    }

    pub(crate) fn edit_selected_chain(&mut self) {
        let filtered = self.filtered_library();
        let Some(&(idx, is_chain)) = filtered.get(self.operations.library_selected) else {
            return;
        };
        if !is_chain {
            return;
        }
        let chain_id = self.operations.chain_definitions[idx].id.clone();
        let client = self.client.clone();
        let tx = self.event_tx.clone();
        tokio::spawn(async move {
            let Some(tx) = tx else { return };
            let _ = client.request_chain_def(&chain_id).await;
            for _ in 0..30 {
                tokio::time::sleep(Duration::from_millis(50)).await;
                if let Some(chain) = client.get_current_chain().await {
                    if chain.id == chain_id {
                        let _ = tx.send(crate::event::AppEvent::ChainLoadedForEdit { chain });
                        return;
                    }
                }
            }
        });
    }

    //
    // Submit: validate, convert drafts and saved positions to a
    // ChainDefinitionInput, dispatch ChainCreate/ChainUpdate.
    //

    pub(crate) async fn submit_chain_form(&mut self) {
        let Some(form) = self.chain_form.as_mut() else {
            return;
        };

        if form.name.trim().is_empty() {
            form.error = Some("Name is required".to_string());
            return;
        }
        if form.elements.is_empty() {
            form.error = Some("Chain must have at least one element".to_string());
            return;
        }

        let trigger_count = form
            .elements
            .iter()
            .filter(|e| e.kind == ElementKind::Trigger)
            .count();
        if trigger_count != 1 {
            form.error = Some("Exactly one Trigger element is required".to_string());
            return;
        }
        let term_count = form
            .elements
            .iter()
            .filter(|e| e.kind == ElementKind::Termination)
            .count();
        if term_count != 1 {
            form.error = Some("Exactly one Termination element is required".to_string());
            return;
        }

        let elements: Vec<ChainElement> = form.elements.iter().map(draft_to_element).collect();
        let connections: Vec<ChainConnection> = form
            .connections
            .iter()
            .map(|c| ChainConnection {
                id: c.id.clone(),
                from_element: c.from_element.clone(),
                to_element: c.to_element.clone(),
                from_port: c.from_port,
                to_port: c.to_port,
                condition: match c.condition {
                    ConditionKind::None => None,
                    ConditionKind::OnSuccess => Some(ConnectionCondition::OnSuccess),
                    ConditionKind::OnFailure => Some(ConnectionCondition::OnFailure),
                },
            })
            .collect();

        let timeout = if form.timeout.trim().is_empty() {
            None
        } else {
            match form.timeout.parse::<u64>() {
                Ok(v) => Some(v),
                Err(_) => {
                    form.error = Some("Timeout must be a number".to_string());
                    return;
                }
            }
        };

        let positions: HashMap<String, ElementPosition> = form
            .positions
            .iter()
            .map(|(id, (x, y))| {
                (
                    id.clone(),
                    ElementPosition {
                        x: *x as f64,
                        y: *y as f64,
                    },
                )
            })
            .collect();

        let definition = ChainDefinitionInput {
            name: form.name.trim().to_string(),
            description: form.description.clone(),
            category: form.category.trim().to_string(),
            elements,
            connections,
            disabled: false,
            timeout,
            positions,
        };

        let editing_id = form.editing_id.clone();
        let result = if let Some(id) = editing_id {
            self.client.update_chain_def(id, definition).await
        } else {
            self.client.add_chain_def(definition).await
        };

        if let Err(e) = result {
            if let Some(form) = self.chain_form.as_mut() {
                form.error = Some(format!("Submit failed: {}", e));
            }
            return;
        }

        self.chain_form = None;
        self.refresh_library_after(Duration::from_millis(300));
    }

    //
    // Adds a new element of the given kind at the centre of the visible
    // canvas viewport, selects it, and clears any in-flight edit state.
    //

    pub(crate) fn add_element_at_centre(&mut self, kind: ElementKind) {
        let centre = self
            .chain_form_hits
            .borrow()
            .canvas_centre_for_new_block();
        let Some(form) = self.chain_form.as_mut() else {
            return;
        };
        let id = form.next_element_id(kind);
        let draft = ChainElementDraft::new(id.clone(), kind);
        form.elements.push(draft);
        let pos = centre.unwrap_or_else(|| next_free_position(form));
        form.positions.insert(id.clone(), pos);
        form.selected = Selected::Block(id);
        form.editing = None;
    }

    //
    // Top-level key dispatch when the chain form is open. The canvas is
    // primarily mouse-driven; the keyboard is used for typing into the
    // active inline edit target and for the global save/cancel shortcuts.
    //

    pub(crate) async fn handle_chain_form_key(&mut self, key: KeyEvent) {
        //
        // Op picker overlay swallows input first.
        //
        if self.chain_form.as_ref().and_then(|f| f.editor.as_ref()).is_some() {
            self.handle_op_picker_key(key).await;
            return;
        }

        match key.code {
            KeyCode::Esc => {
                if let Some(form) = self.chain_form.as_mut() {
                    if form.editing.is_some() {
                        form.editing = None;
                        return;
                    }
                }
                self.chain_form = None;
                return;
            }
            KeyCode::Char('s') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.submit_chain_form().await;
                return;
            }
            _ => {}
        }

        //
        // Inline editing: type into the focused field.
        //
        if let Some(form) = self.chain_form.as_mut() {
            if form.editing.is_some() {
                handle_edit_key(form, key);
                return;
            }
        }

        //
        // No active edit target — arrows pan the canvas and Delete
        // removes the selection.
        //
        if let Some(form) = self.chain_form.as_mut() {
            match key.code {
                KeyCode::Left => form.camera_x -= 4,
                KeyCode::Right => form.camera_x += 4,
                KeyCode::Up => form.camera_y -= 2,
                KeyCode::Down => form.camera_y += 2,
                KeyCode::Delete | KeyCode::Backspace => {
                    delete_selection(form);
                }
                _ => {}
            }
        }
    }

    async fn handle_op_picker_key(&mut self, key: KeyEvent) {
        if matches!(key.code, KeyCode::Esc) {
            if let Some(form) = self.chain_form.as_mut() {
                form.editor = None;
            }
            return;
        }
        let Some(form) = self.chain_form.as_mut() else {
            return;
        };
        let Some(ChainFormEditor::PickOpName { mut cursor, mut filter }) = form.editor.take() else {
            return;
        };
        let filtered: Vec<String> = form
            .available_op_names
            .iter()
            .filter(|n| filter.is_empty() || n.to_lowercase().contains(&filter.to_lowercase()))
            .cloned()
            .collect();
        match key.code {
            KeyCode::Up => {
                if cursor > 0 {
                    cursor -= 1;
                }
            }
            KeyCode::Down => {
                if cursor + 1 < filtered.len() {
                    cursor += 1;
                }
            }
            KeyCode::Enter => {
                if let Some(name) = filtered.get(cursor) {
                    if let Some(el) = form.selected_block_mut() {
                        el.op_name = name.clone();
                    }
                    return;
                }
            }
            KeyCode::Backspace => {
                filter.pop();
                cursor = 0;
            }
            KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                filter.push(c);
                cursor = 0;
            }
            _ => {}
        }
        form.editor = Some(ChainFormEditor::PickOpName { cursor, filter });
    }

    //
    // Mouse handler — see ui/chain_form.rs for the geometry the hit map
    // is computed against. Everything except inline text edits and the op
    // picker happens through this path.
    //

    pub(crate) async fn handle_chain_form_mouse(&mut self, mouse: MouseEvent) {
        if self.chain_form.as_ref().and_then(|f| f.editor.as_ref()).is_some() {
            //
            // Op picker overlay: any click outside closes it; clicks
            // inside are handled by the picker (currently keyboard-only).
            //
            if matches!(mouse.kind, MouseEventKind::Down(MouseButton::Left)) {
                if let Some(form) = self.chain_form.as_mut() {
                    form.editor = None;
                }
            }
            return;
        }

        match mouse.kind {
            MouseEventKind::Down(MouseButton::Left) => self.canvas_mouse_down(mouse).await,
            MouseEventKind::Drag(MouseButton::Left) => self.canvas_mouse_drag(mouse),
            MouseEventKind::Up(MouseButton::Left) => self.canvas_mouse_up(mouse),
            MouseEventKind::ScrollUp => self.canvas_scroll(0, -2),
            MouseEventKind::ScrollDown => self.canvas_scroll(0, 2),
            _ => {}
        }
    }

    fn canvas_scroll(&mut self, dx: i32, dy: i32) {
        if let Some(form) = self.chain_form.as_mut() {
            form.camera_x += dx;
            form.camera_y += dy;
        }
    }

    async fn canvas_mouse_down(&mut self, mouse: MouseEvent) {
        let hit = self.chain_form_hits.borrow().clone();
        let col = mouse.column;
        let row = mouse.row;

        //
        // Top-bar buttons.
        //
        if hit.save_button.contains(col, row) {
            self.submit_chain_form().await;
            return;
        }
        if hit.cancel_button.contains(col, row) {
            self.chain_form = None;
            return;
        }
        if hit.auto_layout_button.contains(col, row) {
            if let Some(form) = self.chain_form.as_mut() {
                form.positions.clear();
                auto_layout(form);
                form.camera_x = 0;
                form.camera_y = 0;
            }
            return;
        }

        //
        // Palette buttons: click [+ Kind] to drop a new block.
        //
        for (kind, rect) in &hit.palette_buttons {
            if rect.contains(col, row) {
                self.add_element_at_centre(*kind);
                return;
            }
        }

        //
        // Header field cells.
        //
        for (target, rect) in &hit.header_fields {
            if rect.contains(col, row) {
                if let Some(form) = self.chain_form.as_mut() {
                    form.editing = Some(target.clone());
                }
                return;
            }
        }

        //
        // Property field cells.
        //
        for (target, rect) in &hit.property_fields {
            if rect.contains(col, row) {
                if let Some(form) = self.chain_form.as_mut() {
                    form.editing = Some(target.clone());
                }
                return;
            }
        }
        if hit.kind_cycle_button.contains(col, row) {
            self.cycle_selected_kind();
            return;
        }
        if hit.delete_element_button.contains(col, row) {
            if let Some(form) = self.chain_form.as_mut() {
                delete_selection(form);
            }
            return;
        }
        if hit.cycle_condition_button.contains(col, row) {
            if let Some(form) = self.chain_form.as_mut() {
                if let Selected::Connection(idx) = form.selected.clone() {
                    if let Some(conn) = form.connections.get_mut(idx) {
                        conn.condition = cycle_condition(conn.condition, 1);
                    }
                }
            }
            return;
        }
        if hit.delete_connection_button.contains(col, row) {
            if let Some(form) = self.chain_form.as_mut() {
                if let Selected::Connection(idx) = form.selected.clone() {
                    if idx < form.connections.len() {
                        form.connections.remove(idx);
                        form.selected = Selected::None;
                    }
                }
            }
            return;
        }
        if hit.pick_op_button.contains(col, row) {
            if let Some(form) = self.chain_form.as_mut() {
                form.editor = Some(ChainFormEditor::PickOpName {
                    cursor: 0,
                    filter: String::new(),
                });
            }
            return;
        }

        //
        // Canvas hit-test. Order: ports first (small targets), then block
        // bodies, then connection segments, then empty space (pan start).
        //
        if hit.canvas.contains(col, row) {
            //
            // Translate viewport pixel to canvas cell.
            //
            let canvas_col = (col as i32 - hit.canvas.x as i32) + self.chain_form_camera_x();
            let canvas_row = (row as i32 - hit.canvas.y as i32) + self.chain_form_camera_y();

            //
            // Ports.
            //
            let port_hit = self.port_at(canvas_col, canvas_row);
            if let Some((id, port, side)) = port_hit {
                if side == PortSide::From {
                    if let Some(form) = self.chain_form.as_mut() {
                        form.drag = Drag::Port {
                            from_id: id,
                            from_port: port,
                            cursor_col: col,
                            cursor_row: row,
                        };
                        form.editing = None;
                    }
                    return;
                }
                //
                // Clicking an input port without dragging does nothing
                // special — fall through to select-block.
                //
            }

            //
            // Block body.
            //
            if let Some(id) = self.block_at(canvas_col, canvas_row) {
                let pos = self
                    .chain_form
                    .as_ref()
                    .map(|f| f.block_pos(&id))
                    .unwrap_or((0, 0));
                let grab_dx = canvas_col - pos.0;
                let grab_dy = canvas_row - pos.1;
                if let Some(form) = self.chain_form.as_mut() {
                    form.selected = Selected::Block(id.clone());
                    form.drag = Drag::Block { id, grab_dx, grab_dy };
                    form.editing = None;
                }
                return;
            }

            //
            // Connection segment.
            //
            if let Some(idx) = self.connection_at(canvas_col, canvas_row) {
                if let Some(form) = self.chain_form.as_mut() {
                    form.selected = Selected::Connection(idx);
                    form.editing = None;
                }
                return;
            }

            //
            // Empty canvas → start panning.
            //
            if let Some(form) = self.chain_form.as_mut() {
                form.drag = Drag::Canvas {
                    last_col: col,
                    last_row: row,
                };
                form.selected = Selected::None;
                form.editing = None;
            }
        }
    }

    fn canvas_mouse_drag(&mut self, mouse: MouseEvent) {
        let col = mouse.column;
        let row = mouse.row;
        let hit = self.chain_form_hits.borrow().clone();
        let Some(form) = self.chain_form.as_mut() else {
            return;
        };
        match form.drag.clone() {
            Drag::Block { id, grab_dx, grab_dy } => {
                if !hit.canvas.is_empty() {
                    let canvas_col = (col as i32 - hit.canvas.x as i32) + form.camera_x;
                    let canvas_row = (row as i32 - hit.canvas.y as i32) + form.camera_y;
                    let new_x = canvas_col - grab_dx;
                    let new_y = canvas_row - grab_dy;
                    form.positions
                        .insert(id, (new_x.max(0), new_y.max(0)));
                }
            }
            Drag::Canvas { last_col, last_row } => {
                let dx = last_col as i32 - col as i32;
                let dy = last_row as i32 - row as i32;
                form.camera_x += dx;
                form.camera_y += dy;
                form.drag = Drag::Canvas {
                    last_col: col,
                    last_row: row,
                };
            }
            Drag::Port {
                from_id,
                from_port,
                ..
            } => {
                form.drag = Drag::Port {
                    from_id,
                    from_port,
                    cursor_col: col,
                    cursor_row: row,
                };
            }
            Drag::None => {}
        }
    }

    fn canvas_mouse_up(&mut self, mouse: MouseEvent) {
        let hit = self.chain_form_hits.borrow().clone();
        let release = {
            let Some(form) = self.chain_form.as_ref() else {
                return;
            };
            form.drag.clone()
        };

        if let Drag::Port {
            from_id, from_port, ..
        } = release
        {
            //
            // On release, hit-test the cursor against input ports. If a
            // hit, create the connection.
            //
            let canvas_col = (mouse.column as i32 - hit.canvas.x as i32) + self.chain_form_camera_x();
            let canvas_row = (mouse.row as i32 - hit.canvas.y as i32) + self.chain_form_camera_y();
            if let Some((to_id, to_port, side)) = self.port_at(canvas_col, canvas_row) {
                if side == PortSide::To && to_id != from_id {
                    if let Some(form) = self.chain_form.as_mut() {
                        let next_id =
                            format!("c_{}", form.connections.len() + 1 + form.element_id_seq as usize);
                        form.connections.push(ConnectionDraft {
                            id: next_id,
                            from_element: from_id,
                            to_element: to_id,
                            from_port,
                            to_port,
                            condition: ConditionKind::None,
                        });
                        form.selected = Selected::Connection(form.connections.len() - 1);
                    }
                }
            }
        }

        if let Some(form) = self.chain_form.as_mut() {
            form.drag = Drag::None;
        }
    }

    fn chain_form_camera_x(&self) -> i32 {
        self.chain_form.as_ref().map(|f| f.camera_x).unwrap_or(0)
    }

    fn chain_form_camera_y(&self) -> i32 {
        self.chain_form.as_ref().map(|f| f.camera_y).unwrap_or(0)
    }

    //
    // Returns the block id at the given canvas coordinate, if any. Iterates
    // back-to-front so the topmost (last-added) block wins on overlap.
    //

    fn block_at(&self, canvas_col: i32, canvas_row: i32) -> Option<String> {
        let form = self.chain_form.as_ref()?;
        for el in form.elements.iter().rev() {
            let (x, y) = form.block_pos(&el.id);
            if canvas_col >= x
                && canvas_col < x + BLOCK_W as i32
                && canvas_row >= y
                && canvas_row < y + BLOCK_H as i32
            {
                return Some(el.id.clone());
            }
        }
        None
    }

    fn port_at(&self, canvas_col: i32, canvas_row: i32) -> Option<(String, u32, PortSide)> {
        let form = self.chain_form.as_ref()?;
        for el in &form.elements {
            let (x, y) = form.block_pos(&el.id);
            //
            // Input port (left edge), one per block.
            //
            if input_port_count(el.kind) > 0 {
                let port_x = x - 1;
                let port_y = y + (BLOCK_H as i32 / 2);
                if canvas_col == port_x && canvas_row == port_y {
                    return Some((el.id.clone(), 0, PortSide::To));
                }
            }
            //
            // Output ports (right edge).
            //
            let n = output_port_count(el.kind);
            for port in 0..n {
                let port_x = x + BLOCK_W as i32;
                let port_y = y + 1 + port as i32;
                if canvas_col == port_x && canvas_row == port_y {
                    return Some((el.id.clone(), port, PortSide::From));
                }
            }
        }
        None
    }

    fn connection_at(&self, canvas_col: i32, canvas_row: i32) -> Option<usize> {
        let form = self.chain_form.as_ref()?;
        //
        // Build the same routing the renderer uses and check whether the
        // cursor cell is on the path.
        //
        for (idx, conn) in form.connections.iter().enumerate() {
            let path = crate::ui::chain_form::route_connection(form, conn);
            for (x, y) in path.iter() {
                if *x == canvas_col && *y == canvas_row {
                    return Some(idx);
                }
            }
        }
        None
    }

    fn cycle_selected_kind(&mut self) {
        let Some(form) = self.chain_form.as_mut() else {
            return;
        };
        let Selected::Block(ref id) = form.selected.clone() else {
            return;
        };
        if let Some(el) = form.elements.iter_mut().find(|e| &e.id == id) {
            let cur = ElementKind::ALL
                .iter()
                .position(|k| *k == el.kind)
                .unwrap_or(0);
            el.kind = ElementKind::ALL[(cur + 1) % ElementKind::ALL.len()];
        }
    }
}

//
// Auto-layout for elements that don't have a stored position. Walks the
// graph from triggers outward in BFS order, placing each row of the BFS
// in a new column to produce a left-to-right flow.
//

pub fn auto_layout(form: &mut ChainForm) {
    let mut adj: HashMap<String, Vec<String>> = HashMap::new();
    for c in &form.connections {
        adj.entry(c.from_element.clone())
            .or_default()
            .push(c.to_element.clone());
    }
    let mut visited: HashSet<String> = HashSet::new();
    let mut col_of: HashMap<String, i32> = HashMap::new();
    let mut row_of: HashMap<String, i32> = HashMap::new();
    let mut col_count: HashMap<i32, i32> = HashMap::new();

    let mut queue: Vec<(String, i32)> = Vec::new();
    for el in &form.elements {
        if el.kind == ElementKind::Trigger {
            queue.push((el.id.clone(), 0));
        }
    }
    while let Some((id, col)) = queue.pop() {
        if visited.contains(&id) {
            continue;
        }
        visited.insert(id.clone());
        let row = *col_count.entry(col).and_modify(|v| *v += 1).or_insert(0);
        col_of.insert(id.clone(), col);
        row_of.insert(id.clone(), row);
        if let Some(targets) = adj.get(&id) {
            for t in targets {
                queue.push((t.clone(), col + 1));
            }
        }
    }
    //
    // Any leftover elements (disconnected) go in a far-right column so
    // they're still visible.
    //
    for el in &form.elements {
        if !visited.contains(&el.id) {
            let col = col_count.len() as i32 + 1;
            let row = *col_count.entry(col).and_modify(|v| *v += 1).or_insert(0);
            col_of.insert(el.id.clone(), col);
            row_of.insert(el.id.clone(), row);
        }
    }

    for el in &form.elements {
        let col = *col_of.get(&el.id).unwrap_or(&0);
        let row = *row_of.get(&el.id).unwrap_or(&0);
        let x = CANVAS_ORIGIN_X + col * LAYOUT_COL_GAP;
        let y = CANVAS_ORIGIN_Y + row * LAYOUT_ROW_GAP;
        form.positions.insert(el.id.clone(), (x, y));
    }
}

//
// Find the next free canvas position when adding a new block: place it at
// the centre-bottom of the existing graph plus a row of clearance.
//

pub fn next_free_position(form: &ChainForm) -> (i32, i32) {
    if form.positions.is_empty() {
        return (CANVAS_ORIGIN_X, CANVAS_ORIGIN_Y);
    }
    let max_y = form
        .positions
        .values()
        .map(|(_, y)| *y)
        .max()
        .unwrap_or(CANVAS_ORIGIN_Y);
    (CANVAS_ORIGIN_X, max_y + LAYOUT_ROW_GAP)
}

//
// Per-kind port counts. Trigger has no input; Termination has no outputs.
// Loop exposes two output ports (retry/exit). All others have one output.
//

pub fn input_port_count(kind: ElementKind) -> u32 {
    match kind {
        ElementKind::Trigger => 0,
        _ => 1,
    }
}

pub fn output_port_count(kind: ElementKind) -> u32 {
    match kind {
        ElementKind::Termination => 0,
        ElementKind::Loop => 2,
        _ => 1,
    }
}

//
// Inline text edit. Backspace pops; printable chars push; Enter commits
// and leaves the field. Type-specific filters: timeout / max_iterations
// reject non-digits.
//

fn handle_edit_key(form: &mut ChainForm, key: KeyEvent) {
    let Some(target) = form.editing.clone() else {
        return;
    };
    match key.code {
        KeyCode::Enter | KeyCode::Tab => {
            form.editing = None;
            return;
        }
        KeyCode::Backspace => match target {
            EditTarget::HeaderName => {
                form.name.pop();
            }
            EditTarget::HeaderCategory => {
                form.category.pop();
            }
            EditTarget::HeaderTimeout => {
                form.timeout.pop();
            }
            EditTarget::HeaderDescription => {
                form.description.pop();
            }
            EditTarget::BlockProp { ref id, field } => {
                if let Some(el) = form.elements.iter_mut().find(|e| &e.id == id) {
                    block_field_mut(el, field).pop();
                }
            }
            EditTarget::ConnectionPort { idx, side } => {
                if let Some(c) = form.connections.get_mut(idx) {
                    match side {
                        PortSide::From => {
                            let mut s = c.from_port.to_string();
                            s.pop();
                            c.from_port = s.parse().unwrap_or(0);
                        }
                        PortSide::To => {
                            let mut s = c.to_port.to_string();
                            s.pop();
                            c.to_port = s.parse().unwrap_or(0);
                        }
                    }
                }
            }
        },
        KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => match target {
            EditTarget::HeaderName => form.name.push(c),
            EditTarget::HeaderCategory => form.category.push(c),
            EditTarget::HeaderTimeout => {
                if c.is_ascii_digit() {
                    form.timeout.push(c);
                }
            }
            EditTarget::HeaderDescription => form.description.push(c),
            EditTarget::BlockProp {
                ref id,
                field: BlockField::MaxIterations,
            } => {
                if c.is_ascii_digit() {
                    if let Some(el) = form.elements.iter_mut().find(|e| &e.id == id) {
                        el.max_iterations.push(c);
                    }
                }
            }
            EditTarget::BlockProp { ref id, field } => {
                if let Some(el) = form.elements.iter_mut().find(|e| &e.id == id) {
                    block_field_mut(el, field).push(c);
                }
            }
            EditTarget::ConnectionPort { idx, side } => {
                if c.is_ascii_digit() {
                    if let Some(c_ref) = form.connections.get_mut(idx) {
                        match side {
                            PortSide::From => {
                                let mut s = c_ref.from_port.to_string();
                                s.push(c);
                                c_ref.from_port = s.parse().unwrap_or(0);
                            }
                            PortSide::To => {
                                let mut s = c_ref.to_port.to_string();
                                s.push(c);
                                c_ref.to_port = s.parse().unwrap_or(0);
                            }
                        }
                    }
                }
            }
        },
        _ => {}
    }
}

fn block_field_mut(el: &mut ChainElementDraft, field: BlockField) -> &mut String {
    match field {
        BlockField::OpName => &mut el.op_name,
        BlockField::ModelRef => &mut el.model_ref,
        BlockField::Prompt => &mut el.prompt,
        BlockField::MemoryKey => &mut el.memory_key,
        BlockField::MaxIterations => &mut el.max_iterations,
        BlockField::ToolName => &mut el.tool_name,
        BlockField::ToolParams => &mut el.tool_params,
        BlockField::PayloadId => &mut el.payload_id,
    }
}

//
// Remove the current selection — a block (with its incident connections)
// or a connection.
//

pub fn delete_selection(form: &mut ChainForm) {
    match form.selected.clone() {
        Selected::Block(id) => {
            //
            // Refuse to delete the last Trigger or Termination — that
            // would invalidate the chain.
            //
            let kind = form
                .elements
                .iter()
                .find(|e| e.id == id)
                .map(|e| e.kind);
            form.elements.retain(|e| e.id != id);
            form.connections
                .retain(|c| c.from_element != id && c.to_element != id);
            form.positions.remove(&id);
            form.selected = Selected::None;
            //
            // No-op refusal would just leave the user with no recourse;
            // we let them delete and re-add via the palette if needed.
            //
            let _ = kind;
        }
        Selected::Connection(idx) => {
            if idx < form.connections.len() {
                form.connections.remove(idx);
            }
            form.selected = Selected::None;
        }
        Selected::None => {}
    }
}

fn cycle_condition(c: ConditionKind, delta: i32) -> ConditionKind {
    let list = [
        ConditionKind::None,
        ConditionKind::OnSuccess,
        ConditionKind::OnFailure,
    ];
    let cur = list.iter().position(|x| *x == c).unwrap_or(0);
    let len = list.len() as i32;
    let next = ((cur as i32 + delta).rem_euclid(len)) as usize;
    list[next]
}

//
// Draft <-> ChainElement conversion (same as the previous implementation).
//

fn draft_to_element(d: &ChainElementDraft) -> ChainElement {
    match d.kind {
        ElementKind::Trigger => ChainElement::Trigger {
            id: d.id.clone(),
            trigger_type: ChainTriggerType::Manual,
        },
        ElementKind::Operation => ChainElement::Operation {
            id: d.id.clone(),
            operation_name: d.op_name.clone(),
            model_ref: empty_to_none(&d.model_ref),
            session_group: None,
            block_config: None,
        },
        ElementKind::Transform => ChainElement::Transform {
            id: d.id.clone(),
            prompt: d.prompt.clone(),
            model_ref: empty_to_none(&d.model_ref),
            session_group: None,
            block_config: None,
        },
        ElementKind::GenericPrompt => ChainElement::GenericPrompt {
            id: d.id.clone(),
            prompt: d.prompt.clone(),
            session_group: None,
            block_config: None,
        },
        ElementKind::Memory => ChainElement::Memory {
            id: d.id.clone(),
            key: d.memory_key.clone(),
            mode: if d.memory_mode == 0 {
                MemoryMode::Store
            } else {
                MemoryMode::Retrieve
            },
        },
        ElementKind::Loop => ChainElement::Loop {
            id: d.id.clone(),
            max_iterations: d.max_iterations.parse().unwrap_or(10),
        },
        ElementKind::Tool => {
            let params = serde_json::from_str(&d.tool_params)
                .unwrap_or(serde_json::Value::Object(Default::default()));
            ChainElement::Tool {
                id: d.id.clone(),
                tool_name: d.tool_name.clone(),
                tool_params: params,
                block_config: None,
            }
        }
        ElementKind::Payload => ChainElement::Payload {
            id: d.id.clone(),
            payload_id: d.payload_id.clone(),
            block_config: None,
        },
        ElementKind::Termination => ChainElement::Termination {
            id: d.id.clone(),
            block_config: None,
        },
    }
}

fn element_to_draft(el: &ChainElement) -> ChainElementDraft {
    match el {
        ChainElement::Trigger { id, .. } => ChainElementDraft::new(id.clone(), ElementKind::Trigger),
        ChainElement::Operation {
            id,
            operation_name,
            model_ref,
            ..
        } => {
            let mut d = ChainElementDraft::new(id.clone(), ElementKind::Operation);
            d.op_name = operation_name.clone();
            d.model_ref = model_ref.clone().unwrap_or_default();
            d
        }
        ChainElement::Transform {
            id,
            prompt,
            model_ref,
            ..
        } => {
            let mut d = ChainElementDraft::new(id.clone(), ElementKind::Transform);
            d.prompt = prompt.clone();
            d.model_ref = model_ref.clone().unwrap_or_default();
            d
        }
        ChainElement::GenericPrompt { id, prompt, .. } => {
            let mut d = ChainElementDraft::new(id.clone(), ElementKind::GenericPrompt);
            d.prompt = prompt.clone();
            d
        }
        ChainElement::Memory { id, key, mode } => {
            let mut d = ChainElementDraft::new(id.clone(), ElementKind::Memory);
            d.memory_key = key.clone();
            d.memory_mode = match mode {
                MemoryMode::Store => 0,
                MemoryMode::Retrieve => 1,
            };
            d
        }
        ChainElement::Loop { id, max_iterations } => {
            let mut d = ChainElementDraft::new(id.clone(), ElementKind::Loop);
            d.max_iterations = max_iterations.to_string();
            d
        }
        ChainElement::Tool {
            id,
            tool_name,
            tool_params,
            ..
        } => {
            let mut d = ChainElementDraft::new(id.clone(), ElementKind::Tool);
            d.tool_name = tool_name.clone();
            d.tool_params = serde_json::to_string(tool_params).unwrap_or_else(|_| "{}".to_string());
            d
        }
        ChainElement::Payload { id, payload_id, .. } => {
            let mut d = ChainElementDraft::new(id.clone(), ElementKind::Payload);
            d.payload_id = payload_id.clone();
            d
        }
        ChainElement::Termination { id, .. } => {
            ChainElementDraft::new(id.clone(), ElementKind::Termination)
        }
    }
}

fn empty_to_none(s: &str) -> Option<String> {
    if s.trim().is_empty() {
        None
    } else {
        Some(s.trim().to_string())
    }
}
