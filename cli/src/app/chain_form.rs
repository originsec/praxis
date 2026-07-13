use super::*;
use crate::ui::chain_form::{BLOCK_H, BLOCK_W};
use common::{
    BlockConfig, ChainConnection, ChainDefinitionInput, ChainElement, ChainTriggerType,
    ConnectionCondition, ElementPosition, MemoryMode, SessionGroup,
};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use std::collections::{HashMap, HashSet};

//
// Spacing for auto-layout. A column is one block width plus a gap; a row
// is one block height plus a gap.
//

pub const LAYOUT_COL_GAP: i32 = BLOCK_W as i32 + 6;
pub const LAYOUT_ROW_GAP: i32 = BLOCK_H as i32 + 2;
pub const CANVAS_ORIGIN_X: i32 = 4;
pub const CANVAS_ORIGIN_Y: i32 = 2;

//
// Hit-test forgiveness: ports match a 3×3 cell around the port; edges
// match within Manhattan distance ≤ EDGE_HIT_TOLERANCE of the path.
// Block drag only starts after the cursor moves DRAG_THRESHOLD cells.
//

pub const PORT_HIT_TOLERANCE: i32 = 1;
pub const EDGE_HIT_TOLERANCE: i32 = 1;
pub const DRAG_THRESHOLD: i32 = 2;

//
// Known toolkit tools when the service list is not cached in the TUI.
//

pub const KNOWN_TOOLKIT_TOOLS: &[&str] = &["message_encoder", "session_history_poisoning"];

// ---------------------------------------------------------------------------
// Pure helpers (unit-tested without a terminal)
// ---------------------------------------------------------------------------

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
// Port centres in canvas coordinates for an element at (bx, by).
//

pub fn input_port_pos(bx: i32, by: i32) -> (i32, i32) {
    (bx - 1, by + (BLOCK_H as i32 / 2))
}

pub fn output_port_pos(bx: i32, by: i32, port: u32) -> (i32, i32) {
    (bx + BLOCK_W as i32, by + 1 + port as i32)
}

fn within_tol(ax: i32, ay: i32, bx: i32, by: i32, tol: i32) -> bool {
    (ax - bx).abs() <= tol && (ay - by).abs() <= tol
}

//
// Nearest port within PORT_HIT_TOLERANCE of (canvas_col, canvas_row).
// Prefers the closest match when multiple ports overlap the tolerance box.
//

pub fn port_at_tolerant(
    form: &ChainForm,
    canvas_col: i32,
    canvas_row: i32,
) -> Option<(String, u32, PortSide)> {
    let mut best: Option<(i32, String, u32, PortSide)> = None;
    for el in &form.elements {
        let (x, y) = form.block_pos(&el.id);
        if input_port_count(el.kind) > 0 {
            let (px, py) = input_port_pos(x, y);
            if within_tol(canvas_col, canvas_row, px, py, PORT_HIT_TOLERANCE) {
                let dist = (canvas_col - px).abs() + (canvas_row - py).abs();
                if best.as_ref().map(|(d, ..)| dist < *d).unwrap_or(true) {
                    best = Some((dist, el.id.clone(), 0, PortSide::To));
                }
            }
        }
        let n = output_port_count(el.kind);
        for port in 0..n {
            let (px, py) = output_port_pos(x, y, port);
            if within_tol(canvas_col, canvas_row, px, py, PORT_HIT_TOLERANCE) {
                let dist = (canvas_col - px).abs() + (canvas_row - py).abs();
                if best.as_ref().map(|(d, ..)| dist < *d).unwrap_or(true) {
                    best = Some((dist, el.id.clone(), port, PortSide::From));
                }
            }
        }
    }
    best.map(|(_, id, port, side)| (id, port, side))
}

//
// Connection path hit with Manhattan distance ≤ EDGE_HIT_TOLERANCE.
//

pub fn connection_at_tolerant(
    form: &ChainForm,
    canvas_col: i32,
    canvas_row: i32,
    route: impl Fn(&ChainForm, &ConnectionDraft) -> Vec<(i32, i32)>,
) -> Option<usize> {
    let mut best: Option<(i32, usize)> = None;
    for (idx, conn) in form.connections.iter().enumerate() {
        let path = route(form, conn);
        for &(x, y) in &path {
            let dist = (canvas_col - x).abs() + (canvas_row - y).abs();
            if dist <= EDGE_HIT_TOLERANCE {
                if best.as_ref().map(|(d, _)| dist < *d).unwrap_or(true) {
                    best = Some((dist, idx));
                }
            }
        }
    }
    best.map(|(_, idx)| idx)
}

