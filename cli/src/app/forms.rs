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
