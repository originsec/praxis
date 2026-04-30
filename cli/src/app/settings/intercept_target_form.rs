//
// Form state for adding or editing an intercept target. Renders inline
// in the Intercept settings tab; navigation cycles through fields with
// Tab and editing happens directly on the focused field. Domains are
// captured as a comma- or newline-separated string and split on save.
//

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InterceptTargetFormMode {
    Create,
    Edit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InterceptTargetFormField {
    Name,
    AgentShortName,
    Domains,
    UrlPattern,
}

pub struct InterceptTargetForm {
    pub mode: InterceptTargetFormMode,
    pub editing_id: Option<String>,
    pub name: String,
    pub agent_short_name: String,
    pub domains: String,
    pub url_pattern: String,
    pub focused: InterceptTargetFormField,
    pub error: Option<String>,
}

impl InterceptTargetForm {
    pub fn new_create() -> Self {
        Self {
            mode: InterceptTargetFormMode::Create,
            editing_id: None,
            name: String::new(),
            agent_short_name: String::new(),
            domains: String::new(),
            url_pattern: String::new(),
            focused: InterceptTargetFormField::Name,
            error: None,
        }
    }

    pub fn from_existing(target: &common::InterceptTargetInfo) -> Self {
        Self {
            mode: InterceptTargetFormMode::Edit,
            editing_id: Some(target.id.clone()),
            name: target.name.clone(),
            agent_short_name: target.agent_short_name.clone(),
            domains: target.domains.join(", "),
            url_pattern: target.url_pattern.clone().unwrap_or_default(),
            focused: InterceptTargetFormField::Name,
            error: None,
        }
    }

    pub fn focus_next(&mut self) {
        self.focused = match self.focused {
            InterceptTargetFormField::Name => InterceptTargetFormField::AgentShortName,
            InterceptTargetFormField::AgentShortName => InterceptTargetFormField::Domains,
            InterceptTargetFormField::Domains => InterceptTargetFormField::UrlPattern,
            InterceptTargetFormField::UrlPattern => InterceptTargetFormField::Name,
        };
    }

    pub fn focus_prev(&mut self) {
        self.focused = match self.focused {
            InterceptTargetFormField::Name => InterceptTargetFormField::UrlPattern,
            InterceptTargetFormField::AgentShortName => InterceptTargetFormField::Name,
            InterceptTargetFormField::Domains => InterceptTargetFormField::AgentShortName,
            InterceptTargetFormField::UrlPattern => InterceptTargetFormField::Domains,
        };
    }

    fn focused_buf_mut(&mut self) -> &mut String {
        match self.focused {
            InterceptTargetFormField::Name => &mut self.name,
            InterceptTargetFormField::AgentShortName => &mut self.agent_short_name,
            InterceptTargetFormField::Domains => &mut self.domains,
            InterceptTargetFormField::UrlPattern => &mut self.url_pattern,
        }
    }

    pub fn type_char(&mut self, c: char) {
        self.focused_buf_mut().push(c);
    }

    pub fn backspace(&mut self) {
        self.focused_buf_mut().pop();
    }

    //
    // Convert the comma- or newline-separated domains buffer into a
    // de-duplicated, trimmed Vec<String>. Empty entries are dropped.
    //
    pub fn parsed_domains(&self) -> Vec<String> {
        let mut seen = std::collections::HashSet::new();
        let mut out = Vec::new();
        for raw in self.domains.split([',', '\n']) {
            let trimmed = raw.trim();
            if trimmed.is_empty() {
                continue;
            }
            if seen.insert(trimmed.to_string()) {
                out.push(trimmed.to_string());
            }
        }
        out
    }

    pub fn parsed_url_pattern(&self) -> Option<String> {
        let trimmed = self.url_pattern.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    }

    //
    // Validate user input. Returns Ok with cleaned values or Err with a
    // user-facing message. The agent short_name is required because
    // node-side routing keys captured traffic by it.
    //
    pub fn validate(&self) -> Result<(String, String, Vec<String>, Option<String>), String> {
        let name = self.name.trim().to_string();
        if name.is_empty() {
            return Err("Name is required".to_string());
        }
        let agent = self.agent_short_name.trim().to_string();
        if agent.is_empty() {
            return Err("Agent short name is required".to_string());
        }
        let domains = self.parsed_domains();
        if domains.is_empty() {
            return Err("At least one domain is required".to_string());
        }
        Ok((name, agent, domains, self.parsed_url_pattern()))
    }
}