pub fn cycle_condition(c: ConditionKind, delta: i32) -> ConditionKind {
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
// Per-element incompleteness messages (empty if the element is ready).
//

pub fn element_issues(el: &ChainElementDraft) -> Vec<String> {
    let mut issues = Vec::new();
    match el.kind {
        ElementKind::Operation if el.op_name.trim().is_empty() => {
            issues.push(format!("{}: no operation selected", el.id));
        }
        ElementKind::Transform | ElementKind::GenericPrompt if el.prompt.trim().is_empty() => {
            issues.push(format!("{}: prompt is empty", el.id));
        }
        ElementKind::Memory if el.memory_key.trim().is_empty() => {
            issues.push(format!("{}: memory key is empty", el.id));
        }
        ElementKind::Loop => {
            if el.max_iterations.trim().is_empty()
                || el.max_iterations.parse::<u32>().unwrap_or(0) == 0
            {
                issues.push(format!("{}: max iterations must be ≥ 1", el.id));
            }
        }
        ElementKind::Tool if el.tool_name.trim().is_empty() => {
            issues.push(format!("{}: no tool selected", el.id));
        }
        ElementKind::Payload if el.payload_id.trim().is_empty() => {
            issues.push(format!("{}: no payload selected", el.id));
        }
        _ => {}
    }
    if el.kind.supports_block_config() && !el.block_config.max_runtime.trim().is_empty() {
        if el.block_config.max_runtime.parse::<u64>().is_err() {
            issues.push(format!("{}: block max runtime must be a number", el.id));
        }
    }
    issues
}

//
// Full form validation. Returns an ordered list of human-readable errors.
// Empty means the form is safe to submit (modulo server-side checks).
//

pub fn validate_chain_form(form: &ChainForm) -> Vec<String> {
    let mut errors = Vec::new();
    if form.name.trim().is_empty() {
        errors.push("Name is required".to_string());
    }
    if form.elements.is_empty() {
        errors.push("Chain must have at least one element".to_string());
    }
    let trigger_count = form
        .elements
        .iter()
        .filter(|e| e.kind == ElementKind::Trigger)
        .count();
    if trigger_count != 1 {
        errors.push("Exactly one Trigger element is required".to_string());
    }
    let term_count = form
        .elements
        .iter()
        .filter(|e| e.kind == ElementKind::Termination)
        .count();
    if term_count != 1 {
        errors.push("Exactly one Termination element is required".to_string());
    }
    if !form.timeout.trim().is_empty() && form.timeout.parse::<u64>().is_err() {
        errors.push("Timeout must be a number".to_string());
    }
    for el in &form.elements {
        errors.extend(element_issues(el));
    }

    //
    // Reachability: every non-trigger element should be reachable from the
    // trigger; termination should be reachable.
    //
    let trigger_id = form
        .elements
        .iter()
        .find(|e| e.kind == ElementKind::Trigger)
        .map(|e| e.id.clone());
    if let Some(trig) = trigger_id {
        let mut adj: HashMap<String, Vec<String>> = HashMap::new();
        for c in &form.connections {
            adj.entry(c.from_element.clone())
                .or_default()
                .push(c.to_element.clone());
        }
        let mut seen = HashSet::new();
        let mut stack = vec![trig];
        while let Some(id) = stack.pop() {
            if !seen.insert(id.clone()) {
                continue;
            }
            if let Some(next) = adj.get(&id) {
                stack.extend(next.iter().cloned());
            }
        }
        for el in &form.elements {
            if el.kind == ElementKind::Trigger {
                continue;
            }
            if !seen.contains(&el.id) {
                errors.push(format!("{}: not reachable from Trigger", el.id));
            }
        }
    }

    //
    // Dangling connections to missing elements.
    //
    let ids: HashSet<&str> = form.elements.iter().map(|e| e.id.as_str()).collect();
    for c in &form.connections {
        if !ids.contains(c.from_element.as_str()) || !ids.contains(c.to_element.as_str()) {
            errors.push(format!("Connection {} references a missing element", c.id));
        }
    }

    errors
}

pub fn empty_to_none(s: &str) -> Option<String> {
    if s.trim().is_empty() {
        None
    } else {
        Some(s.trim().to_string())
    }
}

pub fn draft_session_group(d: &SessionGroupDraft) -> Option<SessionGroup> {
    if d.id.trim().is_empty() {
        return None;
    }
    Some(SessionGroup {
        id: d.id.trim().to_string(),
        color: if d.color.trim().is_empty() {
            "#8B5CF6".to_string()
        } else {
            d.color.trim().to_string()
        },
        yolo_mode: d.yolo_mode,
        working_dir: empty_to_none(&d.working_dir),
    })
}

pub fn draft_block_config(d: &BlockConfigDraft) -> Option<BlockConfig> {
    let max_runtime = if d.max_runtime.trim().is_empty() {
        None
    } else {
        d.max_runtime.parse::<u64>().ok()
    };
    let working_dir = empty_to_none(&d.working_dir);
    if max_runtime.is_none()
        && d.yolo_mode.is_none()
        && working_dir.is_none()
        && d.require_all_inputs.is_none()
    {
        return None;
    }
    Some(BlockConfig {
        max_runtime,
        yolo_mode: d.yolo_mode,
        working_dir,
        require_all_inputs: d.require_all_inputs,
    })
}

pub fn draft_to_element(d: &ChainElementDraft) -> ChainElement {
    match d.kind {
        ElementKind::Trigger => ChainElement::Trigger {
            id: d.id.clone(),
            trigger_type: ChainTriggerType::Manual,
        },
        ElementKind::Operation => ChainElement::Operation {
            id: d.id.clone(),
            operation_name: d.op_name.clone(),
            model_ref: empty_to_none(&d.model_ref),
            session_group: draft_session_group(&d.session_group),
            block_config: draft_block_config(&d.block_config),
        },
        ElementKind::Transform => ChainElement::Transform {
            id: d.id.clone(),
            prompt: d.prompt.clone(),
            model_ref: empty_to_none(&d.model_ref),
            session_group: draft_session_group(&d.session_group),
            block_config: draft_block_config(&d.block_config),
        },
        ElementKind::GenericPrompt => ChainElement::GenericPrompt {
            id: d.id.clone(),
            prompt: d.prompt.clone(),
            session_group: draft_session_group(&d.session_group),
            block_config: draft_block_config(&d.block_config),
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
                block_config: draft_block_config(&d.block_config),
            }
        }
        ElementKind::Payload => ChainElement::Payload {
            id: d.id.clone(),
            payload_id: d.payload_id.clone(),
            block_config: draft_block_config(&d.block_config),
        },
        ElementKind::Termination => ChainElement::Termination {
            id: d.id.clone(),
            block_config: draft_block_config(&d.block_config),
        },
    }
}

pub fn element_to_draft(el: &ChainElement) -> ChainElementDraft {
    match el {
        ChainElement::Trigger { id, .. } => {
            ChainElementDraft::new(id.clone(), ElementKind::Trigger)
        }
        ChainElement::Operation {
            id,
            operation_name,
            model_ref,
            session_group,
            block_config,
        } => {
            let mut d = ChainElementDraft::new(id.clone(), ElementKind::Operation);
            d.op_name = operation_name.clone();
            d.model_ref = model_ref.clone().unwrap_or_default();
            apply_session_group(&mut d, session_group.as_ref());
            apply_block_config(&mut d, block_config.as_ref());
            d
        }
        ChainElement::Transform {
            id,
            prompt,
            model_ref,
            session_group,
            block_config,
        } => {
            let mut d = ChainElementDraft::new(id.clone(), ElementKind::Transform);
            d.prompt = prompt.clone();
            d.model_ref = model_ref.clone().unwrap_or_default();
            apply_session_group(&mut d, session_group.as_ref());
            apply_block_config(&mut d, block_config.as_ref());
            d
        }
        ChainElement::GenericPrompt {
            id,
            prompt,
            session_group,
            block_config,
        } => {
            let mut d = ChainElementDraft::new(id.clone(), ElementKind::GenericPrompt);
            d.prompt = prompt.clone();
            apply_session_group(&mut d, session_group.as_ref());
            apply_block_config(&mut d, block_config.as_ref());
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
            block_config,
        } => {
            let mut d = ChainElementDraft::new(id.clone(), ElementKind::Tool);
            d.tool_name = tool_name.clone();
            d.tool_params = serde_json::to_string(tool_params).unwrap_or_else(|_| "{}".to_string());
            apply_block_config(&mut d, block_config.as_ref());
            d
        }
        ChainElement::Payload {
            id,
            payload_id,
            block_config,
        } => {
            let mut d = ChainElementDraft::new(id.clone(), ElementKind::Payload);
            d.payload_id = payload_id.clone();
            apply_block_config(&mut d, block_config.as_ref());
            d
        }
        ChainElement::Termination { id, block_config } => {
            let mut d = ChainElementDraft::new(id.clone(), ElementKind::Termination);
            apply_block_config(&mut d, block_config.as_ref());
            d
        }
    }
}

