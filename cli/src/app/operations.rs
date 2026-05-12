use super::*;
use common::TriggerConfig;

impl App {
    pub(crate) fn refresh_operations(&self) {
        let client = self.client.clone();
        let tx = self.event_tx.clone();

        tokio::spawn(async move {
            let Some(tx) = tx else { return };

            let _ = client.request_op_def_list().await;
            let _ = client.request_semantic_op_list().await;
            let _ = client.request_chain_list().await;
            let _ = client.request_chain_execution_list().await;
            let _ = client.request_chain_triggers().await;

            tokio::time::sleep(Duration::from_millis(300)).await;

            let op_definitions = client.get_operation_definitions().await;
            let chain_definitions = client.get_chain_definitions().await;
            let operations = client.get_operations().await;
            let chain_executions = client.get_chain_executions().await;
            let triggers = client.get_chain_triggers().await;
            let intercept_rules = client.list_intercept_rules().await.unwrap_or_default();

            let _ = tx.send(AppEvent::OperationsRefreshed {
                op_definitions,
                chain_definitions,
                operations,
                chain_executions,
            });
            let _ = tx.send(AppEvent::TriggersRefreshed {
                triggers,
                intercept_rules,
            });
        });
    }

    pub(crate) fn refresh_triggers_after(&self, delay: Duration) {
        let client = self.client.clone();
        let tx = self.event_tx.clone();

        tokio::spawn(async move {
            let Some(tx) = tx else { return };

            if !delay.is_zero() {
                tokio::time::sleep(delay).await;
            }

            let _ = client.request_chain_triggers().await;
            tokio::time::sleep(Duration::from_millis(300)).await;

            let triggers = client.get_chain_triggers().await;
            let intercept_rules = client.list_intercept_rules().await.unwrap_or_default();
            let _ = tx.send(AppEvent::TriggersRefreshed {
                triggers,
                intercept_rules,
            });
        });
    }

    pub(crate) fn refresh_library_after(&self, delay: Duration) {
        let client = self.client.clone();
        let tx = self.event_tx.clone();

        tokio::spawn(async move {
            let Some(tx) = tx else { return };

            if !delay.is_zero() {
                tokio::time::sleep(delay).await;
            }

            let _ = client.request_op_def_list().await;
            let _ = client.request_chain_list().await;
            tokio::time::sleep(Duration::from_millis(300)).await;

            let op_definitions = client.get_operation_definitions().await;
            let chain_definitions = client.get_chain_definitions().await;

            let _ = tx.send(AppEvent::LibraryRefreshed {
                op_definitions,
                chain_definitions,
            });
        });
    }

    pub(crate) fn refresh_execution_lists_after(&self, delay: Duration, reset_selection: bool) {
        let client = self.client.clone();
        let tx = self.event_tx.clone();

        tokio::spawn(async move {
            let Some(tx) = tx else { return };

            if !delay.is_zero() {
                tokio::time::sleep(delay).await;
            }

            let initial_operations = client.get_operations().await;
            let initial_chain_executions = client.get_chain_executions().await;
            let initial_operations_snapshot =
                serde_json::to_string(&initial_operations).unwrap_or_default();
            let initial_chain_snapshot =
                serde_json::to_string(&initial_chain_executions).unwrap_or_default();

            let _ = client.request_semantic_op_list().await;
            let _ = client.request_chain_execution_list().await;

            let mut operations = initial_operations.clone();
            let mut chain_executions = initial_chain_executions.clone();

            for _ in 0..20 {
                tokio::time::sleep(Duration::from_millis(100)).await;

                operations = client.get_operations().await;
                chain_executions = client.get_chain_executions().await;

                let operations_snapshot = serde_json::to_string(&operations).unwrap_or_default();
                let chain_snapshot = serde_json::to_string(&chain_executions).unwrap_or_default();

                if operations_snapshot != initial_operations_snapshot
                    || chain_snapshot != initial_chain_snapshot
                {
                    break;
                }
            }

            let _ = tx.send(AppEvent::ExecutionListsRefreshed {
                operations,
                chain_executions,
                reset_selection,
            });
        });
    }

