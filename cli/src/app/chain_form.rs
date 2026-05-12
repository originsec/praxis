use super::*;
use common::{
    ChainConnection, ChainDefinitionInput, ChainElement, ChainTriggerType, ConnectionCondition,
    MemoryMode,
};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::Rect;

impl App {
    //
    // Open a fresh chain form for creating a new chain. Populates the
    // available op names list from the current library snapshot so the
    // operation picker overlay has something to show.
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

        //
        // Seed every new chain with a Trigger and a Termination so the
        // graph is valid on day one. Connect them as well so the user has
        // a working scaffold to extend.
        //
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
            from_element: trig_id,
            to_element: term_id,
            from_port: 0,
            to_port: 0,
            condition: ConditionKind::None,
        });

        self.chain_form = Some(form);
    }

    //
    // Open the form pre-populated from an existing chain definition so the
    // user can edit it. Requires the full ChainDefinitionFull which we
    // request from the service via ChainGet; the response handler will
    // call this once `current_chain` is populated.
    //

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

        self.chain_form = Some(form);
    }

    //
    // Edit the currently selected library row if it's a chain. Triggers a
    // ChainGet request and stashes the chain id so handle_app_event picks
    // up the response and opens the form.
    //

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
            //
            // Poll for the response — service typically replies within a
            // few tens of ms but the round-trip goes through RabbitMQ so
            // we give it up to ~1.5 seconds before giving up.
            //
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
    // Submit the form: validate, convert drafts to ChainDefinitionInput,
    // and dispatch ChainCreate or ChainUpdate. Validation errors are
    // surfaced via `form.error` so the modal can render them inline.
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

        let elements: Vec<ChainElement> = form
            .elements
            .iter()
            .map(draft_to_element)
            .collect();
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

        let definition = ChainDefinitionInput {
            name: form.name.trim().to_string(),
            description: form.description.clone(),
            category: form.category.trim().to_string(),
            elements,
            connections,
            disabled: false,
            timeout,
            positions: std::collections::HashMap::new(),
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
    // Top-level key dispatch when the chain form is open. Delegates to
    // sub-editors (kind picker, op picker, connection editor) when one is
    // active; otherwise routes by focused section.
    //

    pub(crate) async fn handle_chain_form_key(&mut self, key: KeyEvent) {
        //
        // Sub-editor overlays consume input first.
        //
        if self.chain_form.as_ref().and_then(|f| f.editor.as_ref()).is_some() {
            self.handle_chain_editor_key(key).await;
            return;
        }

        match key.code {
            KeyCode::Esc => {
                self.chain_form = None;
                return;
            }
            KeyCode::Char('s') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.submit_chain_form().await;
                return;
            }
            KeyCode::Tab => {
                if let Some(form) = self.chain_form.as_mut() {
                    form.section = next_section(form.section);
                    return;
                }
            }
            KeyCode::BackTab => {
                if let Some(form) = self.chain_form.as_mut() {
                    form.section = prev_section(form.section);
                    return;
                }
            }
            _ => {}
        }

        let section = match self.chain_form.as_ref() {
            Some(f) => f.section,
            None => return,
        };

        match section {
            ChainFormSection::Header => self.handle_chain_header_key(key),
            ChainFormSection::Elements => self.handle_chain_elements_key(key).await,
            ChainFormSection::Properties => self.handle_chain_properties_key(key),
            ChainFormSection::Connections => self.handle_chain_connections_key(key).await,
            ChainFormSection::Buttons => self.handle_chain_buttons_key(key).await,
        }
    }

    fn handle_chain_header_key(&mut self, key: KeyEvent) {
        let Some(form) = self.chain_form.as_mut() else {
            return;
        };
        match key.code {
            KeyCode::Up => {
                if form.focused_header_field > 0 {
                    form.focused_header_field -= 1;
                }
            }
            KeyCode::Down => {
                if form.focused_header_field < 3 {
                    form.focused_header_field += 1;
                }
            }
            KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                push_header_char(form, c);
            }
            KeyCode::Backspace => {
                pop_header_char(form);
            }
            _ => {}
        }
    }

    async fn handle_chain_elements_key(&mut self, key: KeyEvent) {
        let Some(form) = self.chain_form.as_mut() else {
            return;
        };
        match key.code {
            KeyCode::Up => {
                if form.element_selected > 0 {
                    form.element_selected -= 1;
                }
            }
            KeyCode::Down => {
                if form.element_selected + 1 < form.elements.len() {
                    form.element_selected += 1;
                }
            }
            KeyCode::Char('a') | KeyCode::Char('+') | KeyCode::Char('n') => {
                form.editor = Some(ChainFormEditor::PickElementKind { cursor: 1 });
            }
            KeyCode::Delete | KeyCode::Char('d') => {
                delete_selected_element(form);
            }
            KeyCode::Enter => {
                form.section = ChainFormSection::Properties;
                form.focused_prop_field = 0;
            }
            _ => {}
        }
    }

    fn handle_chain_properties_key(&mut self, key: KeyEvent) {
        let Some(form) = self.chain_form.as_mut() else {
            return;
        };
        let field_count = property_field_count(form);
        match key.code {
            KeyCode::Up => {
                if form.focused_prop_field > 0 {
                    form.focused_prop_field -= 1;
                }
            }
            KeyCode::Down => {
                if (form.focused_prop_field as usize + 1) < field_count {
                    form.focused_prop_field += 1;
                }
            }
            KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                push_prop_char(form, c);
            }
            KeyCode::Backspace => {
                pop_prop_char(form);
            }
            KeyCode::Left | KeyCode::Right => {
                toggle_prop_field(form, matches!(key.code, KeyCode::Right));
            }
            _ => {}
        }
    }

    async fn handle_chain_connections_key(&mut self, key: KeyEvent) {
        let Some(form) = self.chain_form.as_mut() else {
            return;
        };
        match key.code {
            KeyCode::Up => {
                if form.connection_selected > 0 {
                    form.connection_selected -= 1;
                }
            }
            KeyCode::Down => {
                if form.connection_selected + 1 < form.connections.len() {
                    form.connection_selected += 1;
                }
            }
            KeyCode::Char('a') | KeyCode::Char('+') | KeyCode::Char('n') => {
                if form.elements.len() < 2 {
                    form.error = Some("Need at least two elements first".to_string());
                    return;
                }
                form.editor = Some(ChainFormEditor::EditConnection {
                    editing_idx: None,
                    from_idx: 0,
                    to_idx: form.elements.len().min(2) - 1,
                    from_port: "0".to_string(),
                    to_port: "0".to_string(),
                    condition: ConditionKind::None,
                    focus: 0,
                });
            }
            KeyCode::Char('e') => {
                open_connection_editor_for_selected(form);
            }
            KeyCode::Delete | KeyCode::Char('d') => {
                if form.connection_selected < form.connections.len() {
                    form.connections.remove(form.connection_selected);
                    if form.connection_selected >= form.connections.len()
                        && form.connection_selected > 0
                    {
                        form.connection_selected -= 1;
                    }
                }
            }
            _ => {}
        }
    }

    //
    // Mouse handling. Reads the hit map stashed by the renderer and maps
    // clicks to actions. Left-click also acts as "submit" on the various
    // [+ Add ...] / [Save] / [Cancel] buttons.
    //

    pub(crate) async fn handle_chain_form_mouse(&mut self, mouse: MouseEvent) {
        let MouseEventKind::Down(MouseButton::Left) = mouse.kind else {
            return;
        };
        let col = mouse.column;
        let row = mouse.row;

        //
        // Overlay editors swallow clicks while open. Clicking outside the
        // overlay closes it.
        //
        if self.chain_form.as_ref().and_then(|f| f.editor.as_ref()).is_some() {
            self.handle_chain_editor_mouse(col, row);
            return;
        }

        //
        // Snapshot the hit map and release the borrow before mutating
        // self further — many of the branches below call into other
        // methods that take &mut self.
        //
        let hit = self.chain_form_hits.borrow().clone();

        if hit_contains(&hit.save_button, col, row) {
            self.submit_chain_form().await;
            return;
        }
        if hit_contains(&hit.cancel_button, col, row) {
            self.chain_form = None;
            return;
        }
        if hit_contains(&hit.add_element_button, col, row) {
            if let Some(form) = self.chain_form.as_mut() {
                form.section = ChainFormSection::Elements;
                form.editor = Some(ChainFormEditor::PickElementKind { cursor: 1 });
            }
            return;
        }
        if hit_contains(&hit.add_connection_button, col, row) {
            if let Some(form) = self.chain_form.as_mut() {
                if form.elements.len() < 2 {
                    form.error = Some("Need at least two elements first".to_string());
                    return;
                }
                form.section = ChainFormSection::Connections;
                form.editor = Some(ChainFormEditor::EditConnection {
                    editing_idx: None,
                    from_idx: 0,
                    to_idx: form.elements.len().min(2) - 1,
                    from_port: "0".to_string(),
                    to_port: "0".to_string(),
                    condition: ConditionKind::None,
                    focus: 0,
                });
            }
            return;
        }
        if hit_contains(&hit.delete_element_button, col, row) {
            if let Some(form) = self.chain_form.as_mut() {
                delete_selected_element(form);
            }
            return;
        }

        //
        // Header fields: click to focus the field and switch section.
        //
        for (idx, rect) in &hit.header_fields {
            if hit_contains(rect, col, row) {
                if let Some(form) = self.chain_form.as_mut() {
                    form.section = ChainFormSection::Header;
                    form.focused_header_field = *idx;
                }
                return;
            }
        }

        //
        // Element list rows.
        //
        for (idx, rect) in &hit.element_rows {
            if hit_contains(rect, col, row) {
                if let Some(form) = self.chain_form.as_mut() {
                    form.section = ChainFormSection::Elements;
                    form.element_selected = *idx;
                }
                return;
            }
        }

        //
        // Property rows.
        //
        for (idx, rect) in &hit.property_rows {
            if hit_contains(rect, col, row) {
                if let Some(form) = self.chain_form.as_mut() {
                    form.section = ChainFormSection::Properties;
                    form.focused_prop_field = *idx;
                    //
                    // Clicking the Operation row opens the op-name picker
                    // directly since there's no other left/right interaction
                    // most users will discover via mouse.
                    //
                    if *idx >= 1
                        && form.selected_element().map(|e| e.kind)
                            == Some(ElementKind::Operation)
                        && *idx == 1
                    {
                        form.editor = Some(ChainFormEditor::PickOpName {
                            cursor: 0,
                            filter: String::new(),
                        });
                    } else if *idx == 0 {
                        //
                        // Clicking the Kind row cycles forward.
                        //
                        cycle_element_kind(form, true);
                    } else if form.selected_element().map(|e| e.kind)
                        == Some(ElementKind::Memory)
                        && *idx == 2
                    {
                        if let Some(el) = form.selected_element_mut() {
                            el.memory_mode = (el.memory_mode + 1) % 2;
                        }
                    }
                }
                return;
            }
        }

        //
        // Connection rows.
        //
        for (idx, rect) in &hit.connection_rows {
            if hit_contains(rect, col, row) {
                if let Some(form) = self.chain_form.as_mut() {
                    form.section = ChainFormSection::Connections;
                    form.connection_selected = *idx;
                    //
                    // Double-click would normally open the editor; for a
                    // mouse-driven UX we instead open it on a single click
                    // so users can revise the connection without keyboard.
                    //
                    open_connection_editor_for_selected(form);
                }
                return;
            }
        }

        //
        // Section panel clicks (background area, not a row): just switch
        // section focus.
        //
        if hit_contains(&hit.elements_panel, col, row) {
            if let Some(form) = self.chain_form.as_mut() {
                form.section = ChainFormSection::Elements;
            }
            return;
        }
        if hit_contains(&hit.properties_panel, col, row) {
            if let Some(form) = self.chain_form.as_mut() {
                form.section = ChainFormSection::Properties;
            }
            return;
        }
        if hit_contains(&hit.connections_panel, col, row) {
            if let Some(form) = self.chain_form.as_mut() {
                form.section = ChainFormSection::Connections;
            }
            return;
        }
    }

    fn handle_chain_editor_mouse(&mut self, _col: u16, _row: u16) {
        //
        // Overlay editors are keyboard-only for now to keep complexity
        // bounded. Any click while an overlay is open is treated as a
        // close request so a user clicking outside doesn't get stuck.
        //
        if let Some(form) = self.chain_form.as_mut() {
            form.editor = None;
        }
    }

    async fn handle_chain_buttons_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Enter => {
                self.submit_chain_form().await;
            }
            KeyCode::Char(' ') => {
                self.submit_chain_form().await;
            }
            _ => {}
        }
    }

    //
    // Editor overlays.
    //

    async fn handle_chain_editor_key(&mut self, key: KeyEvent) {
        let close = matches!(key.code, KeyCode::Esc);
        if close {
            if let Some(form) = self.chain_form.as_mut() {
                form.editor = None;
            }
            return;
        }

        let Some(form) = self.chain_form.as_mut() else {
            return;
        };
        let editor = match form.editor.take() {
            Some(e) => e,
            None => return,
        };

        match editor {
            ChainFormEditor::PickElementKind { mut cursor } => {
                match key.code {
                    KeyCode::Up => {
                        if cursor > 0 {
                            cursor -= 1;
                        }
                    }
                    KeyCode::Down => {
                        if cursor + 1 < ElementKind::ALL.len() {
                            cursor += 1;
                        }
                    }
                    KeyCode::Enter => {
                        let kind = ElementKind::ALL[cursor];
                        let id = form.next_element_id(kind);
                        let draft = ChainElementDraft::new(id, kind);
                        form.elements.push(draft);
                        form.element_selected = form.elements.len() - 1;
                        form.section = ChainFormSection::Properties;
                        form.focused_prop_field = 0;
                        return;
                    }
                    _ => {}
                }
                form.editor = Some(ChainFormEditor::PickElementKind { cursor });
            }
            ChainFormEditor::EditConnection {
                editing_idx,
                mut from_idx,
                mut to_idx,
                mut from_port,
                mut to_port,
                mut condition,
                mut focus,
            } => {
                let total = form.elements.len().max(1);
                match key.code {
                    KeyCode::Tab => focus = (focus + 1) % 7,
                    KeyCode::BackTab => focus = (focus + 6) % 7,
                    KeyCode::Up => {
                        if focus > 0 {
                            focus -= 1;
                        }
                    }
                    KeyCode::Down => {
                        if focus < 6 {
                            focus += 1;
                        }
                    }
                    KeyCode::Left | KeyCode::Right => {
                        let delta: i32 = if matches!(key.code, KeyCode::Right) { 1 } else { -1 };
                        match focus {
                            0 => {
                                from_idx = ((from_idx as i32 + delta).rem_euclid(total as i32))
                                    as usize;
                            }
                            1 => {
                                to_idx =
                                    ((to_idx as i32 + delta).rem_euclid(total as i32)) as usize;
                            }
                            2 => {
                                let cur: i32 = from_port.parse().unwrap_or(0);
                                from_port = (cur + delta).max(0).to_string();
                            }
                            3 => {
                                let cur: i32 = to_port.parse().unwrap_or(0);
                                to_port = (cur + delta).max(0).to_string();
                            }
                            4 => {
                                condition = cycle_condition(condition, delta);
                            }
                            _ => {}
                        }
                    }
                    KeyCode::Enter => {
                        if focus == 5 {
                            commit_connection_editor(
                                form, editing_idx, from_idx, to_idx, &from_port, &to_port,
                                condition,
                            );
                            return;
                        } else if focus == 6 {
                            return;
                        } else {
                            focus = (focus + 1) % 7;
                        }
                    }
                    _ => {}
                }
                form.editor = Some(ChainFormEditor::EditConnection {
                    editing_idx,
                    from_idx,
                    to_idx,
                    from_port,
                    to_port,
                    condition,
                    focus,
                });
            }
            ChainFormEditor::PickOpName { mut cursor, mut filter } => {
                let filtered: Vec<String> = form
                    .available_op_names
                    .iter()
                    .filter(|n| {
                        filter.is_empty() || n.to_lowercase().contains(&filter.to_lowercase())
                    })
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
                            if let Some(el) = form.elements.get_mut(form.element_selected) {
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
        }
    }
}

//
// Helpers.
//

pub(crate) fn next_section(s: ChainFormSection) -> ChainFormSection {
    match s {
        ChainFormSection::Header => ChainFormSection::Elements,
        ChainFormSection::Elements => ChainFormSection::Properties,
        ChainFormSection::Properties => ChainFormSection::Connections,
        ChainFormSection::Connections => ChainFormSection::Buttons,
        ChainFormSection::Buttons => ChainFormSection::Header,
    }
}

pub(crate) fn prev_section(s: ChainFormSection) -> ChainFormSection {
    match s {
        ChainFormSection::Header => ChainFormSection::Buttons,
        ChainFormSection::Elements => ChainFormSection::Header,
        ChainFormSection::Properties => ChainFormSection::Elements,
        ChainFormSection::Connections => ChainFormSection::Properties,
        ChainFormSection::Buttons => ChainFormSection::Connections,
    }
}

fn push_header_char(form: &mut ChainForm, c: char) {
    match form.focused_header_field {
        0 => form.name.push(c),
        1 => form.category.push(c),
        2 => {
            if c.is_ascii_digit() {
                form.timeout.push(c);
            }
        }
        3 => form.description.push(c),
        _ => {}
    }
}

fn pop_header_char(form: &mut ChainForm) {
    match form.focused_header_field {
        0 => {
            form.name.pop();
        }
        1 => {
            form.category.pop();
        }
        2 => {
            form.timeout.pop();
        }
        3 => {
            form.description.pop();
        }
        _ => {}
    }
}

fn delete_selected_element(form: &mut ChainForm) {
    if form.elements.is_empty() {
        return;
    }
    let removed_id = form.elements[form.element_selected].id.clone();
    form.elements.remove(form.element_selected);
    if form.element_selected >= form.elements.len() && form.element_selected > 0 {
        form.element_selected -= 1;
    }
    //
    // Drop any connections referencing the deleted element.
    //
    form.connections
        .retain(|c| c.from_element != removed_id && c.to_element != removed_id);
}

//
// Returns the number of editable rows for the currently selected
// element's properties. Used by up/down navigation to clamp focus.
//

fn property_field_count(form: &ChainForm) -> usize {
    let Some(el) = form.selected_element() else {
        return 0;
    };
    //
    // Every kind has: ID (read-only), Kind (read-only), then a kind-specific
    // tail. We surface 1 + tail-count interactive rows because Kind is the
    // first focused row that toggles via Left/Right.
    //
    1 + kind_field_count(el.kind)
}

fn kind_field_count(kind: ElementKind) -> usize {
    match kind {
        ElementKind::Trigger => 0,
        ElementKind::Operation => 2,
        ElementKind::Transform => 2,
        ElementKind::GenericPrompt => 1,
        ElementKind::Memory => 2,
        ElementKind::Loop => 1,
        ElementKind::Tool => 2,
        ElementKind::Payload => 1,
        ElementKind::Termination => 0,
    }
}

fn push_prop_char(form: &mut ChainForm, c: char) {
    let field = form.focused_prop_field;
    let Some(el) = form.selected_element_mut() else {
        return;
    };
    //
    // Field 0 is Kind (toggle via Left/Right). Fields 1..=N are
    // kind-specific text inputs.
    //
    let tail = field.saturating_sub(1);
    match el.kind {
        ElementKind::Operation => match tail {
            0 => el.op_name.push(c),
            1 => el.model_ref.push(c),
            _ => {}
        },
        ElementKind::Transform => match tail {
            0 => el.prompt.push(c),
            1 => el.model_ref.push(c),
            _ => {}
        },
        ElementKind::GenericPrompt => {
            if tail == 0 {
                el.prompt.push(c);
            }
        }
        ElementKind::Memory => match tail {
            0 => el.memory_key.push(c),
            _ => {}
        },
        ElementKind::Loop => {
            if tail == 0 && c.is_ascii_digit() {
                el.max_iterations.push(c);
            }
        }
        ElementKind::Tool => match tail {
            0 => el.tool_name.push(c),
            1 => el.tool_params.push(c),
            _ => {}
        },
        ElementKind::Payload => {
            if tail == 0 {
                el.payload_id.push(c);
            }
        }
        ElementKind::Trigger | ElementKind::Termination => {}
    }
}

fn pop_prop_char(form: &mut ChainForm) {
    let field = form.focused_prop_field;
    let Some(el) = form.selected_element_mut() else {
        return;
    };
    let tail = field.saturating_sub(1);
    match el.kind {
        ElementKind::Operation => match tail {
            0 => {
                el.op_name.pop();
            }
            1 => {
                el.model_ref.pop();
            }
            _ => {}
        },
        ElementKind::Transform => match tail {
            0 => {
                el.prompt.pop();
            }
            1 => {
                el.model_ref.pop();
            }
            _ => {}
        },
        ElementKind::GenericPrompt => {
            if tail == 0 {
                el.prompt.pop();
            }
        }
        ElementKind::Memory => {
            if tail == 0 {
                el.memory_key.pop();
            }
        }
        ElementKind::Loop => {
            if tail == 0 {
                el.max_iterations.pop();
            }
        }
        ElementKind::Tool => match tail {
            0 => {
                el.tool_name.pop();
            }
            1 => {
                el.tool_params.pop();
            }
            _ => {}
        },
        ElementKind::Payload => {
            if tail == 0 {
                el.payload_id.pop();
            }
        }
        ElementKind::Trigger | ElementKind::Termination => {}
    }
}

//
// Left/Right toggles for non-text rows (Kind cycling on row 0, Memory mode
// on Memory's tail-1). For Operation, opens the op-name picker overlay so
// the user can pick from known op definitions instead of free-typing.
//

fn toggle_prop_field(form: &mut ChainForm, forward: bool) {
    let field = form.focused_prop_field;
    if field == 0 {
        cycle_element_kind(form, forward);
        return;
    }
    let tail = field - 1;
    let kind = match form.selected_element() {
        Some(el) => el.kind,
        None => return,
    };
    match (kind, tail) {
        (ElementKind::Memory, 1) => {
            if let Some(el) = form.selected_element_mut() {
                el.memory_mode = (el.memory_mode + 1) % 2;
            }
        }
        (ElementKind::Operation, 0) => {
            form.editor = Some(ChainFormEditor::PickOpName {
                cursor: 0,
                filter: String::new(),
            });
        }
        _ => {}
    }
}

fn cycle_element_kind(form: &mut ChainForm, forward: bool) {
    let Some(el) = form.selected_element_mut() else {
        return;
    };
    let cur = ElementKind::ALL
        .iter()
        .position(|k| *k == el.kind)
        .unwrap_or(0);
    let len = ElementKind::ALL.len();
    let next = if forward {
        (cur + 1) % len
    } else {
        (cur + len - 1) % len
    };
    el.kind = ElementKind::ALL[next];
}

fn cycle_condition(c: ConditionKind, delta: i32) -> ConditionKind {
    let list = [ConditionKind::None, ConditionKind::OnSuccess, ConditionKind::OnFailure];
    let cur = list.iter().position(|x| *x == c).unwrap_or(0);
    let len = list.len() as i32;
    let next = ((cur as i32 + delta).rem_euclid(len)) as usize;
    list[next]
}

fn open_connection_editor_for_selected(form: &mut ChainForm) {
    let Some(conn) = form.connections.get(form.connection_selected).cloned() else {
        return;
    };
    let from_idx = form
        .elements
        .iter()
        .position(|e| e.id == conn.from_element)
        .unwrap_or(0);
    let to_idx = form
        .elements
        .iter()
        .position(|e| e.id == conn.to_element)
        .unwrap_or(0);
    form.editor = Some(ChainFormEditor::EditConnection {
        editing_idx: Some(form.connection_selected),
        from_idx,
        to_idx,
        from_port: conn.from_port.to_string(),
        to_port: conn.to_port.to_string(),
        condition: conn.condition,
        focus: 0,
    });
}

fn commit_connection_editor(
    form: &mut ChainForm,
    editing_idx: Option<usize>,
    from_idx: usize,
    to_idx: usize,
    from_port: &str,
    to_port: &str,
    condition: ConditionKind,
) {
    let from = form.elements.get(from_idx).map(|e| e.id.clone());
    let to = form.elements.get(to_idx).map(|e| e.id.clone());
    let (Some(from), Some(to)) = (from, to) else {
        return;
    };
    let from_port = from_port.parse().unwrap_or(0);
    let to_port = to_port.parse().unwrap_or(0);

    if let Some(idx) = editing_idx {
        if let Some(c) = form.connections.get_mut(idx) {
            c.from_element = from;
            c.to_element = to;
            c.from_port = from_port;
            c.to_port = to_port;
            c.condition = condition;
        }
    } else {
        let next_id = format!("c_{}", form.connections.len() + 1);
        form.connections.push(ConnectionDraft {
            id: next_id,
            from_element: from,
            to_element: to,
            from_port,
            to_port,
            condition,
        });
        form.connection_selected = form.connections.len() - 1;
    }
}

//
// Draft <-> ChainElement conversion. We discard advanced fields
// (session_group, block_config) on round-trip because the TUI form
// doesn't yet expose them; they survive only if untouched on the
// service end. Editing a chain that uses them via the TUI will reset
// them — the same scope limit applied to the early web builder.
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

fn hit_contains(rect: &Rect, col: u16, row: u16) -> bool {
    if rect.width == 0 || rect.height == 0 {
        return false;
    }
    col >= rect.x && col < rect.x + rect.width && row >= rect.y && row < rect.y + rect.height
}

fn empty_to_none(s: &str) -> Option<String> {
    if s.trim().is_empty() {
        None
    } else {
        Some(s.trim().to_string())
    }
}