fn apply_session_group(d: &mut ChainElementDraft, sg: Option<&SessionGroup>) {
    if let Some(sg) = sg {
        d.session_group = SessionGroupDraft {
            id: sg.id.clone(),
            color: sg.color.clone(),
            yolo_mode: sg.yolo_mode,
            working_dir: sg.working_dir.clone().unwrap_or_default(),
        };
    }
}

fn apply_block_config(d: &mut ChainElementDraft, bc: Option<&BlockConfig>) {
    if let Some(bc) = bc {
        d.block_config = BlockConfigDraft {
            max_runtime: bc
                .max_runtime
                .map(|v| v.to_string())
                .unwrap_or_default(),
            yolo_mode: bc.yolo_mode,
            working_dir: bc.working_dir.clone().unwrap_or_default(),
            require_all_inputs: bc.require_all_inputs,
        };
    }
}

//
// Auto-layout: BFS from triggers, left-to-right columns.
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
// Place a new block to the right of the selection (or below the graph).
//

pub fn position_for_new_element(form: &ChainForm, fallback_centre: Option<(i32, i32)>) -> (i32, i32) {
    if let Selected::Block(ref id) = form.selected {
        let (x, y) = form.block_pos(id);
        return (x + LAYOUT_COL_GAP, y);
    }
    fallback_centre.unwrap_or_else(|| next_free_position(form))
}

//
// Wire a newly added element into the graph: if a body block is selected,
// insert between it and its first downstream neighbour (or just append).
// If a trigger is selected, insert between trigger and its first target.
//

pub fn auto_wire_new_element(form: &mut ChainForm, new_id: &str) {
    let Selected::Block(ref from_id) = form.selected.clone() else {
        return;
    };
    if from_id == new_id {
        return;
    }
    let from_kind = form
        .elements
        .iter()
        .find(|e| e.id == *from_id)
        .map(|e| e.kind);
    let Some(from_kind) = from_kind else {
        return;
    };
    if output_port_count(from_kind) == 0 {
        return;
    }
    let new_kind = form
        .elements
        .iter()
        .find(|e| e.id == new_id)
        .map(|e| e.kind);
    if new_kind == Some(ElementKind::Trigger) {
        return;
    }
    if input_port_count(new_kind.unwrap_or(ElementKind::Operation)) == 0 {
        return;
    }

    //
    // Prefer rewiring the first unconditional edge out of `from` through
    // the new node; otherwise append a fresh edge.
    //
    if let Some(idx) = form
        .connections
        .iter()
        .position(|c| c.from_element == *from_id && c.from_port == 0)
    {
        let old = form.connections[idx].clone();
        form.connections[idx].to_element = new_id.to_string();
        form.connections[idx].to_port = 0;
        let next_id = format!(
            "c_{}",
            form.connections.len() + 1 + form.element_id_seq as usize
        );
        form.connections.push(ConnectionDraft {
            id: next_id,
            from_element: new_id.to_string(),
            to_element: old.to_element,
            from_port: 0,
            to_port: old.to_port,
            condition: ConditionKind::None,
        });
    } else {
        let next_id = format!(
            "c_{}",
            form.connections.len() + 1 + form.element_id_seq as usize
        );
        form.connections.push(ConnectionDraft {
            id: next_id,
            from_element: from_id.clone(),
            to_element: new_id.to_string(),
            from_port: 0,
            to_port: 0,
            condition: ConditionKind::None,
        });
    }
}

pub fn delete_selection(form: &mut ChainForm) {
    match form.selected.clone() {
        Selected::Block(id) => {
            let kind = form.elements.iter().find(|e| e.id == id).map(|e| e.kind);
            //
            // Refuse deleting the only Trigger / Termination so the graph
            // stays structurally valid without a surprise validation wall.
            //
            if matches!(kind, Some(ElementKind::Trigger)) {
                let n = form
                    .elements
                    .iter()
                    .filter(|e| e.kind == ElementKind::Trigger)
                    .count();
                if n <= 1 {
                    form.error = Some("Cannot delete the only Trigger".to_string());
                    return;
                }
            }
            if matches!(kind, Some(ElementKind::Termination)) {
                let n = form
                    .elements
                    .iter()
                    .filter(|e| e.kind == ElementKind::Termination)
                    .count();
                if n <= 1 {
                    form.error = Some("Cannot delete the only Termination".to_string());
                    return;
                }
            }
            form.elements.retain(|e| e.id != id);
            form.connections
                .retain(|c| c.from_element != id && c.to_element != id);
            form.positions.remove(&id);
            form.selected = Selected::None;
            form.props_modal = false;
            form.editing = None;
            form.mark_dirty();
        }
        Selected::Connection(idx) => {
            if idx < form.connections.len() {
                form.connections.remove(idx);
            }
            form.selected = Selected::None;
            form.props_modal = false;
            form.editing = None;
            form.mark_dirty();
        }
        Selected::None => {}
    }
}

pub fn cycle_body_kind(kind: ElementKind) -> ElementKind {
    if !kind.is_body() {
        return kind;
    }
    let cur = ElementKind::BODY
        .iter()
        .position(|k| *k == kind)
        .unwrap_or(0);
    ElementKind::BODY[(cur + 1) % ElementKind::BODY.len()]
}

//
// Keyboard selection targets: every block (element order), then every
// connection (index order). Tab / Shift+Tab walk this list without a mouse.
//

pub fn selection_targets(form: &ChainForm) -> Vec<Selected> {
    let mut out = Vec::new();
    for el in &form.elements {
        out.push(Selected::Block(el.id.clone()));
    }
    for i in 0..form.connections.len() {
        out.push(Selected::Connection(i));
    }
    out
}

pub fn cycle_selection(form: &mut ChainForm, delta: i32) {
    let targets = selection_targets(form);
    if targets.is_empty() {
        form.selected = Selected::None;
        form.props_modal = false;
        return;
    }
    let cur = targets
        .iter()
        .position(|t| t == &form.selected)
        .unwrap_or(usize::MAX);
    let len = targets.len() as i32;
    let next = if cur == usize::MAX {
        if delta >= 0 {
            0
        } else {
            targets.len() - 1
        }
    } else {
        ((cur as i32 + delta).rem_euclid(len)) as usize
    };
    form.selected = targets[next].clone();
    form.props_modal = false;
    form.editing = None;
    form.props_scroll = 0;
}

