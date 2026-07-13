//
// Rule form: edits an intercept rule (create or update). Field
// navigation and serialization into the client's request types.
//

use common::{InterceptRule, RuleScope, TargetDirection};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuleFormField {
    Name,
    Regex,
    Direction,
    Scope,
    ScopeNode,
    ScopeAgent,
    /// On/off toggle for LLM match summarization.
    Summarize,
    /// Free-text summarization prompt (only when Summarize is on).
    SummarizePrompt,
}

impl RuleFormField {
    /// True when space / ←→ should cycle this field rather than edit text.
    pub fn is_cycleable(self) -> bool {
        matches!(
            self,
            RuleFormField::Direction | RuleFormField::Scope | RuleFormField::Summarize
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuleFormScope {
    All,
    Node,
    Agent,
}

impl RuleFormScope {
    pub fn label(self) -> &'static str {
        match self {
            Self::All => "All nodes & agents",
            Self::Node => "Specific node",
            Self::Agent => "Specific agent",
        }
    }
    pub fn cycle(self) -> Self {
        match self {
            Self::All => Self::Node,
            Self::Node => Self::Agent,
            Self::Agent => Self::All,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FormMode {
    Create,
    Edit(i64),
}

pub struct RuleForm {
    pub mode: FormMode,
    pub focus: RuleFormField,
    pub name: String,
    pub regex: String,
    pub direction: TargetDirection,
    pub scope: RuleFormScope,
    pub scope_node: String,
    pub scope_agent: String,
    pub summarize: String,
    pub summarize_enabled: bool,
    pub last_error: Option<String>,
}

impl RuleForm {
    pub fn new_create() -> Self {
        Self {
            mode: FormMode::Create,
            focus: RuleFormField::Name,
            name: String::new(),
            regex: String::new(),
            direction: TargetDirection::Both,
            scope: RuleFormScope::All,
            scope_node: String::new(),
            scope_agent: String::new(),
            summarize: String::new(),
            summarize_enabled: false,
            last_error: None,
        }
    }

    pub fn from_rule(rule: &InterceptRule) -> Self {
        let (scope, scope_node, scope_agent) = match &rule.scope {
            RuleScope::All => (RuleFormScope::All, String::new(), String::new()),
            RuleScope::Node { node_id } => (RuleFormScope::Node, node_id.clone(), String::new()),
            RuleScope::Agent {
                node_id,
                agent_short_name,
            } => (
                RuleFormScope::Agent,
                node_id.clone(),
                agent_short_name.clone(),
            ),
        };
        let (summarize, summarize_enabled) = match &rule.summarization_prompt {
            Some(s) => (s.clone(), true),
            None => (String::new(), false),
        };
        Self {
            mode: FormMode::Edit(rule.id),
            focus: RuleFormField::Name,
            name: rule.name.clone(),
            regex: rule.regex_pattern.clone(),
            direction: rule.target_direction.clone(),
            scope,
            scope_node,
            scope_agent,
            summarize,
            summarize_enabled,
            last_error: None,
        }
    }

    //
    // Field ordering, skipping scope sub-fields when not applicable so
    // tab navigation doesn't land on inert inputs.
    //

    pub fn fields(&self) -> Vec<RuleFormField> {
        let mut v = vec![
            RuleFormField::Name,
            RuleFormField::Regex,
            RuleFormField::Direction,
            RuleFormField::Scope,
        ];
        match self.scope {
            RuleFormScope::All => {}
            RuleFormScope::Node => v.push(RuleFormField::ScopeNode),
            RuleFormScope::Agent => {
                v.push(RuleFormField::ScopeNode);
                v.push(RuleFormField::ScopeAgent);
            }
        }
        v.push(RuleFormField::Summarize);
        if self.summarize_enabled {
            v.push(RuleFormField::SummarizePrompt);
        }
        v
    }

    pub fn focus_next(&mut self) {
        let fields = self.fields();
        let i = fields.iter().position(|f| *f == self.focus).unwrap_or(0);
        self.focus = fields[(i + 1) % fields.len()];
    }

    pub fn focus_prev(&mut self) {
        let fields = self.fields();
        let i = fields.iter().position(|f| *f == self.focus).unwrap_or(0);
        self.focus = fields[(i + fields.len() - 1) % fields.len()];
    }

    pub fn current_text_mut(&mut self) -> Option<&mut String> {
        match self.focus {
            RuleFormField::Name => Some(&mut self.name),
            RuleFormField::Regex => Some(&mut self.regex),
            RuleFormField::ScopeNode => Some(&mut self.scope_node),
            RuleFormField::ScopeAgent => Some(&mut self.scope_agent),
            RuleFormField::SummarizePrompt => Some(&mut self.summarize),
            _ => None,
        }
    }

    pub fn cycle_current(&mut self) {
        match self.focus {
            RuleFormField::Direction => {
                self.direction = match self.direction {
                    TargetDirection::Both => TargetDirection::Send,
                    TargetDirection::Send => TargetDirection::Receive,
                    TargetDirection::Receive => TargetDirection::Both,
                };
            }
            RuleFormField::Scope => {
                self.scope = self.scope.cycle();
            }
            RuleFormField::Summarize => {
                self.summarize_enabled = !self.summarize_enabled;
                //
                // Prompt field disappears when toggled off; if focus was
                // left on it, clamp back to the toggle.
                //
                if !self.summarize_enabled
                    && matches!(self.focus, RuleFormField::SummarizePrompt)
                {
                    self.focus = RuleFormField::Summarize;
                }
            }
            _ => {}
        }
    }

    //
    // Validate and bundle the form into values ready for the client
    // API. Returns a human-readable error string on invalid input.
    //

    pub fn build(
        &self,
    ) -> Result<(String, String, TargetDirection, RuleScope, Option<String>), String> {
        let name = self.name.trim().to_string();
        if name.is_empty() {
            return Err("Name is required.".into());
        }
        let regex = self.regex.trim().to_string();
        if regex.is_empty() {
            return Err("Regex pattern is required.".into());
        }
        if let Err(e) = regex::Regex::new(&regex) {
            return Err(format!("Invalid regex: {}", e));
        }

        let scope = match self.scope {
            RuleFormScope::All => RuleScope::All,
            RuleFormScope::Node => {
                let node_id = self.scope_node.trim().to_string();
                if node_id.is_empty() {
                    return Err("Node ID is required for node-scoped rule.".into());
                }
                RuleScope::Node { node_id }
            }
            RuleFormScope::Agent => {
                let node_id = self.scope_node.trim().to_string();
                if node_id.is_empty() {
                    return Err("Node ID is required for agent-scoped rule.".into());
                }
                let agent = self.scope_agent.trim().to_string();
                if agent.is_empty() {
                    return Err("Agent short name is required for agent-scoped rule.".into());
                }
                RuleScope::Agent {
                    node_id,
                    agent_short_name: agent,
                }
            }
        };

        let summarize = if self.summarize_enabled {
            let s = self.summarize.trim();
            if s.is_empty() {
                None
            } else {
                Some(s.to_string())
            }
        } else {
            None
        };

        Ok((name, regex, self.direction.clone(), scope, summarize))
    }

    pub fn edit_id(&self) -> Option<i64> {
        match self.mode {
            FormMode::Create => None,
            FormMode::Edit(id) => Some(id),
        }
    }
}
