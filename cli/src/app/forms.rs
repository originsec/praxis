use std::collections::HashMap;

//
// Form used by the Nodes window to add a new remote agent node (e.g.
// a Codex app-server reachable over WebSocket). Submission publishes
// `ClientSignalMessage::AddRemoteNode`. The node's display name is
// derived from the upstream agent's reported identity once it
// initializes; the user only enters the URL and an optional token.
//

#[derive(Default)]
pub struct AddRemoteNodeForm {
    //
    // Index into the static remote-node kinds list. Codex is the only
    // option today, but the field exists so future kinds plug in
    // without UI changes.
    //
    pub kind_idx: usize,
    pub url: String,
    pub url_cursor: usize,
    pub token: String,
    pub token_cursor: usize,
    pub focused_field: usize, // 0=kind, 1=url, 2=token
    pub editing_text: bool,   // true while typing into URL/Token
}

impl AddRemoteNodeForm {
    pub const FIELD_COUNT: usize = 3;
    pub const KIND_FIELD: usize = 0;
    pub const URL_FIELD: usize = 1;
    pub const TOKEN_FIELD: usize = 2;

    pub fn field_label(idx: usize) -> &'static str {
        match idx {
            0 => "Type",
            1 => "URL",
            2 => "Token (opt)",
            _ => "",
        }
    }

    pub fn active_pair_mut(&mut self) -> Option<(&mut String, &mut usize)> {
        match self.focused_field {
            Self::URL_FIELD => Some((&mut self.url, &mut self.url_cursor)),
            Self::TOKEN_FIELD => Some((&mut self.token, &mut self.token_cursor)),
            _ => None,
        }
    }
}

pub struct NewOpForm {
    pub name: String,
    pub short_name: String,
    pub category: String,
    pub description: String,
    pub mode: usize, // 0=one-shot, 1=agent
    pub timeout: String,
    pub iterations: String,
    pub yolo: bool,
    pub prompt: String,
    pub focused_field: usize, // 0-8
}

impl NewOpForm {
    pub fn field_count() -> usize {
        9
    }

    //
    // Field indices: 0=Mode, 1=Name, 2=Short Name, 3=Category,
    // 4=Description, 5=Iterations, 6=Timeout, 7=YOLO, 8=Prompt
    //
    pub fn field_label(idx: usize) -> &'static str {
        match idx {
            0 => "Mode",
            1 => "Name",
            2 => "Short Name",
            3 => "Category",
            4 => "Description",
            5 => "Iterations",
            6 => "Timeout",
            7 => "YOLO",
            8 => "Prompt",
            _ => "",
        }
    }

    pub fn is_toggle(idx: usize) -> bool {
        matches!(idx, 0 | 7)
    }
}

pub struct RunOptions {
    pub op_name: String,
    pub is_chain: bool,
    pub chain_id: Option<String>,
    pub nodes: Vec<(String, String, bool)>, // (node_id, machine_name, selected)
    pub agents: Vec<(String, bool)>,        // (agent_short_name, selected)
    pub yolo: bool,
    pub focused_section: u8, // 0=nodes, 1=agents, 2=yolo
    pub cursor: usize,
}