//
// Unique session groups currently assigned on the form (for the picker).
//

pub fn collect_session_groups(form: &ChainForm) -> Vec<SessionGroupDraft> {
    let mut out: Vec<SessionGroupDraft> = Vec::new();
    for el in &form.elements {
        if el.session_group.id.trim().is_empty() {
            continue;
        }
        if !out.iter().any(|g| g.id == el.session_group.id) {
            out.push(el.session_group.clone());
        }
    }
    out
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
        BlockField::SessionGroupColor => &mut el.session_group.color,
        BlockField::SessionGroupWorkingDir => &mut el.session_group.working_dir,
        BlockField::BlockMaxRuntime => &mut el.block_config.max_runtime,
        BlockField::BlockWorkingDir => &mut el.block_config.working_dir,
    }
}

fn handle_edit_key(form: &mut ChainForm, key: KeyEvent) {
    let Some(target) = form.editing.clone() else {
        return;
    };
    match key.code {
        KeyCode::Enter if !key.modifiers.contains(KeyModifiers::SHIFT) => {
            form.editing = None;
            return;
        }
        KeyCode::Tab => {
            form.editing = None;
            return;
        }
        KeyCode::Enter if key.modifiers.contains(KeyModifiers::SHIFT) => {
            //
            // Shift+Enter inserts a newline into multiline fields.
            //
            if let EditTarget::BlockProp {
                ref id,
                field: BlockField::Prompt | BlockField::ToolParams,
            } = target
            {
                if let Some(el) = form.elements.iter_mut().find(|e| &e.id == id) {
                    block_field_mut(
                        el,
                        match target {
                            EditTarget::BlockProp { field, .. } => field,
                            _ => return,
                        },
                    )
                    .push('\n');
                    form.mark_dirty();
                }
            }
            return;
        }
        KeyCode::Backspace => match target {
            EditTarget::HeaderName => {
                form.name.pop();
                form.mark_dirty();
            }
            EditTarget::HeaderCategory => {
                form.category.pop();
                form.mark_dirty();
            }
            EditTarget::HeaderTimeout => {
                form.timeout.pop();
                form.mark_dirty();
            }
            EditTarget::HeaderDescription => {
                form.description.pop();
                form.mark_dirty();
            }
            EditTarget::BlockProp { ref id, field } => {
                if let Some(el) = form.elements.iter_mut().find(|e| &e.id == id) {
                    block_field_mut(el, field).pop();
                    form.mark_dirty();
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
                    form.mark_dirty();
                }
            }
        },
        KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => match target {
            EditTarget::HeaderName => {
                form.name.push(c);
                form.mark_dirty();
            }
            EditTarget::HeaderCategory => {
                form.category.push(c);
                form.mark_dirty();
            }
            EditTarget::HeaderTimeout => {
                if c.is_ascii_digit() {
                    form.timeout.push(c);
                    form.mark_dirty();
                }
            }
            EditTarget::HeaderDescription => {
                form.description.push(c);
                form.mark_dirty();
            }
            EditTarget::BlockProp {
                ref id,
                field: BlockField::MaxIterations | BlockField::BlockMaxRuntime,
            } => {
                if c.is_ascii_digit() {
                    if let Some(el) = form.elements.iter_mut().find(|e| &e.id == id) {
                        let field = match target {
                            EditTarget::BlockProp { field, .. } => field,
                            _ => return,
                        };
                        block_field_mut(el, field).push(c);
                        form.mark_dirty();
                    }
                }
            }
            EditTarget::BlockProp { ref id, field } => {
                if let Some(el) = form.elements.iter_mut().find(|e| &e.id == id) {
                    block_field_mut(el, field).push(c);
                    form.mark_dirty();
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
                        form.mark_dirty();
                    }
                }
            }
        },
        _ => {}
    }
}

// ---------------------------------------------------------------------------
// App methods
// ---------------------------------------------------------------------------

impl App {
    fn chain_picker_lists(&self) -> (Vec<String>, Vec<String>, Vec<String>, Vec<String>) {
        let ops: Vec<String> = self
            .operations
            .op_definitions
            .iter()
            .filter(|d| !d.disabled)
            .map(|d| d.full_name.clone())
            .collect();
        let models: Vec<String> = self
            .settings
            .model_definitions
            .iter()
            .map(|d| {
                if d.name.is_empty() {
                    format!("{}::{}", d.provider, d.model)
                } else {
                    d.name.clone()
                }
            })
            .collect();
        let tools: Vec<String> = KNOWN_TOOLKIT_TOOLS
            .iter()
            .map(|s| (*s).to_string())
            .collect();
        let payloads: Vec<String> = Vec::new();
        (ops, models, tools, payloads)
    }

    pub(crate) fn open_new_chain_form(&mut self) {
        let (ops, models, tools, payloads) = self.chain_picker_lists();
        let mut form = ChainForm::new(ops, models, tools, payloads);

        let trig_id = form.next_element_id(ElementKind::Trigger);
        let term_id = form.next_element_id(ElementKind::Termination);
        form.elements.push(ChainElementDraft::new(
            trig_id.clone(),
            ElementKind::Trigger,
        ));
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
        //
        // Pre-select Trigger so the first palette / keyboard add auto-wires
        // instead of leaving an orphan node.
        //
        form.selected = Selected::Block(trig_id);
        form.dirty = false;
        self.chain_form = Some(form);
    }