    pub(crate) async fn handle_operations_key(&mut self, key: KeyEvent) {
        //
        // Tab switches tabs regardless of pane focus — the user should
        // always be able to jump from Library to Executions without
        // first escaping the detail pane.
        //
        if key.code == KeyCode::Tab {
            self.operations.tab = match self.operations.tab {
                OpsTab::Executions => OpsTab::Library,
                OpsTab::Library => OpsTab::Triggers,
                OpsTab::Triggers => OpsTab::Executions,
            };
            self.operations.filter.clear();
            if self.operations.tab == OpsTab::Triggers {
                self.refresh_triggers_after(Duration::ZERO);
            }
            return;
        }
        if key.code == KeyCode::BackTab {
            self.operations.tab = match self.operations.tab {
                OpsTab::Executions => OpsTab::Triggers,
                OpsTab::Library => OpsTab::Executions,
                OpsTab::Triggers => OpsTab::Library,
            };
            self.operations.filter.clear();
            if self.operations.tab == OpsTab::Triggers {
                self.refresh_triggers_after(Duration::ZERO);
            }
            return;
        }

        if self.operations.detail_focus {
            match key.code {
                KeyCode::Esc | KeyCode::Left => {
                    self.operations.detail_focus = false;
                }
                KeyCode::Up => {
                    if self.operations.collapsed.focused_section > 0 {
                        self.operations.collapsed.focused_section -= 1;
                    }
                }
                KeyCode::Down => {
                    let max = CollapsedSections::section_count().saturating_sub(1);
                    if self.operations.collapsed.focused_section < max {
                        self.operations.collapsed.focused_section += 1;
                    }
                }
                KeyCode::Enter | KeyCode::Char(' ') => {
                    let idx = self.operations.collapsed.focused_section;
                    if idx < self.operations.collapsed.sections.len() {
                        self.operations.collapsed.sections[idx] =
                            !self.operations.collapsed.sections[idx];
                    }
                }
                KeyCode::PageUp => {
                    self.operations.detail_scroll =
                        self.operations.detail_scroll.saturating_sub(10);
                }
                KeyCode::PageDown => {
                    let max = self.operations.exec_detail_max_scroll.get();
                    self.operations.detail_scroll =
                        self.operations.detail_scroll.saturating_add(10).min(max);
                }
                _ => {}
            }
            return;
        }

        match key.code {
            KeyCode::Up => match self.operations.tab {
                OpsTab::Library => {
                    if self.operations.library_selected > 0 {
                        self.operations.library_selected -= 1;
                    }
                }
                OpsTab::Executions => {
                    if self.operations.exec_selected > 0 {
                        self.operations.exec_selected -= 1;
                        self.operations.detail_scroll = 0;
                    }
                }
                OpsTab::Triggers => {
                    if self.operations.trigger_selected > 0 {
                        self.operations.trigger_selected -= 1;
                    }
                }
            },
            KeyCode::Down => match self.operations.tab {
                OpsTab::Library => {
                    let total = self.ops_library_count();
                    if self.operations.library_selected + 1 < total {
                        self.operations.library_selected += 1;
                    }
                }
                OpsTab::Executions => {
                    let total = self.sorted_executions().len();
                    if self.operations.exec_selected + 1 < total {
                        self.operations.exec_selected += 1;
                        self.operations.detail_scroll = 0;
                    }
                }
                OpsTab::Triggers => {
                    let total = self.operations.triggers.len();
                    if self.operations.trigger_selected + 1 < total {
                        self.operations.trigger_selected += 1;
                    }
                }
            },
            KeyCode::Right => {
                self.operations.detail_focus = true;
                self.operations.detail_scroll = 0;
            }
            KeyCode::Enter if self.operations.filter_focused => {
                self.operations.filter_focused = false;
            }
            KeyCode::Enter => match self.operations.tab {
                //
                // Enter focuses the detail pane for Library/Executions
                // (^r runs the op/chain from Library). On the Triggers
                // tab Enter keeps its original toggle-enabled meaning.
                //
                OpsTab::Library | OpsTab::Executions => {
                    self.operations.detail_focus = true;
                    self.operations.detail_scroll = 0;
                }
                OpsTab::Triggers => {
                    self.toggle_selected_trigger_enabled().await;
                }
            },
            KeyCode::Char('r') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                if self.operations.tab == OpsTab::Library {
                    self.open_run_target_popup();
                }
            }
            KeyCode::Char('n')
                if key
                    .modifiers
                    .contains(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
            {
                if self.operations.tab == OpsTab::Library {
                    self.open_new_chain_form();
                }
            }
            KeyCode::Char('n')
                if key.modifiers.contains(KeyModifiers::CONTROL)
                    && !key.modifiers.contains(KeyModifiers::ALT) =>
            {
                match self.operations.tab {
                    OpsTab::Library => self.open_new_op_form(),
                    OpsTab::Triggers => self.open_new_trigger_form(),
                    _ => {}
                }
            }
            KeyCode::Char('e') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                match self.operations.tab {
                    OpsTab::Library => {
                        let filtered = self.filtered_library();
                        match filtered.get(self.operations.library_selected) {
                            Some(&(_, true)) => self.edit_selected_chain(),
                            Some(&(_, false)) => self.edit_selected_op(),
                            None => {}
                        }
                    }
                    OpsTab::Triggers => self.edit_selected_trigger(),
                    _ => {}
                }
            }
            KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                match self.operations.tab {
                    OpsTab::Library => self.delete_selected_library_row().await,
                    OpsTab::Executions => self.delete_selected_execution().await,
                    OpsTab::Triggers => self.delete_selected_trigger().await,
                }
            }
            KeyCode::Char('c')
                if key.modifiers.contains(KeyModifiers::CONTROL)
                    && self.operations.tab == OpsTab::Executions =>
            {
                self.cancel_selected_execution().await;
            }
            KeyCode::Char('x')
                if key.modifiers.contains(KeyModifiers::CONTROL)
                    && self.operations.tab == OpsTab::Executions =>
            {
                self.confirm = Some(ConfirmAction {
                    message: "Clear all executions?".to_string(),
                    action: ConfirmKind::ClearAllExecutions,
                });
            }
            KeyCode::Esc => {
                if self.operations.filter_focused {
                    self.operations.filter_focused = false;
                } else if !self.operations.filter.is_empty() {
                    self.operations.filter.clear();
                    self.operations.library_selected = 0;
                    self.operations.exec_selected = 0;
                }
            }
            KeyCode::Backspace => {
                if self.operations.filter_focused && !self.operations.filter.is_empty() {
                    self.operations.filter.pop();
                    self.operations.library_selected = 0;
                    self.operations.exec_selected = 0;
                }
            }
            KeyCode::Char('/') if !self.operations.detail_focus => {
                //
                // `/` enters filter-input mode; subsequent chars append to
                // the filter until Enter/Esc. Matches the intercept
                // behaviour so both windows share a muscle memory.
                //
                self.operations.filter_focused = true;
            }
            KeyCode::Char(c) => {
                if self.operations.filter_focused && !self.operations.detail_focus {
                    self.operations.filter.push(c);
                    self.operations.library_selected = 0;
                    self.operations.exec_selected = 0;
                }
            }
            _ => {}
        }
    }

    pub fn filtered_library(&self) -> Vec<(usize, bool)> {
        Self::filtered_library_static(
            &self.operations.op_definitions,
            &self.operations.chain_definitions,
            &self.operations.filter,
        )
    }

    pub fn filtered_library_static(
        op_definitions: &[common::OperationDefinitionInfo],
        chain_definitions: &[common::ChainDefinitionInfo],
        filter: &str,
    ) -> Vec<(usize, bool)> {
        let filter = filter.to_lowercase();
        let mut result = Vec::new();

        for (idx, def) in op_definitions.iter().enumerate() {
            if def.disabled {
                continue;
            }
            if filter.is_empty()
                || def.name.to_lowercase().contains(&filter)
                || def.category.to_lowercase().contains(&filter)
                || def.full_name.to_lowercase().contains(&filter)
            {
                result.push((idx, false));
            }
        }

        for (idx, chain) in chain_definitions.iter().enumerate() {
            if chain.disabled {
                continue;
            }
            if filter.is_empty()
                || chain.name.to_lowercase().contains(&filter)
                || chain.category.to_lowercase().contains(&filter)
            {
                result.push((idx, true));
            }
        }

        result
    }

    pub fn sorted_executions(&self) -> Vec<(bool, usize)> {
        let filter = self.operations.filter.to_lowercase();
        let mut entries: Vec<(chrono::DateTime<chrono::Utc>, bool, usize)> = Vec::new();

        for (i, op) in self.operations.operations.iter().enumerate() {
            if !filter.is_empty()
                && !op.spec.name.to_lowercase().contains(&filter)
                && !op.agent_short_name.to_lowercase().contains(&filter)
            {
                continue;
            }
            entries.push((op.start_time, true, i));
        }

        for (i, exec) in self.operations.chain_executions.iter().enumerate() {
            if !filter.is_empty()
                && !exec.chain_name.to_lowercase().contains(&filter)
                && !exec.agent_short_name.to_lowercase().contains(&filter)
            {
                continue;
            }
            entries.push((exec.started_at, false, i));
        }

        entries.sort_by(|a, b| b.0.cmp(&a.0));
        entries
            .into_iter()
            .map(|(_, is_op, idx)| (is_op, idx))
            .collect()
    }

    pub fn sorted_exec_static(
        operations: &[common::SemanticOpUpdate],
        chain_executions: &[common::ChainExecutionUpdate],
        filter: &str,
    ) -> Vec<(bool, usize)> {
        let filter = filter.to_lowercase();
        let mut entries: Vec<(chrono::DateTime<chrono::Utc>, bool, usize)> = Vec::new();

        for (i, op) in operations.iter().enumerate() {
            if !filter.is_empty()
                && !op.spec.name.to_lowercase().contains(&filter)
                && !op.agent_short_name.to_lowercase().contains(&filter)
            {
                continue;
            }
            entries.push((op.start_time, true, i));
        }

        for (i, exec) in chain_executions.iter().enumerate() {
            if !filter.is_empty()
                && !exec.chain_name.to_lowercase().contains(&filter)
                && !exec.agent_short_name.to_lowercase().contains(&filter)
            {
                continue;
            }
            entries.push((exec.started_at, false, i));
        }

        entries.sort_by(|a, b| b.0.cmp(&a.0));
        entries
            .into_iter()
            .map(|(_, is_op, idx)| (is_op, idx))
            .collect()
    }

    pub(crate) fn ops_library_count(&self) -> usize {
        self.filtered_library().len()
    }

    pub(crate) fn open_run_target_popup(&mut self) {
        let filtered = self.filtered_library();
        let Some(&(idx, is_chain)) = filtered.get(self.operations.library_selected) else {
            return;
        };
        let (op_name, chain_id) = if is_chain {
            let chain = &self.operations.chain_definitions[idx];
            (chain.name.clone(), Some(chain.id.clone()))
        } else {
            let op = &self.operations.op_definitions[idx];
            (op.full_name.clone(), None)
        };

        let nodes: Vec<_> = self
            .nodes
            .nodes
            .iter()
            .map(|n| (n.node_id.clone(), n.machine_name.clone(), true))
            .collect();

        let mut agent_names: Vec<String> = Vec::new();
        for node in &self.nodes.nodes {
            for agent in &node.discovered_agents {
                if agent.available && !agent_names.contains(&agent.short_name) {
                    agent_names.push(agent.short_name.clone());
                }
            }
        }
        let agents: Vec<_> = agent_names.into_iter().map(|a| (a, true)).collect();

        if nodes.is_empty() || agents.is_empty() {
            return;
        }

        self.run_options = Some(RunOptions {
            op_name,
            is_chain,
            chain_id,
            nodes,
            agents,
            yolo: false,
            focused_section: 0,
            cursor: 0,
        });
    }

    pub(crate) async fn cancel_selected_execution(&mut self) {
        let sorted = self.sorted_executions();
        let Some(&(is_op, idx)) = sorted.get(self.operations.exec_selected) else {
            return;
        };

        if is_op {
            let op_id = self.operations.operations[idx].operation_id.clone();
            let _ = self.client.cancel_semantic_op(op_id).await;
        } else {
            let exec_id = self.operations.chain_executions[idx].execution_id.clone();
            let _ = self.client.cancel_chain(exec_id).await;
        }
    }

    pub(crate) fn is_finished_semantic_op(status: &common::SemanticOpStatus) -> bool {
        matches!(
            status,
            common::SemanticOpStatus::Completed
                | common::SemanticOpStatus::Failed
                | common::SemanticOpStatus::Cancelled
        )
    }

    pub(crate) fn is_finished_chain_execution(status: &common::ChainExecutionStatus) -> bool {
        matches!(
            status,
            common::ChainExecutionStatus::Completed
                | common::ChainExecutionStatus::Failed
                | common::ChainExecutionStatus::Cancelled
        )
    }

    pub(crate) async fn delete_selected_execution(&mut self) {
        let sorted = self.sorted_executions();
        let Some(&(is_op, idx)) = sorted.get(self.operations.exec_selected) else {
            return;
        };

        if is_op {
            let op_id = self.operations.operations[idx].operation_id.clone();
            let _ = self.client.remove_semantic_op(op_id).await;
            self.operations.operations.remove(idx);
        } else {
            let exec_id = self.operations.chain_executions[idx].execution_id.clone();
            let _ = self.client.remove_chain_execution(exec_id).await;
            self.operations.chain_executions.remove(idx);
        }

        let total = self.sorted_executions().len();
        if total == 0 {
            self.operations.exec_selected = 0;
        } else if self.operations.exec_selected >= total {
            self.operations.exec_selected = total - 1;
        }

        self.refresh_execution_lists_after(Duration::from_millis(300), false);
    }

    pub(crate) fn edit_selected_op(&mut self) {
        let filtered = self.filtered_library();
        if let Some(&(idx, is_chain)) = filtered.get(self.operations.library_selected) {
            if is_chain {
                return;
            }
            let def = &self.operations.op_definitions[idx];
            self.new_op_form = Some(NewOpForm {
                name: def.name.clone(),
                short_name: def.short_name.clone(),
                category: def.category.clone(),
                description: def.description.clone(),
                mode: if def.mode == "agent" { 1 } else { 0 },
                timeout: def.timeout.to_string(),
                iterations: def.agent_iterations.to_string(),
                yolo: def.yolo_mode,
                prompt: def.operation_prompt.clone(),
                focused_field: 0,
            });
        }
    }

    pub(crate) fn open_new_op_form(&mut self) {
        self.new_op_form = Some(NewOpForm {
            name: String::new(),
            short_name: String::new(),
            category: "custom".to_string(),
            description: String::new(),
            mode: 0,
            timeout: "600".to_string(),
            iterations: "10".to_string(),
            yolo: false,
            prompt: String::new(),
            focused_field: 0,
        });
    }

    pub(crate) async fn submit_new_op(&mut self) {
        let form = match self.new_op_form.take() {
            Some(f) => f,
            None => return,
        };

        if form.name.is_empty() || form.short_name.is_empty() {
            return;
        }

        let mode_str = if form.mode == 0 { "one-shot" } else { "agent" };

        let op_def = serde_json::json!({
            "full_name": format!("{}::{}", form.category, form.short_name),
            "category": form.category,
            "short_name": form.short_name,
            "name": form.name,
            "description": form.description,
            "agent_info": "",
            "timeout": form.timeout.parse::<u64>().unwrap_or(60),
            "operation_prompt": form.prompt,
            "mode": mode_str,
            "agent_iterations": form.iterations.parse::<u32>().unwrap_or(5),
            "operation_chain": [],
            "disabled": false,
            "yolo_mode": form.yolo,
            "model_ref": null,
        });

        if let Err(e) = self.client.add_op_def(op_def.to_string()).await {
            if let Some(session) = self.orchestrator.active_session_mut() {
                session.messages.push(ConversationEntry::Error(format!("Failed to add op: {}", e)));
            }
        }

        self.refresh_library_after(Duration::from_millis(300));
    }

    pub(crate) async fn handle_new_op_form_key(&mut self, key: KeyEvent) {
        let visual_order = |form: &NewOpForm| -> Vec<usize> {
            let mut order = vec![0, 1, 2, 3, 4];
            if form.mode == 1 {
                order.push(5);
            }
            order.extend([6, 7, 8]);
            order
        };

        match key.code {
            KeyCode::Esc => {
                self.new_op_form = None;
            }
            KeyCode::Down | KeyCode::Tab => {
                if let Some(ref mut form) = self.new_op_form {
                    let order = visual_order(form);
                    let pos = order
                        .iter()
                        .position(|&f| f == form.focused_field)
                        .unwrap_or(0);
                    let next = (pos + 1) % order.len();
                    form.focused_field = order[next];
                }
            }
            KeyCode::Up | KeyCode::BackTab => {
                if let Some(ref mut form) = self.new_op_form {
                    let order = visual_order(form);
                    let pos = order
                        .iter()
                        .position(|&f| f == form.focused_field)
                        .unwrap_or(0);
                    let prev = if pos > 0 { pos - 1 } else { order.len() - 1 };
                    form.focused_field = order[prev];
                }
            }
            KeyCode::Char(' ') => {
                if let Some(ref mut form) = self.new_op_form {
                    if NewOpForm::is_toggle(form.focused_field) {
                        Self::toggle_new_op_field(form);
                    } else {
                        match form.focused_field {
                            1 => form.name.push(' '),
                            2 => form.short_name.push(' '),
                            3 => form.category.push(' '),
                            4 => form.description.push(' '),
                            5 => form.iterations.push(' '),
                            6 => form.timeout.push(' '),
                            8 => form.prompt.push(' '),
                            _ => {}
                        }
                    }
                }
            }
            KeyCode::Left | KeyCode::Right => {
                if let Some(ref mut form) = self.new_op_form {
                    Self::toggle_new_op_field(form);
                }
            }
            KeyCode::Enter
                if key.modifiers.contains(KeyModifiers::SHIFT)
                    || key.modifiers.contains(KeyModifiers::ALT) =>
            {
                if let Some(ref mut form) = self.new_op_form {
                    if form.focused_field == 8 {
                        form.prompt.push('\n');
                    }
                }
            }
            KeyCode::Char('\n') => {
                if let Some(ref mut form) = self.new_op_form {
                    if form.focused_field == 8 {
                        form.prompt.push('\n');
                    }
                }
            }
            KeyCode::Enter => {
                if let Some(ref mut form) = self.new_op_form {
                    let order = visual_order(form);
                    let pos = order
                        .iter()
                        .position(|&f| f == form.focused_field)
                        .unwrap_or(0);
                    let next = (pos + 1) % order.len();
                    form.focused_field = order[next];
                }
            }
            KeyCode::Char('s') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                let valid = if let Some(ref form) = self.new_op_form {
                    !form.name.is_empty()
                        && !form.short_name.is_empty()
                        && !form.category.is_empty()
                        && !form.prompt.is_empty()
                        && !form.timeout.is_empty()
                } else {
                    false
                };

                if valid {
                    self.submit_new_op().await;
                } else if let Some(ref form) = self.new_op_form {
                    let mut missing = Vec::new();
                    if form.name.is_empty() {
                        missing.push("Name");
                    }
                    if form.short_name.is_empty() {
                        missing.push("Short Name");
                    }
                    if form.category.is_empty() {
                        missing.push("Category");
                    }
                    if form.prompt.is_empty() {
                        missing.push("Prompt");
                    }
                    if form.timeout.is_empty() {
                        missing.push("Timeout");
                    }
                    self.confirm = Some(ConfirmAction {
                        message: format!("Required: {}", missing.join(", ")),
                        action: ConfirmKind::Info,
                    });
                }
            }
            KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                if let Some(ref mut form) = self.new_op_form {
                    if !NewOpForm::is_toggle(form.focused_field) {
                        match form.focused_field {
                            1 => form.name.push(c),
                            2 => form.short_name.push(c),
                            3 => form.category.push(c),
                            4 => form.description.push(c),
                            5 => form.iterations.push(c),
                            6 => form.timeout.push(c),
                            8 => form.prompt.push(c),
                            _ => {}
                        }
                    }
                }
            }
            KeyCode::Backspace => {
                if let Some(ref mut form) = self.new_op_form {
                    match form.focused_field {
                        1 => {
                            form.name.pop();
                        }
                        2 => {
                            form.short_name.pop();
                        }
                        3 => {
                            form.category.pop();
                        }
                        4 => {
                            form.description.pop();
                        }
                        5 => {
                            form.iterations.pop();
                        }
                        6 => {
                            form.timeout.pop();
                        }
                        8 => {
                            form.prompt.pop();
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }

    pub(crate) async fn delete_selected_op(&mut self) {
        let filtered = self.filtered_library();
        let Some(&(idx, is_chain)) = filtered.get(self.operations.library_selected) else {
            return;
        };

        if !is_chain {
            let op = &self.operations.op_definitions[idx];
            let full_name = op.full_name.clone();
            let name = op.name.clone();
            self.confirm = Some(ConfirmAction {
                message: format!("Delete operation \"{}\" ({})?", name, full_name),
                action: ConfirmKind::DeleteOp(full_name),
            });
        }
    }

    pub(crate) async fn delete_selected_library_row(&mut self) {
        let filtered = self.filtered_library();
        let Some(&(idx, is_chain)) = filtered.get(self.operations.library_selected) else {
            return;
        };
        if is_chain {
            let chain = &self.operations.chain_definitions[idx];
            let chain_id = chain.id.clone();
            let name = chain.name.clone();
            self.confirm = Some(ConfirmAction {
                message: format!("Delete chain \"{}\"?", name),
                action: ConfirmKind::DeleteChain(chain_id),
            });
        } else {
            self.delete_selected_op().await;
        }
    }

    pub(crate) async fn execute_run_options(&mut self, opts: RunOptions) {
        let selected_nodes: Vec<String> = opts
            .nodes
            .iter()
            .filter(|(_, _, sel)| *sel)
            .map(|(id, _, _)| id.clone())
            .collect();
        let selected_agents: Vec<String> = opts
            .agents
            .iter()
            .filter(|(_, sel)| *sel)
            .map(|(name, _)| name.clone())
            .collect();

        if selected_nodes.is_empty() || selected_agents.is_empty() {
            return;
        }

        for node_id in &selected_nodes {
            for agent in &selected_agents {
                if opts.is_chain {
                    if let Some(ref chain_id) = opts.chain_id {
                        let _ = self
                            .client
                            .run_chain(chain_id.clone(), node_id.clone(), agent.clone(), None)
                            .await;
                    }
                } else {
                    let _ = self
                        .client
                        .run_semantic_op(node_id.clone(), agent.clone(), opts.op_name.clone(), None)
                        .await;
                }
            }
        }

        self.operations.tab = OpsTab::Executions;
        self.refresh_execution_lists_after(Duration::from_millis(500), false);
    }

    pub(crate) async fn handle_run_options_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => {
                self.run_options = None;
            }
            KeyCode::Tab => {
                if let Some(ref mut opts) = self.run_options {
                    let section_count = if opts.is_chain { 2 } else { 3 };
                    opts.focused_section = (opts.focused_section + 1) % section_count;
                    opts.cursor = 0;
                }
            }
            KeyCode::Up => {
                if let Some(ref mut opts) = self.run_options {
                    if opts.cursor > 0 {
                        opts.cursor -= 1;
                    } else if opts.focused_section > 0 {
                        opts.focused_section -= 1;
                        let prev_max = match opts.focused_section {
                            0 => opts.nodes.len(),
                            1 => opts.agents.len(),
                            _ => 1,
                        };
                        opts.cursor = prev_max.saturating_sub(1);
                    }
                }
            }
            KeyCode::Down => {
                if let Some(ref mut opts) = self.run_options {
                    let section_count = if opts.is_chain { 2 } else { 3 };
                    let max = match opts.focused_section {
                        0 => opts.nodes.len(),
                        1 => opts.agents.len(),
                        _ => 1,
                    };
                    if opts.cursor + 1 < max {
                        opts.cursor += 1;
                    } else if opts.focused_section + 1 < section_count {
                        opts.focused_section += 1;
                        opts.cursor = 0;
                    }
                }
            }
            KeyCode::Char(' ') | KeyCode::Enter => {
                if let Some(ref opts) = self.run_options {
                    let section = opts.focused_section;
                    let cursor = opts.cursor;
                    self.toggle_run_option(section, cursor);
                }
            }
            KeyCode::Char('r') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                if let Some(opts) = self.run_options.take() {
                    self.execute_run_options(opts).await;
                }
            }
            _ => {}
        }
    }

    pub(crate) fn toggle_run_option(&mut self, section: u8, cursor: usize) {
        if let Some(ref mut opts) = self.run_options {
            opts.focused_section = section;
            opts.cursor = cursor;
            match section {
                0 => {
                    if let Some(n) = opts.nodes.get_mut(cursor) {
                        n.2 = !n.2;
                    }
                }
                1 => {
                    if let Some(a) = opts.agents.get_mut(cursor) {
                        a.1 = !a.1;
                    }
                }
                2 => opts.yolo = !opts.yolo,
                _ => {}
            }
        }
    }

    pub(crate) fn toggle_new_op_field(form: &mut NewOpForm) {
        match form.focused_field {
            0 => form.mode = (form.mode + 1) % 2,
            7 => form.yolo = !form.yolo,
            _ => {}
        }
    }

    pub(crate) async fn handle_operations_mouse(&mut self, mouse: MouseEvent, content_area: Rect) {
        let ops_chunks = Layout::vertical([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Min(1),
            Constraint::Length(1),
        ])
        .split(content_area);
        let tabs_area = ops_chunks[0];
        let hints_area = ops_chunks[3];
        let main_area = ops_chunks[2];
        //
        // All three tabs share a single resizable split so the border
        // drag handle and list/detail hit-testing are consistent.
        //
        let pct = self.operations.split_percent.clamp(20, 80);
        let split = Layout::horizontal([
            Constraint::Percentage(pct),
            Constraint::Percentage(100 - pct),
        ])
        .split(main_area);
        let list_area = split[0];
        let detail_area = split[1];
        let detail_inner = Rect::new(
            detail_area.x.saturating_add(1),
            detail_area.y.saturating_add(1),
            detail_area.width.saturating_sub(2),
            detail_area.height.saturating_sub(2),
        );

        match mouse.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                //
                // Tab clicks. Keep label widths in sync with
                // ui::operations::render_tabs so positional matching stays
                // accurate.
                //
                if mouse.row == tabs_area.y {
                    let rel = mouse.column.saturating_sub(tabs_area.x) as i32;
                    let exec_count =
                        self.operations.operations.len() + self.operations.chain_executions.len();
                    let lib_count = self
                        .operations
                        .op_definitions
                        .iter()
                        .filter(|d| !d.disabled)
                        .count()
                        + self
                            .operations
                            .chain_definitions
                            .iter()
                            .filter(|c| !c.disabled)
                            .count();
                    let trig_count = self.operations.triggers.len();

                    //
                    // Column widths mirror ui::operations::render_tabs:
                    // leading "  " (2) + " Executions " (12) + "N " (>=2)
                    // + "  │  " (5) + " Library " (9) + count + sep + ...
                    //
                    let exec_start = 2i32;
                    let exec_width = (" Executions ".len() + format!("{} ", exec_count).len()) as i32;
                    let sep = 5i32;
                    let lib_start = exec_start + exec_width + sep;
                    let lib_width = (" Library ".len() + format!("{} ", lib_count).len()) as i32;
                    let trig_start = lib_start + lib_width + sep;
                    let trig_width = (" Triggers ".len() + format!("{} ", trig_count).len()) as i32;

                    let prev_tab = self.operations.tab;
                    if rel >= exec_start && rel < exec_start + exec_width {
                        self.operations.tab = OpsTab::Executions;
                    } else if rel >= lib_start && rel < lib_start + lib_width {
                        self.operations.tab = OpsTab::Library;
                    } else if rel >= trig_start && rel < trig_start + trig_width {
                        self.operations.tab = OpsTab::Triggers;
                    }
                    if self.operations.tab != prev_tab {
                        self.operations.filter.clear();
                        if self.operations.tab == OpsTab::Triggers {
                            self.refresh_triggers_after(Duration::ZERO);
                        }
                    }
                    return;
                }

                //
                // Hint bar clicks.
                //
                if mouse.row == hints_area.y {
                    let rel = mouse.column.saturating_sub(hints_area.x) as usize;
                    match self.operations.tab {
                        OpsTab::Library => {
                            //
                            // " ^r execute    ^n new op    ^! newchain    ^e edit    ^d delete"
                            // Approximate column ranges below; clicks anywhere
                            // inside the hint chip dispatch the action.
                            //
                            if rel < 13 {
                                self.open_run_target_popup();
                            } else if rel < 24 {
                                self.open_new_op_form();
                            } else if rel < 38 {
                                self.open_new_chain_form();
                            } else if rel < 49 {
                                let filtered = self.filtered_library();
                                match filtered.get(self.operations.library_selected) {
                                    Some(&(_, true)) => self.edit_selected_chain(),
                                    Some(&(_, false)) => self.edit_selected_op(),
                                    None => {}
                                }
                            } else if rel < 61 {
                                self.delete_selected_library_row().await;
                            }
                        }
                        OpsTab::Executions => {
                            // Hint text varies, use find-based approach
                            let hint_text = " ^c cancel  ^d delete  ^x clear all  ";
                            if let Some(pos) = hint_text.find("cancel") {
                                let cancel_start = pos.saturating_sub(3);
                                let cancel_end = pos + 6;
                                if rel >= cancel_start && rel < cancel_end + 2 {
                                    self.cancel_selected_execution().await;
                                    return;
                                }
                            }
                            if let Some(pos) = hint_text.find("delete") {
                                let delete_start = pos.saturating_sub(3);
                                let delete_end = pos + 6;
                                if rel >= delete_start && rel < delete_end + 2 {
                                    self.delete_selected_execution().await;
                                    return;
                                }
                            }
                            if let Some(pos) = hint_text.find("clear all") {
                                let clear_start = pos.saturating_sub(3);
                                let clear_end = pos + 9;
                                if rel >= clear_start && rel < clear_end + 2 {
                                    self.confirm = Some(ConfirmAction {
                                        message: "Clear all executions?".to_string(),
                                        action: ConfirmKind::ClearAllExecutions,
                                    });
                                    return;
                                }
                            }
                        }
                        OpsTab::Triggers => {
                            //
                            // " enter toggle  ^n new  ^e edit  ^d delete  "
                            //  0 1    5 6    14 15 16 17   22 23 24 25   31 32 33 34   42
                            //
                            if (1..15).contains(&rel) {
                                self.toggle_selected_trigger_enabled().await;
                            } else if (15..23).contains(&rel) {
                                self.open_new_trigger_form();
                            } else if (23..32).contains(&rel) {
                                self.edit_selected_trigger();
                            } else if (32..43).contains(&rel) {
                                self.delete_selected_trigger().await;
                            }
                        }
                    }
                    return;
                }

                //
                // Pane border drag start — check before list/detail
                // hit-tests so a border-column click doesn't get eaten
                // by the neighbouring pane.
                //
                {
                    let border_rect = Rect {
                        height: main_area.height,
                        y: main_area.y,
                        ..list_area
                    };
                    if crate::ui::common::hit_vertical_border(
                        border_rect,
                        mouse.column,
                        mouse.row,
                    ) {
                        self.operations.dragging = true;
                        return;
                    }
                }

                //
                // List item click (with double-click support).
                //
                if mouse.column >= list_area.x
                    && mouse.column < list_area.x.saturating_add(list_area.width)
                {
                    let list_start_row = list_area.y.saturating_add(2);
                    if mouse.row >= list_start_row
                        && mouse.row < list_area.y.saturating_add(list_area.height)
                    {
                        let clicked_idx = (mouse.row - list_start_row) as usize;
                        let is_dbl = self.is_double_click(mouse.row, mouse.column);
                        match self.operations.tab {
                            OpsTab::Library => {
                                let total = self.ops_library_count();
                                if clicked_idx < total {
                                    self.operations.library_selected = clicked_idx;
                                    self.operations.detail_focus = false;
                                    if is_dbl {
                                        self.open_run_target_popup();
                                    }
                                }
                            }
                            OpsTab::Executions => {
                                let total = self.sorted_executions().len();
                                if clicked_idx < total {
                                    self.operations.exec_selected = clicked_idx;
                                    self.operations.detail_scroll = 0;
                                    self.operations.detail_focus = false;
                                }
                            }
                            OpsTab::Triggers => {
                                let total = self.operations.triggers.len();
                                if clicked_idx < total {
                                    self.operations.trigger_selected = clicked_idx;
                                    self.operations.detail_focus = false;
                                    if is_dbl {
                                        self.edit_selected_trigger();
                                    }
                                }
                            }
                        }
                    }
                    return;
                }

                //
                // Detail pane click.
                //
                if mouse.column >= detail_area.x
                    && mouse.column < detail_area.x.saturating_add(detail_area.width)
                    && mouse.row >= detail_area.y
                    && mouse.row < detail_area.y.saturating_add(detail_area.height)
                {
                    self.operations.detail_focus = true;

                    if self.operations.tab == OpsTab::Executions
                        && mouse.column >= detail_inner.x
                        && mouse.column < detail_inner.x.saturating_add(detail_inner.width)
                        && mouse.row >= detail_inner.y
                        && mouse.row < detail_inner.y.saturating_add(detail_inner.height)
                    {
                        let visual_row = mouse
                            .row
                            .saturating_sub(detail_inner.y)
                            .saturating_add(self.operations.detail_scroll);
                        if let Some(section_idx) =
                            crate::ui::operations::execution_detail_section_at_row(
                                &self.operations,
                                detail_inner.width,
                                visual_row,
                            )
                        {
                            self.operations.collapsed.focused_section = section_idx;
                            if section_idx < self.operations.collapsed.sections.len() {
                                self.operations.collapsed.sections[section_idx] =
                                    !self.operations.collapsed.sections[section_idx];
                            }
                        }
                    }
                    return;
                }

            }
            MouseEventKind::Drag(MouseButton::Left) => {
                if self.operations.dragging {
                    self.operations.split_percent = crate::ui::common::drag_split_percent(
                        0,
                        self.terminal_width,
                        mouse.column,
                    );
                }
            }
            MouseEventKind::Up(MouseButton::Left) => {
                self.operations.dragging = false;
            }
            _ => {}
        }
        return;
    }

    //
    // Trigger actions.
    //

    pub(crate) async fn toggle_selected_trigger_enabled(&mut self) {
        let Some(trigger) = self
            .operations
            .triggers
            .get(self.operations.trigger_selected)
            .cloned()
        else {
            return;
        };
        let _ = self
            .client
            .update_chain_trigger(trigger.id, Some(!trigger.enabled), None, None)
            .await;
        self.refresh_triggers_after(Duration::from_millis(200));
    }

    pub(crate) async fn delete_selected_trigger(&mut self) {
        let Some(trigger) = self
            .operations
            .triggers
            .get(self.operations.trigger_selected)
            .cloned()
        else {
            return;
        };
        let chain_name = self
            .operations
            .chain_definitions
            .iter()
            .find(|c| c.id == trigger.chain_id)
            .map(|c| c.name.clone())
            .unwrap_or_else(|| trigger.chain_id.clone());
        self.confirm = Some(ConfirmAction {
            message: format!("Delete trigger for \"{}\"?", chain_name),
            action: ConfirmKind::DeleteTrigger(trigger.id),
        });
    }

    pub(crate) fn open_new_trigger_form(&mut self) {
        let chains: Vec<(String, String)> = self
            .operations
            .chain_definitions
            .iter()
            .filter(|c| !c.disabled)
            .map(|c| (c.id.clone(), c.name.clone()))
            .collect();
        if chains.is_empty() {
            self.confirm = Some(ConfirmAction {
                message: "No chains available — create a chain first.".to_string(),
                action: ConfirmKind::Info,
            });
            return;
        }

        let rules: Vec<(i64, String)> = self
            .operations
            .intercept_rules
            .iter()
            .map(|r| (r.id, r.name.clone()))
            .collect();

        let nodes: Vec<(String, String, bool)> = self
            .nodes
            .nodes
            .iter()
            .map(|n| (n.node_id.clone(), n.machine_name.clone(), false))
            .collect();

        let mut agent_names: Vec<String> = Vec::new();
        for node in &self.nodes.nodes {
            for agent in &node.discovered_agents {
                if agent.available && !agent_names.contains(&agent.short_name) {
                    agent_names.push(agent.short_name.clone());
                }
            }
        }
        let agents: Vec<(String, bool)> = agent_names.into_iter().map(|a| (a, false)).collect();

        self.trigger_form = Some(TriggerForm {
            editing_id: None,
            chains,
            chain_cursor: 0,
            kind: TriggerKind::Scheduled,
            schedule_kind: ScheduleKind::Interval,
            hour: 0,
            minute: 0,
            interval_minutes: 60,
            recurring: true,
            rules,
            rule_cursor: 0,
            nodes,
            agents,
            os_filter: String::new(),
            include_triggering_node: false,
            focused_section: TriggerFormSection::Chain,
            cursor: 0,
        });
    }

    pub(crate) fn edit_selected_trigger(&mut self) {
        let Some(trigger) = self
            .operations
            .triggers
            .get(self.operations.trigger_selected)
            .cloned()
        else {
            return;
        };

        let chains: Vec<(String, String)> = self
            .operations
            .chain_definitions
            .iter()
            .filter(|c| !c.disabled)
            .map(|c| (c.id.clone(), c.name.clone()))
            .collect();
        let chain_cursor = chains
            .iter()
            .position(|(id, _)| id == &trigger.chain_id)
            .unwrap_or(0);

        let rules: Vec<(i64, String)> = self
            .operations
            .intercept_rules
            .iter()
            .map(|r| (r.id, r.name.clone()))
            .collect();

        let nodes: Vec<(String, String, bool)> = self
            .nodes
            .nodes
            .iter()
            .map(|n| {
                let selected = trigger.target_spec.node_ids.contains(&n.node_id);
                (n.node_id.clone(), n.machine_name.clone(), selected)
            })
            .collect();

        let mut agent_names: Vec<String> = Vec::new();
        for node in &self.nodes.nodes {
            for agent in &node.discovered_agents {
                if agent.available && !agent_names.contains(&agent.short_name) {
                    agent_names.push(agent.short_name.clone());
                }
            }
        }
        //
        // Also include any agent referenced by the spec so the user can see
        // it even if no node is currently online advertising it.
        //
        for a in &trigger.target_spec.agent_short_names {
            if !agent_names.contains(a) {
                agent_names.push(a.clone());
            }
        }
        let agents: Vec<(String, bool)> = agent_names
            .into_iter()
            .map(|a| {
                let sel = trigger.target_spec.agent_short_names.contains(&a);
                (a, sel)
            })
            .collect();

        let (kind, schedule_kind, hour, minute, interval_minutes, recurring, rule_cursor) =
            match &trigger.trigger_config {
                TriggerConfig::Scheduled { schedule, recurring } => {
                    let (sk, h, m, iv) = match schedule {
                        common::ScheduleSpec::DailyAt { hour, minute } => {
                            (ScheduleKind::DailyAt, *hour, *minute, 60)
                        }
                        common::ScheduleSpec::Interval { minutes } => {
                            (ScheduleKind::Interval, 0, 0, *minutes)
                        }
                    };
                    (TriggerKind::Scheduled, sk, h, m, iv, *recurring, 0)
                }
                TriggerConfig::InterceptMatch { rule_id } => {
                    let rc = rules
                        .iter()
                        .position(|(id, _)| id == rule_id)
                        .unwrap_or(0);
                    (
                        TriggerKind::InterceptMatch,
                        ScheduleKind::Interval,
                        0,
                        0,
                        60,
                        true,
                        rc,
                    )
                }
                TriggerConfig::NewNode => (
                    TriggerKind::NewNode,
                    ScheduleKind::Interval,
                    0,
                    0,
                    60,
                    true,
                    0,
                ),
            };

        self.trigger_form = Some(TriggerForm {
            editing_id: Some(trigger.id.clone()),
            chains,
            chain_cursor,
            kind,
            schedule_kind,
            hour,
            minute,
            interval_minutes,
            recurring,
            rules,
            rule_cursor,
            nodes,
            agents,
            os_filter: trigger
                .target_spec
                .os_filter
                .clone()
                .unwrap_or_default(),
            include_triggering_node: trigger.target_spec.include_triggering_node,
            focused_section: TriggerFormSection::Chain,
            cursor: 0,
        });
    }

    pub(crate) async fn submit_trigger_form(&mut self) {
        let Some(form) = self.trigger_form.take() else {
            return;
        };

        let Some((chain_id, _)) = form.chains.get(form.chain_cursor).cloned() else {
            return;
        };

        let trigger_config = match form.kind {
            TriggerKind::Scheduled => {
                let schedule = match form.schedule_kind {
                    ScheduleKind::DailyAt => common::ScheduleSpec::DailyAt {
                        hour: form.hour.min(23),
                        minute: form.minute.min(59),
                    },
                    ScheduleKind::Interval => common::ScheduleSpec::Interval {
                        minutes: form.interval_minutes.max(1),
                    },
                };
                TriggerConfig::Scheduled {
                    schedule,
                    recurring: form.recurring,
                }
            }
            TriggerKind::InterceptMatch => {
                let Some((rule_id, _)) = form.rules.get(form.rule_cursor).cloned() else {
                    self.confirm = Some(ConfirmAction {
                        message: "Pick an intercept rule first.".to_string(),
                        action: ConfirmKind::Info,
                    });
                    //
                    // Restore the form so the user can fix it.
                    //
                    self.trigger_form = Some(form);
                    return;
                };
                TriggerConfig::InterceptMatch { rule_id }
            }
            TriggerKind::NewNode => TriggerConfig::NewNode,
        };

        let target_spec = common::TargetSpec {
            node_ids: form
                .nodes
                .iter()
                .filter(|(_, _, s)| *s)
                .map(|(id, _, _)| id.clone())
                .collect(),
            os_filter: {
                let trimmed = form.os_filter.trim();
                if trimmed.is_empty() {
                    None
                } else {
                    Some(trimmed.to_string())
                }
            },
            agent_short_names: form
                .agents
                .iter()
                .filter(|(_, s)| *s)
                .map(|(a, _)| a.clone())
                .collect(),
            include_triggering_node: form.include_triggering_node,
        };

        let result = if let Some(id) = form.editing_id {
            self.client
                .update_chain_trigger(id, None, Some(trigger_config), Some(target_spec))
                .await
        } else {
            self.client
                .create_chain_trigger(chain_id, trigger_config, target_spec)
                .await
        };

        if let Err(e) = result {
            self.confirm = Some(ConfirmAction {
                message: format!("Trigger save failed: {}", e),
                action: ConfirmKind::Info,
            });
        }
        self.refresh_triggers_after(Duration::from_millis(250));
    }

    pub(crate) async fn handle_trigger_form_key(&mut self, key: KeyEvent) {
        use TriggerFormSection as S;

        match key.code {
            KeyCode::Esc => {
                self.trigger_form = None;
            }
            KeyCode::Char('s') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.submit_trigger_form().await;
            }
            KeyCode::Tab | KeyCode::Down => {
                if let Some(form) = self.trigger_form.as_mut() {
                    Self::advance_trigger_form_cursor(form, 1);
                }
            }
            KeyCode::BackTab | KeyCode::Up => {
                if let Some(form) = self.trigger_form.as_mut() {
                    Self::advance_trigger_form_cursor(form, -1);
                }
            }
            KeyCode::Left | KeyCode::Right => {
                if let Some(form) = self.trigger_form.as_mut() {
                    let delta: i32 = if matches!(key.code, KeyCode::Left) { -1 } else { 1 };
                    Self::tweak_trigger_form_field(form, delta);
                }
            }
            KeyCode::Char(' ') => {
                if let Some(form) = self.trigger_form.as_mut() {
                    Self::toggle_trigger_form_selection(form);
                }
            }
            KeyCode::Enter => {
                if let Some(form) = self.trigger_form.as_mut() {
                    Self::toggle_trigger_form_selection(form);
                }
            }
            KeyCode::Backspace => {
                if let Some(form) = self.trigger_form.as_mut() {
                    match form.focused_section {
                        S::OsFilter => {
                            form.os_filter.pop();
                        }
                        S::ScheduleValueRow if form.kind == TriggerKind::Scheduled => {
                            match form.schedule_kind {
                                ScheduleKind::Interval => {
                                    form.interval_minutes /= 10;
                                    if form.interval_minutes == 0 {
                                        form.interval_minutes = 1;
                                    }
                                }
                                ScheduleKind::DailyAt => {
                                    if form.cursor == 1 {
                                        form.minute /= 10;
                                    } else {
                                        form.hour /= 10;
                                    }
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
            KeyCode::Char(c) => {
                if let Some(form) = self.trigger_form.as_mut() {
                    match form.focused_section {
                        S::OsFilter => {
                            form.os_filter.push(c);
                        }
                        S::ScheduleValueRow
                            if form.kind == TriggerKind::Scheduled && c.is_ascii_digit() =>
                        {
                            let d = c.to_digit(10).unwrap();
                            match form.schedule_kind {
                                ScheduleKind::Interval => {
                                    let next = form.interval_minutes.saturating_mul(10) + d;
                                    form.interval_minutes = next.max(1);
                                }
                                ScheduleKind::DailyAt => {
                                    if form.cursor == 1 {
                                        let next = (form.minute as u32) * 10 + d;
                                        form.minute = (next.min(59)) as u8;
                                    } else {
                                        let next = (form.hour as u32) * 10 + d;
                                        form.hour = (next.min(23)) as u8;
                                    }
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }

    fn advance_trigger_form_cursor(form: &mut TriggerForm, delta: i32) {
        let order = form.section_order();
        let current = order
            .iter()
            .position(|s| *s == form.focused_section)
            .unwrap_or(0);
        let next = ((current as i32 + delta).rem_euclid(order.len() as i32)) as usize;
        form.focused_section = order[next];
        form.cursor = 0;
    }

    //
    // Tweak: left/right on a focused row. For pickers (Chain, Type, Rule,
    // ScheduleKindRow) cycles through options; for scalar values (hour,
    // minute, interval) nudges by 1; for list sections (Nodes, Agents)
    // moves the cursor.
    //
    fn tweak_trigger_form_field(form: &mut TriggerForm, delta: i32) {
        use TriggerFormSection as S;
        match form.focused_section {
            S::Chain => {
                if form.chains.is_empty() {
                    return;
                }
                let n = form.chains.len() as i32;
                form.chain_cursor =
                    (((form.chain_cursor as i32) + delta).rem_euclid(n)) as usize;
            }
            S::Type => {
                let variants = [
                    TriggerKind::Scheduled,
                    TriggerKind::InterceptMatch,
                    TriggerKind::NewNode,
                ];
                let idx = variants
                    .iter()
                    .position(|k| *k == form.kind)
                    .unwrap_or(0) as i32;
                let n = variants.len() as i32;
                let next = (idx + delta).rem_euclid(n) as usize;
                form.kind = variants[next];
                form.cursor = 0;
            }
            S::ScheduleKindRow => {
                form.schedule_kind = match form.schedule_kind {
                    ScheduleKind::Interval => ScheduleKind::DailyAt,
                    ScheduleKind::DailyAt => ScheduleKind::Interval,
                };
                form.cursor = 0;
            }
            S::ScheduleValueRow => match form.schedule_kind {
                ScheduleKind::Interval => {
                    let next = (form.interval_minutes as i32).saturating_add(delta).max(1);
                    form.interval_minutes = next as u32;
                }
                ScheduleKind::DailyAt => {
                    if form.cursor == 0 {
                        let h = (form.hour as i32 + delta).rem_euclid(24) as u8;
                        form.hour = h;
                    } else {
                        let m = (form.minute as i32 + delta).rem_euclid(60) as u8;
                        form.minute = m;
                    }
                }
            },
            S::Recurring => {
                form.recurring = !form.recurring;
            }
            S::Rule => {
                if form.rules.is_empty() {
                    return;
                }
                let n = form.rules.len() as i32;
                form.rule_cursor =
                    (((form.rule_cursor as i32) + delta).rem_euclid(n)) as usize;
            }
            S::Nodes => {
                if form.nodes.is_empty() {
                    return;
                }
                let n = form.nodes.len() as i32;
                form.cursor = (((form.cursor as i32) + delta).rem_euclid(n)) as usize;
            }
            S::Agents => {
                if form.agents.is_empty() {
                    return;
                }
                let n = form.agents.len() as i32;
                form.cursor = (((form.cursor as i32) + delta).rem_euclid(n)) as usize;
            }
            S::OsFilter => {}
            S::IncludeTriggering => {
                form.include_triggering_node = !form.include_triggering_node;
            }
        }
    }

    //
    // Space / enter on a focused row: for lists toggles the current item;
    // for toggles flips the value.
    //
    pub(crate) fn toggle_trigger_form_selection(form: &mut TriggerForm) {
        use TriggerFormSection as S;
        match form.focused_section {
            S::Nodes => {
                if let Some(n) = form.nodes.get_mut(form.cursor) {
                    n.2 = !n.2;
                }
            }
            S::Agents => {
                if let Some(a) = form.agents.get_mut(form.cursor) {
                    a.1 = !a.1;
                }
            }
            S::Recurring => form.recurring = !form.recurring,
            S::IncludeTriggering => {
                form.include_triggering_node = !form.include_triggering_node;
            }
            //
            // For scalar pickers, toggle treats Enter like →.
            //
            _ => Self::tweak_trigger_form_field(form, 1),
        }
    }
}