//
// Trigger create/edit form. When `editing_id` is Some the form updates an
// existing trigger; otherwise it creates a new one.
//

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum TriggerKind {
    Scheduled,
    InterceptMatch,
    NewNode,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ScheduleKind {
    Interval,
    DailyAt,
}

pub struct TriggerForm {
    pub editing_id: Option<String>,

    //
    // Target chain: which chain this trigger fires. Displayed as a picker
    // cycled with left/right while the chain section is focused.
    //
    pub chains: Vec<(String, String)>, // (chain_id, display_name)
    pub chain_cursor: usize,

    //
    // Trigger type and its associated config.
    //
    pub kind: TriggerKind,
    pub schedule_kind: ScheduleKind,
    pub hour: u8,
    pub minute: u8,
    pub interval_minutes: u32,
    pub recurring: bool,

    //
    // Intercept rules available for InterceptMatch triggers.
    //
    pub rules: Vec<(i64, String)>, // (rule_id, display name)
    pub rule_cursor: usize,

    //
    // Target spec: node list + agents. Each is a (id/name, label, selected).
    //
    pub nodes: Vec<(String, String, bool)>,
    pub agents: Vec<(String, bool)>,
    pub os_filter: String,
    pub include_triggering_node: bool,

    //
    // Focus: 0=Chain, 1=Type, 2..=type-specific-rows, then nodes, agents,
    // os_filter, include_triggering_node. `section` picks which logical
    // pane is focused; `cursor` is the row within that pane.
    //
    pub focused_section: TriggerFormSection,
    pub cursor: usize,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum TriggerFormSection {
    Chain,
    Type,
    ScheduleKindRow,
    ScheduleValueRow,
    Recurring,
    Rule,
    Nodes,
    OsFilter,
    Agents,
    IncludeTriggering,
}

//
// Chain builder form. Blocks sit on a 2D canvas with orthogonal
// connectors. Header fields live above the canvas; properties open in a
// modal (double-click / Enter). Palette buttons add elements along the
// bottom. The struct owns the in-progress chain, viewport state, pickers,
// and dirty tracking.
//

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ElementKind {
    Trigger,
    Operation,
    Transform,
    GenericPrompt,
    Memory,
    Loop,
    Tool,
    Payload,
    Termination,
}

impl ElementKind {
    pub const ALL: [ElementKind; 9] = [
        ElementKind::Trigger,
        ElementKind::Operation,
        ElementKind::Transform,
        ElementKind::GenericPrompt,
        ElementKind::Memory,
        ElementKind::Loop,
        ElementKind::Tool,
        ElementKind::Payload,
        ElementKind::Termination,
    ];

    //
    // Kinds the properties cycler may change between. Trigger and
    // Termination stay fixed so the chain remains structurally valid.
    //
    pub const BODY: [ElementKind; 7] = [
        ElementKind::Operation,
        ElementKind::Transform,
        ElementKind::GenericPrompt,
        ElementKind::Memory,
        ElementKind::Loop,
        ElementKind::Tool,
        ElementKind::Payload,
    ];

    pub fn label(self) -> &'static str {
        match self {
            ElementKind::Trigger => "Trigger",
            ElementKind::Operation => "Operation",
            ElementKind::Transform => "Transform",
            ElementKind::GenericPrompt => "Generic Prompt",
            ElementKind::Memory => "Memory",
            ElementKind::Loop => "Loop",
            ElementKind::Tool => "Tool",
            ElementKind::Payload => "Payload",
            ElementKind::Termination => "Termination",
        }
    }

    pub fn short(self) -> &'static str {
        match self {
            ElementKind::Trigger => "TRG",
            ElementKind::Operation => "OP",
            ElementKind::Transform => "TXR",
            ElementKind::GenericPrompt => "GP",
            ElementKind::Memory => "MEM",
            ElementKind::Loop => "LP",
            ElementKind::Tool => "TL",
            ElementKind::Payload => "PL",
            ElementKind::Termination => "TRM",
        }
    }

    pub fn id_prefix(self) -> &'static str {
        match self {
            ElementKind::Trigger => "trigger",
            ElementKind::Operation => "op",
            ElementKind::Transform => "transform",
            ElementKind::GenericPrompt => "prompt",
            ElementKind::Memory => "mem",
            ElementKind::Loop => "loop",
            ElementKind::Tool => "tool",
            ElementKind::Payload => "payload",
            ElementKind::Termination => "term",
        }
    }

    pub fn is_body(self) -> bool {
        !matches!(self, ElementKind::Trigger | ElementKind::Termination)
    }

    pub fn supports_session_group(self) -> bool {
        matches!(
            self,
            ElementKind::Operation | ElementKind::Transform | ElementKind::GenericPrompt
        )
    }

    pub fn supports_block_config(self) -> bool {
        matches!(
            self,
            ElementKind::Operation
                | ElementKind::Transform
                | ElementKind::GenericPrompt
                | ElementKind::Tool
                | ElementKind::Payload
                | ElementKind::Termination
        )
    }
}