    pub(crate) fn open_edit_chain_form_for(&mut self, chain: common::ChainDefinitionFull) {
        let (ops, models, tools, payloads) = self.chain_picker_lists();
        let mut form = ChainForm::new(ops, models, tools, payloads);
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
        for (id, pos) in &chain.positions {
            form.positions
                .insert(id.clone(), (pos.x as i32, pos.y as i32));
        }
        if form.positions.is_empty() {
            auto_layout(&mut form);
        } else {
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

        if let Some(trig) = form
            .elements
            .iter()
            .find(|e| e.kind == ElementKind::Trigger)
            .map(|e| e.id.clone())
        {
            form.selected = Selected::Block(trig);
        }
        form.dirty = false;
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

    pub(crate) fn request_close_chain_form(&mut self) {
        let dirty = self.chain_form.as_ref().map(|f| f.dirty).unwrap_or(false);
        if dirty {
            self.confirm = Some(ConfirmAction {
                message: "Discard unsaved chain changes?".to_string(),
                action: ConfirmKind::DiscardChainForm,
            });
        } else {
            self.chain_form = None;
        }
    }

    pub(crate) async fn submit_chain_form(&mut self) {
        let Some(form) = self.chain_form.as_mut() else {
            return;
        };

        let errors = validate_chain_form(form);
        if !errors.is_empty() {
            form.error = Some(errors.join("; "));
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
    // Add element: position relative to selection, auto-wire, open props /
    // op picker for Operation.
    //

    pub(crate) fn add_element_at_centre(&mut self, kind: ElementKind) {
        let centre = self.chain_form_hits.borrow().canvas_centre_for_new_block();
        let Some(form) = self.chain_form.as_mut() else {
            return;
        };
        //
        // Only one Trigger / Termination each.
        //
        if kind == ElementKind::Trigger
            && form.elements.iter().any(|e| e.kind == ElementKind::Trigger)
        {
            form.error = Some("Chain already has a Trigger".to_string());
            return;
        }
        if kind == ElementKind::Termination
            && form
                .elements
                .iter()
                .any(|e| e.kind == ElementKind::Termination)
        {
            form.error = Some("Chain already has a Termination".to_string());
            return;
        }

        let wire_from = form.selected.clone();
        let id = form.next_element_id(kind);
        let draft = ChainElementDraft::new(id.clone(), kind);
        form.elements.push(draft);
        let pos = position_for_new_element(form, centre);
        form.positions.insert(id.clone(), pos);

        if matches!(wire_from, Selected::Block(_)) {
            //
            // Temporarily restore selection so auto_wire sees the previous
            // block, then select the new one.
            //
            form.selected = wire_from;
            auto_wire_new_element(form, &id);
        }
        form.selected = Selected::Block(id.clone());
        form.editing = None;
        form.props_modal = kind != ElementKind::Operation;
        form.props_scroll = 0;
        form.mark_dirty();

        if kind == ElementKind::Operation {
            form.editor = Some(ChainFormEditor::PickOpName {
                cursor: 0,
                filter: String::new(),
            });
            form.props_modal = true;
        }
    }

    pub(crate) async fn handle_chain_form_key(&mut self, key: KeyEvent) {
        if self
            .chain_form
            .as_ref()
            .and_then(|f| f.editor.as_ref())
            .is_some()
        {
            self.handle_picker_key(key);
            return;
        }

        match key.code {
            KeyCode::Esc => {
                if let Some(form) = self.chain_form.as_mut() {
                    if form.editing.is_some() {
                        form.editing = None;
                        return;
                    }
                    if form.props_modal {
                        form.props_modal = false;
                        form.props_scroll = 0;
                        return;
                    }
                }
                self.request_close_chain_form();
                return;
            }
            KeyCode::Char('s') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.submit_chain_form().await;
                return;
            }
            KeyCode::Enter if !key.modifiers.contains(KeyModifiers::SHIFT) => {
                if let Some(form) = self.chain_form.as_mut() {
                    if form.editing.is_some() {
                        form.editing = None;
                        return;
                    }
                    if !matches!(form.selected, Selected::None) {
                        form.props_modal = true;
                        form.props_scroll = 0;
                        return;
                    }
                }
            }
            _ => {}
        }

        if let Some(form) = self.chain_form.as_mut() {
            if form.editing.is_some() {
                handle_edit_key(form, key);
                return;
            }
        }

        let props_open = self
            .chain_form
            .as_ref()
            .map(|f| f.props_modal)
            .unwrap_or(false);

        //
        // Palette shortcuts only on the canvas (not while properties are
        // open): o=op, t=transform, g=generic prompt, m=memory, p=loop,
        // k=tool, y=payload.
        //
        if !props_open {
            let add_kind = match key.code {
                KeyCode::Char('o') if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                    Some(ElementKind::Operation)
                }
                KeyCode::Char('t') if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                    Some(ElementKind::Transform)
                }
                KeyCode::Char('g') if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                    Some(ElementKind::GenericPrompt)
                }
                KeyCode::Char('m') if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                    Some(ElementKind::Memory)
                }
                KeyCode::Char('p') if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                    Some(ElementKind::Loop)
                }
                KeyCode::Char('k') if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                    Some(ElementKind::Tool)
                }
                KeyCode::Char('y') if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                    Some(ElementKind::Payload)
                }
                _ => None,
            };
            if let Some(kind) = add_kind {
                self.add_element_at_centre(kind);
                return;
            }
        }

        if let Some(form) = self.chain_form.as_mut() {
            match key.code {
                KeyCode::Left => form.camera_x -= 4,
                KeyCode::Right => form.camera_x += 4,
                KeyCode::Up => {
                    if form.props_modal {
                        form.props_scroll = form.props_scroll.saturating_sub(1);
                    } else {
                        form.camera_y -= 2;
                    }
                }
                KeyCode::Down => {
                    if form.props_modal {
                        form.props_scroll = form.props_scroll.saturating_add(1);
                    } else {
                        form.camera_y += 2;
                    }
                }
                KeyCode::Delete | KeyCode::Backspace => {
                    delete_selection(form);
                }
                KeyCode::Char('c') if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                    //
                    // If nothing / a block is selected, jump to the first
                    // connection so condition cycling is reachable by keyboard.
                    //
                    if !matches!(form.selected, Selected::Connection(_)) {
                        if !form.connections.is_empty() {
                            form.selected = Selected::Connection(0);
                            form.props_modal = false;
                        }
                    }
                    if let Selected::Connection(idx) = form.selected {
                        if let Some(conn) = form.connections.get_mut(idx) {
                            conn.condition = cycle_condition(conn.condition, 1);
                            form.mark_dirty();
                        }
                    }
                }
                KeyCode::Char('l') if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                    form.positions.clear();
                    auto_layout(form);
                    form.camera_x = 0;
                    form.camera_y = 0;
                    form.mark_dirty();
                }
                KeyCode::Tab if form.props_modal => {
                    if let Selected::Block(ref id) = form.selected.clone() {
                        if let Some(el) = form.elements.iter_mut().find(|e| &e.id == id) {
                            if el.kind.is_body() {
                                el.kind = cycle_body_kind(el.kind);
                                form.mark_dirty();
                            }
                        }
                    }
                }
                KeyCode::Tab if !form.props_modal => {
                    let delta = if key.modifiers.contains(KeyModifiers::SHIFT) {
                        -1
                    } else {
                        1
                    };
                    cycle_selection(form, delta);
                }
                KeyCode::BackTab if !form.props_modal => {
                    cycle_selection(form, -1);
                }
                _ => {}
            }
        }
    }

    fn handle_picker_key(&mut self, key: KeyEvent) {
        if matches!(key.code, KeyCode::Esc) {
            if let Some(form) = self.chain_form.as_mut() {
                form.editor = None;
            }
            return;
        }
        let Some(form) = self.chain_form.as_mut() else {
            return;
        };
        let Some(editor) = form.editor.take() else {
            return;
        };

        match editor {
            ChainFormEditor::PickSessionGroup { mut cursor } => {
                let mut items: Vec<String> = vec!["(none)".to_string(), "(new group)".to_string()];
                for g in collect_session_groups(form) {
                    items.push(g.id);
                }
                match key.code {
                    KeyCode::Up => {
                        if cursor > 0 {
                            cursor -= 1;
                        }
                    }
                    KeyCode::Down => {
                        if cursor + 1 < items.len() {
                            cursor += 1;
                        }
                    }
                    KeyCode::Enter => {
                        match cursor {
                            0 => {
                                if let Some(el) = form.selected_block_mut() {
                                    el.session_group = SessionGroupDraft::default();
                                    form.mark_dirty();
                                }
                            }
                            1 => {
                                let color = form.next_session_group_color();
                                let id = format!("sg_{}", form.element_id_seq + 1);
                                form.element_id_seq += 1;
                                if let Some(el) = form.selected_block_mut() {
                                    el.session_group = SessionGroupDraft {
                                        id,
                                        color,
                                        yolo_mode: false,
                                        working_dir: String::new(),
                                    };
                                    form.mark_dirty();
                                }
                            }
                            n => {
                                let groups = collect_session_groups(form);
                                if let Some(g) = groups.get(n - 2).cloned() {
                                    if let Some(el) = form.selected_block_mut() {
                                        el.session_group = g;
                                        form.mark_dirty();
                                    }
                                }
                            }
                        }
                        return;
                    }
                    _ => {}
                }
                form.editor = Some(ChainFormEditor::PickSessionGroup { cursor });
            }
            other => {
                let (kind_tag, list, mut cursor, mut filter) = match other {
                    ChainFormEditor::PickOpName { cursor, filter } => {
                        ("op", form.available_op_names.clone(), cursor, filter)
                    }
                    ChainFormEditor::PickModel { cursor, filter } => {
                        ("model", form.available_models.clone(), cursor, filter)
                    }
                    ChainFormEditor::PickTool { cursor, filter } => {
                        ("tool", form.available_tools.clone(), cursor, filter)
                    }
                    ChainFormEditor::PickPayload { cursor, filter } => {
                        ("payload", form.available_payloads.clone(), cursor, filter)
                    }
                    ChainFormEditor::PickSessionGroup { .. } => unreachable!(),
                };
                let filtered: Vec<String> = list
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
                            if let Some(el) = form.selected_block_mut() {
                                match kind_tag {
                                    "op" => el.op_name = name.clone(),
                                    "model" => el.model_ref = name.clone(),
                                    "tool" => el.tool_name = name.clone(),
                                    "payload" => el.payload_id = name.clone(),
                                    _ => {}
                                }
                                form.mark_dirty();
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
                form.editor = Some(match kind_tag {
                    "model" => ChainFormEditor::PickModel { cursor, filter },
                    "tool" => ChainFormEditor::PickTool { cursor, filter },
                    "payload" => ChainFormEditor::PickPayload { cursor, filter },
                    _ => ChainFormEditor::PickOpName { cursor, filter },
                });
            }
        }
    }

    pub(crate) fn handle_chain_form_motion(&mut self, mouse: MouseEvent) {
        match mouse.kind {
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

    pub(crate) async fn chain_form_canvas_down(&mut self, mouse: MouseEvent) {
        let hit = self.chain_form_hits.borrow().clone();
        let col = mouse.column;
        let row = mouse.row;

        if hit.canvas.contains(col, row) {
            let canvas_col = (col as i32 - hit.canvas.x as i32) + self.chain_form_camera_x();
            let canvas_row = (row as i32 - hit.canvas.y as i32) + self.chain_form_camera_y();

            let port_hit = {
                let form = self.chain_form.as_ref();
                form.and_then(|f| port_at_tolerant(f, canvas_col, canvas_row))
            };
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
            }

            if let Some(id) = self.block_at(canvas_col, canvas_row) {
                let is_dbl = self.is_double_click(row, col);
                let pos = self
                    .chain_form
                    .as_ref()
                    .map(|f| f.block_pos(&id))
                    .unwrap_or((0, 0));
                let grab_dx = canvas_col - pos.0;
                let grab_dy = canvas_row - pos.1;
                if let Some(form) = self.chain_form.as_mut() {
                    form.selected = Selected::Block(id.clone());
                    form.editing = None;
                    if is_dbl {
                        form.props_modal = true;
                        form.props_scroll = 0;
                        form.drag = Drag::None;
                    } else {
                        form.props_modal = false;
                        form.drag = Drag::PendingBlock {
                            id,
                            grab_dx,
                            grab_dy,
                            start_col: col,
                            start_row: row,
                        };
                    }
                }
                return;
            }

            let conn_idx = {
                let form = self.chain_form.as_ref();
                form.and_then(|f| {
                    connection_at_tolerant(f, canvas_col, canvas_row, |form, conn| {
                        crate::ui::chain_form::route_connection(form, conn)
                    })
                })
            };
            if let Some(idx) = conn_idx {
                let is_dbl = self.is_double_click(row, col);
                if let Some(form) = self.chain_form.as_mut() {
                    form.selected = Selected::Connection(idx);
                    form.editing = None;
                    form.props_modal = is_dbl;
                    form.props_scroll = 0;
                    form.drag = Drag::None;
                }
                return;
            }

            if let Some(form) = self.chain_form.as_mut() {
                form.drag = Drag::Canvas {
                    last_col: col,
                    last_row: row,
                };
                form.selected = Selected::None;
                form.editing = None;
                form.props_modal = false;
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
            Drag::PendingBlock {
                id,
                grab_dx,
                grab_dy,
                start_col,
                start_row,
            } => {
                let dx = (col as i32 - start_col as i32).abs();
                let dy = (row as i32 - start_row as i32).abs();
                if dx >= DRAG_THRESHOLD || dy >= DRAG_THRESHOLD {
                    form.drag = Drag::Block {
                        id: id.clone(),
                        grab_dx,
                        grab_dy,
                    };
                    if !hit.canvas.is_empty() {
                        let canvas_col = (col as i32 - hit.canvas.x as i32) + form.camera_x;
                        let canvas_row = (row as i32 - hit.canvas.y as i32) + form.camera_y;
                        let new_x = canvas_col - grab_dx;
                        let new_y = canvas_row - grab_dy;
                        form.positions.insert(id, (new_x.max(0), new_y.max(0)));
                        form.mark_dirty();
                    }
                }
            }
            Drag::Block {
                id,
                grab_dx,
                grab_dy,
            } => {
                if !hit.canvas.is_empty() {
                    let canvas_col = (col as i32 - hit.canvas.x as i32) + form.camera_x;
                    let canvas_row = (row as i32 - hit.canvas.y as i32) + form.camera_y;
                    let new_x = canvas_col - grab_dx;
                    let new_y = canvas_row - grab_dy;
                    form.positions.insert(id, (new_x.max(0), new_y.max(0)));
                    form.mark_dirty();
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
                from_id, from_port, ..
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
            let canvas_col =
                (mouse.column as i32 - hit.canvas.x as i32) + self.chain_form_camera_x();
            let canvas_row = (mouse.row as i32 - hit.canvas.y as i32) + self.chain_form_camera_y();
            if let Some(form) = self.chain_form.as_ref() {
                if let Some((to_id, to_port, side)) =
                    port_at_tolerant(form, canvas_col, canvas_row)
                {
                    if side == PortSide::To && to_id != from_id {
                        if let Some(form) = self.chain_form.as_mut() {
                            let dup = form.connections.iter().any(|c| {
                                c.from_element == from_id
                                    && c.to_element == to_id
                                    && c.from_port == from_port
                                    && c.to_port == to_port
                            });
                            if !dup {
                                let next_id = format!(
                                    "c_{}",
                                    form.connections.len() + 1 + form.element_id_seq as usize
                                );
                                form.connections.push(ConnectionDraft {
                                    id: next_id,
                                    from_element: from_id,
                                    to_element: to_id,
                                    from_port,
                                    to_port,
                                    condition: ConditionKind::None,
                                });
                                form.selected = Selected::Connection(form.connections.len() - 1);
                                form.mark_dirty();
                            }
                        }
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

    pub(crate) fn cycle_selected_kind(&mut self) {
        let Some(form) = self.chain_form.as_mut() else {
            return;
        };
        let Selected::Block(ref id) = form.selected.clone() else {
            return;
        };
        if let Some(el) = form.elements.iter_mut().find(|e| &e.id == id) {
            if el.kind.is_body() {
                el.kind = cycle_body_kind(el.kind);
                form.mark_dirty();
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Unit tests — pure helpers only (no terminal)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn empty_form() -> ChainForm {
        ChainForm::new(vec!["cat::op".into()], vec!["openai::gpt".into()], vec!["message_encoder".into()], vec![])
    }

    fn seeded() -> ChainForm {
        let mut form = empty_form();
        form.name = "demo".into();
        let t = form.next_element_id(ElementKind::Trigger);
        let o = form.next_element_id(ElementKind::Operation);
        let e = form.next_element_id(ElementKind::Termination);
        form.elements
            .push(ChainElementDraft::new(t.clone(), ElementKind::Trigger));
        let mut op = ChainElementDraft::new(o.clone(), ElementKind::Operation);
        op.op_name = "cat::op".into();
        form.elements.push(op);
        form.elements
            .push(ChainElementDraft::new(e.clone(), ElementKind::Termination));
        form.connections.push(ConnectionDraft {
            id: "c1".into(),
            from_element: t,
            to_element: o.clone(),
            from_port: 0,
            to_port: 0,
            condition: ConditionKind::None,
        });
        form.connections.push(ConnectionDraft {
            id: "c2".into(),
            from_element: o,
            to_element: e,
            from_port: 0,
            to_port: 0,
            condition: ConditionKind::OnSuccess,
        });
        auto_layout(&mut form);
        form
    }

    #[test]
    fn port_hit_tolerance_accepts_neighbour_cells() {
        let form = seeded();
        let el = form.elements.iter().find(|e| e.kind == ElementKind::Operation).unwrap();
        let (bx, by) = form.block_pos(&el.id);
        let (px, py) = output_port_pos(bx, by, 0);
        // exact
        let hit = port_at_tolerant(&form, px, py).expect("exact");
        assert_eq!(hit.0, el.id);
        assert_eq!(hit.2, PortSide::From);
        // offset by tolerance
        let hit2 = port_at_tolerant(&form, px + 1, py).expect("tol");
        assert_eq!(hit2.0, el.id);
        // outside tolerance
        assert!(port_at_tolerant(&form, px + PORT_HIT_TOLERANCE + 1, py).is_none());
    }

    #[test]
    fn edge_hit_tolerance_accepts_near_path() {
        let form = seeded();
        let route = |f: &ChainForm, c: &ConnectionDraft| {
            // Minimal L path for test: use real router if available, else
            // rebuild from stored positions like the UI does.
            crate::ui::chain_form::route_connection(f, c)
        };
        let path = route(&form, &form.connections[0]);
        assert!(!path.is_empty());
        let (x, y) = path[path.len() / 2];
        let idx = connection_at_tolerant(&form, x, y + EDGE_HIT_TOLERANCE, route);
        assert_eq!(idx, Some(0));
        assert!(connection_at_tolerant(&form, x, y + EDGE_HIT_TOLERANCE + 2, route).is_none());
    }

    #[test]
    fn validation_rejects_empty_op_and_missing_name() {
        let mut form = seeded();
        form.name.clear();
        form.elements[1].op_name.clear();
        let errs = validate_chain_form(&form);
        assert!(errs.iter().any(|e| e.contains("Name")));
        assert!(errs.iter().any(|e| e.contains("no operation")));
    }

    #[test]
    fn validation_flags_unreachable() {
        let mut form = seeded();
        let orphan = form.next_element_id(ElementKind::Memory);
        form.elements
            .push(ChainElementDraft::new(orphan, ElementKind::Memory));
        let errs = validate_chain_form(&form);
        assert!(errs.iter().any(|e| e.contains("not reachable")));
    }

    #[test]
    fn draft_roundtrip_session_group_and_block_config() {
        let mut d = ChainElementDraft::new("op_1".into(), ElementKind::Operation);
        d.op_name = "x::y".into();
        d.session_group = SessionGroupDraft {
            id: "sg1".into(),
            color: "#FF00FF".into(),
            yolo_mode: true,
            working_dir: "/tmp".into(),
        };
        d.block_config = BlockConfigDraft {
            max_runtime: "30".into(),
            yolo_mode: Some(false),
            working_dir: "/work".into(),
            require_all_inputs: Some(false),
        };
        let el = draft_to_element(&d);
        match &el {
            ChainElement::Operation {
                session_group,
                block_config,
                ..
            } => {
                let sg = session_group.as_ref().expect("sg");
                assert_eq!(sg.id, "sg1");
                assert!(sg.yolo_mode);
                assert_eq!(sg.working_dir.as_deref(), Some("/tmp"));
                let bc = block_config.as_ref().expect("bc");
                assert_eq!(bc.max_runtime, Some(30));
                assert_eq!(bc.yolo_mode, Some(false));
                assert_eq!(bc.require_all_inputs, Some(false));
            }
            _ => panic!("expected Operation"),
        }
        let back = element_to_draft(&el);
        assert_eq!(back.session_group.id, "sg1");
        assert_eq!(back.block_config.max_runtime, "30");
        assert_eq!(back.block_config.yolo_mode, Some(false));
    }

    #[test]
    fn memory_mode_roundtrip() {
        let mut d = ChainElementDraft::new("m1".into(), ElementKind::Memory);
        d.memory_key = "k".into();
        d.memory_mode = 1;
        let el = draft_to_element(&d);
        let back = element_to_draft(&el);
        assert_eq!(back.memory_mode, 1);
        assert_eq!(back.memory_key, "k");
    }

    #[test]
    fn auto_wire_inserts_between() {
        let mut form = seeded();
        // select trigger
        let trig = form
            .elements
            .iter()
            .find(|e| e.kind == ElementKind::Trigger)
            .unwrap()
            .id
            .clone();
        form.selected = Selected::Block(trig);
        let id = form.next_element_id(ElementKind::Transform);
        form.elements
            .push(ChainElementDraft::new(id.clone(), ElementKind::Transform));
        form.positions.insert(id.clone(), (10, 10));
        auto_wire_new_element(&mut form, &id);
        // new node should appear as a from or to of some connection
        assert!(form
            .connections
            .iter()
            .any(|c| c.from_element == id || c.to_element == id));
    }

    #[test]
    fn position_for_new_is_right_of_selection() {
        let mut form = seeded();
        let op = form
            .elements
            .iter()
            .find(|e| e.kind == ElementKind::Operation)
            .unwrap()
            .id
            .clone();
        let (ox, oy) = form.block_pos(&op);
        form.selected = Selected::Block(op);
        let (nx, ny) = position_for_new_element(&form, None);
        assert_eq!(nx, ox + LAYOUT_COL_GAP);
        assert_eq!(ny, oy);
    }

    #[test]
    fn cycle_body_skips_trigger() {
        assert_eq!(cycle_body_kind(ElementKind::Trigger), ElementKind::Trigger);
        assert_ne!(cycle_body_kind(ElementKind::Operation), ElementKind::Trigger);
        assert!(cycle_body_kind(ElementKind::Operation).is_body());
    }

    #[test]
    fn draft_block_config_none_when_empty() {
        assert!(draft_block_config(&BlockConfigDraft::default()).is_none());
        let bc = draft_block_config(&BlockConfigDraft {
            max_runtime: "5".into(),
            ..Default::default()
        })
        .unwrap();
        assert_eq!(bc.max_runtime, Some(5));
    }

    #[test]
    fn session_group_none_when_empty_id() {
        assert!(draft_session_group(&SessionGroupDraft::default()).is_none());
        let sg = draft_session_group(&SessionGroupDraft {
            id: "a".into(),
            color: "".into(),
            yolo_mode: false,
            working_dir: String::new(),
        })
        .unwrap();
        assert_eq!(sg.id, "a");
        assert_eq!(sg.color, "#8B5CF6");
    }

    #[test]
    fn condition_cycle_wraps() {
        assert_eq!(
            cycle_condition(ConditionKind::None, 1),
            ConditionKind::OnSuccess
        );
        assert_eq!(
            cycle_condition(ConditionKind::OnFailure, 1),
            ConditionKind::None
        );
    }

    #[test]
    fn cycle_selection_walks_blocks_then_connections() {
        let mut form = seeded();
        form.selected = Selected::None;
        cycle_selection(&mut form, 1);
        assert!(matches!(form.selected, Selected::Block(_)));
        let first_block = form.selected.clone();
        // advance past all blocks into connections
        let n_blocks = form.elements.len();
        for _ in 0..n_blocks {
            cycle_selection(&mut form, 1);
        }
        assert!(
            matches!(form.selected, Selected::Connection(_)),
            "expected connection after {} block steps, got {:?}",
            n_blocks,
            form.selected
        );
        // full reverse cycle returns to the same selection
        let at = form.selected.clone();
        let n = selection_targets(&form).len();
        for _ in 0..n {
            cycle_selection(&mut form, -1);
        }
        assert_eq!(form.selected, at);
        // reverse from connection back to a block
        cycle_selection(&mut form, -1);
        assert!(matches!(form.selected, Selected::Block(_)));
        let _ = first_block;
    }

    #[test]
    fn preselected_trigger_auto_wire_reaches_termination() {
        //
        // Mirrors open_new_chain_form: seed Trigger→Term, select Trigger,
        // add Operation with auto_wire — graph must stay fully reachable.
        //
        let mut form = empty_form();
        form.name = "demo".into();
        let t = form.next_element_id(ElementKind::Trigger);
        let e = form.next_element_id(ElementKind::Termination);
        form.elements
            .push(ChainElementDraft::new(t.clone(), ElementKind::Trigger));
        form.elements
            .push(ChainElementDraft::new(e.clone(), ElementKind::Termination));
        form.connections.push(ConnectionDraft {
            id: "c1".into(),
            from_element: t.clone(),
            to_element: e,
            from_port: 0,
            to_port: 0,
            condition: ConditionKind::None,
        });
        auto_layout(&mut form);
        form.selected = Selected::Block(t);
        let id = form.next_element_id(ElementKind::Operation);
        let mut op = ChainElementDraft::new(id.clone(), ElementKind::Operation);
        op.op_name = "cat::op".into();
        form.elements.push(op);
        form.positions
            .insert(id.clone(), position_for_new_element(&form, None));
        auto_wire_new_element(&mut form, &id);
        form.selected = Selected::Block(id);
        let errs = validate_chain_form(&form);
        assert!(
            errs.is_empty(),
            "expected valid graph after auto-wire from Trigger, got {:?}",
            errs
        );
    }

    #[test]
    fn selection_targets_include_connections() {
        let form = seeded();
        let targets = selection_targets(&form);
        assert_eq!(
            targets.len(),
            form.elements.len() + form.connections.len()
        );
        assert!(targets.iter().any(|t| matches!(t, Selected::Connection(0))));
    }
}
