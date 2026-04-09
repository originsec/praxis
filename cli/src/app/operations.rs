use super::*;

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

            tokio::time::sleep(Duration::from_millis(300)).await;

            let op_definitions = client.get_operation_definitions().await;
            let chain_definitions = client.get_chain_definitions().await;
            let operations = client.get_operations().await;
            let chain_executions = client.get_chain_executions().await;

            let _ = tx.send(AppEvent::OperationsRefreshed {
                op_definitions,
                chain_definitions,
                operations,
                chain_executions,
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
                    self.operations.detail_scroll =
                        self.operations.detail_scroll.saturating_add(10);
                }
                _ => {}
            }
            return;
        }

        match key.code {
            KeyCode::Tab | KeyCode::BackTab => {
                self.operations.tab = match self.operations.tab {
                    OpsTab::Library => OpsTab::Executions,
                    OpsTab::Executions => OpsTab::Library,
                };
                self.operations.filter.clear();
            }
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
            },
            KeyCode::Right => {
                self.operations.detail_focus = true;
                self.operations.detail_scroll = 0;
            }
            KeyCode::Enter => {
                if self.operations.tab == OpsTab::Library {
                    self.open_run_target_popup();
                } else {
                    self.operations.detail_focus = true;
                    self.operations.detail_scroll = 0;
                }
            }
            KeyCode::Char('n') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                if self.operations.tab == OpsTab::Library {
                    self.open_new_op_form();
                }
            }
            KeyCode::Char('e') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                if self.operations.tab == OpsTab::Library {
                    self.edit_selected_op();
                }
            }
            KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                match self.operations.tab {
                    OpsTab::Library => self.delete_selected_op().await,
                    OpsTab::Executions => self.delete_selected_execution().await,
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
                if !self.operations.filter.is_empty() {
                    self.operations.filter.clear();
                    self.operations.library_selected = 0;
                    self.operations.exec_selected = 0;
                }
            }
            KeyCode::Backspace => {
                if !self.operations.filter.is_empty() && !self.operations.detail_focus {
                    self.operations.filter.pop();
                    self.operations.library_selected = 0;
                    self.operations.exec_selected = 0;
                }
            }
            KeyCode::Char(c) => {
                if !self.operations.detail_focus {
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
            self.orchestrator
                .messages
                .push(ConversationEntry::Error(format!("Failed to add op: {}", e)));
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
}