//
// Session-group fields on a draft element. Empty id means unassigned.
//

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SessionGroupDraft {
    pub id: String,
    pub color: String,
    pub yolo_mode: bool,
    pub working_dir: String,
}

//
// Per-block overrides. Empty strings / None mean "inherit default".
// yolo_mode: None = inherit, Some(true/false) = force.
// require_all_inputs: None = default true, Some(false) = any-input.
//

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BlockConfigDraft {
    pub max_runtime: String,
    pub yolo_mode: Option<bool>,
    pub working_dir: String,
    pub require_all_inputs: Option<bool>,
}

#[derive(Clone, Debug)]
pub struct ChainElementDraft {
    pub id: String,
    pub kind: ElementKind,
    pub op_name: String,
    pub model_ref: String,
    pub prompt: String,
    pub memory_key: String,
    pub memory_mode: u8,
    pub max_iterations: String,
    pub tool_name: String,
    pub tool_params: String,
    pub payload_id: String,
    pub session_group: SessionGroupDraft,
    pub block_config: BlockConfigDraft,
}

impl ChainElementDraft {
    pub fn new(id: String, kind: ElementKind) -> Self {
        Self {
            id,
            kind,
            op_name: String::new(),
            model_ref: String::new(),
            prompt: String::new(),
            memory_key: String::new(),
            memory_mode: 0,
            max_iterations: "10".to_string(),
            tool_name: String::new(),
            tool_params: "{}".to_string(),
            payload_id: String::new(),
            session_group: SessionGroupDraft::default(),
            block_config: BlockConfigDraft::default(),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ConditionKind {
    None,
    OnSuccess,
    OnFailure,
}

#[derive(Clone, Debug)]
pub struct ConnectionDraft {
    pub id: String,
    pub from_element: String,
    pub to_element: String,
    pub from_port: u32,
    pub to_port: u32,
    pub condition: ConditionKind,
}

//
// Overlay pickers stacked above the properties modal / canvas.
//

#[derive(Clone, Debug)]
pub enum ChainFormEditor {
    PickOpName { cursor: usize, filter: String },
    PickModel { cursor: usize, filter: String },
    PickTool { cursor: usize, filter: String },
    PickPayload { cursor: usize, filter: String },
    PickSessionGroup { cursor: usize },
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Selected {
    None,
    Block(String),
    Connection(usize),
}

//
// Active drag. Block drag may be Pending until the cursor moves past
// DRAG_THRESHOLD so a simple click does not nudge the block.
//

#[derive(Clone, Debug)]
pub enum Drag {
    None,
    PendingBlock {
        id: String,
        grab_dx: i32,
        grab_dy: i32,
        start_col: u16,
        start_row: u16,
    },
    Block {
        id: String,
        grab_dx: i32,
        grab_dy: i32,
    },
    Canvas {
        last_col: u16,
        last_row: u16,
    },
    Port {
        from_id: String,
        from_port: u32,
        cursor_col: u16,
        cursor_row: u16,
    },
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum EditTarget {
    HeaderName,
    HeaderCategory,
    HeaderTimeout,
    HeaderDescription,
    BlockProp { id: String, field: BlockField },
    ConnectionPort { idx: usize, side: PortSide },
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PortSide {
    From,
    To,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BlockField {
    OpName,
    ModelRef,
    Prompt,
    MemoryKey,
    MaxIterations,
    ToolName,
    ToolParams,
    PayloadId,
    SessionGroupColor,
    SessionGroupWorkingDir,
    BlockMaxRuntime,
    BlockWorkingDir,
}

pub struct ChainForm {
    pub editing_id: Option<String>,
    pub name: String,
    pub category: String,
    pub description: String,
    pub timeout: String,
    pub elements: Vec<ChainElementDraft>,
    pub connections: Vec<ConnectionDraft>,
    pub positions: HashMap<String, (i32, i32)>,
    pub camera_x: i32,
    pub camera_y: i32,
    pub selected: Selected,
    pub drag: Drag,
    pub editing: Option<EditTarget>,
    pub props_modal: bool,
    pub props_scroll: u16,
    pub available_op_names: Vec<String>,
    pub available_models: Vec<String>,
    pub available_tools: Vec<String>,
    pub available_payloads: Vec<String>,
    pub element_id_seq: u32,
    pub editor: Option<ChainFormEditor>,
    pub error: Option<String>,
    //
    // True after any user mutation since open. Esc/Cancel confirm when set.
    //
    pub dirty: bool,
    //
    // Session-group color palette index for newly created groups.
    //
    pub next_sg_color: usize,
}

impl ChainForm {
    pub fn new(
        available_op_names: Vec<String>,
        available_models: Vec<String>,
        available_tools: Vec<String>,
        available_payloads: Vec<String>,
    ) -> Self {
        Self {
            editing_id: None,
            name: String::new(),
            category: "custom".to_string(),
            description: String::new(),
            timeout: String::new(),
            elements: Vec::new(),
            connections: Vec::new(),
            positions: HashMap::new(),
            camera_x: 0,
            camera_y: 0,
            selected: Selected::None,
            drag: Drag::None,
            editing: None,
            props_modal: false,
            props_scroll: 0,
            available_op_names,
            available_models,
            available_tools,
            available_payloads,
            element_id_seq: 0,
            editor: None,
            error: None,
            dirty: false,
            next_sg_color: 0,
        }
    }

    pub fn mark_dirty(&mut self) {
        self.dirty = true;
        self.error = None;
    }

    pub fn next_element_id(&mut self, kind: ElementKind) -> String {
        self.element_id_seq += 1;
        format!("{}_{}", kind.id_prefix(), self.element_id_seq)
    }

    pub fn selected_block_mut(&mut self) -> Option<&mut ChainElementDraft> {
        if let Selected::Block(ref id) = self.selected.clone() {
            self.elements.iter_mut().find(|e| &e.id == id)
        } else {
            None
        }
    }

    pub fn block_pos(&self, id: &str) -> (i32, i32) {
        self.positions.get(id).copied().unwrap_or((0, 0))
    }

    pub fn next_session_group_color(&mut self) -> String {
        const COLORS: &[&str] = &[
            "#8B5CF6", "#06B6D4", "#F59E0B", "#10B981", "#EF4444", "#3B82F6", "#EC4899",
        ];
        let c = COLORS[self.next_sg_color % COLORS.len()].to_string();
        self.next_sg_color += 1;
        c
    }
}

impl TriggerForm {
    //
    // Section ordering depends on the trigger type. Scheduled has a few
    // extra rows; InterceptMatch swaps schedule rows for a rule picker;
    // NewNode has neither. All three include the target spec sections at
    // the bottom.
    //

    pub fn section_order(&self) -> Vec<TriggerFormSection> {
        let mut order = vec![TriggerFormSection::Chain, TriggerFormSection::Type];
        match self.kind {
            TriggerKind::Scheduled => {
                order.push(TriggerFormSection::ScheduleKindRow);
                order.push(TriggerFormSection::ScheduleValueRow);
                order.push(TriggerFormSection::Recurring);
            }
            TriggerKind::InterceptMatch => {
                order.push(TriggerFormSection::Rule);
            }
            TriggerKind::NewNode => {}
        }
        order.push(TriggerFormSection::Nodes);
        order.push(TriggerFormSection::OsFilter);
        order.push(TriggerFormSection::Agents);
        if matches!(
            self.kind,
            TriggerKind::InterceptMatch | TriggerKind::NewNode
        ) {
            order.push(TriggerFormSection::IncludeTriggering);
        }
        order
    }
}
