--
-- PostgreSQL Schema for Praxis Database
--

-- Operations table
CREATE TABLE IF NOT EXISTS operations (
    operation_id TEXT PRIMARY KEY,
    node_id TEXT NOT NULL,
    agent_short_name TEXT NOT NULL DEFAULT '',
    operation_spec TEXT NOT NULL,
    status TEXT NOT NULL,
    start_time TEXT NOT NULL,
    end_time TEXT,
    summary TEXT,
    result TEXT,
    queue_position INTEGER,
    created_at TEXT NOT NULL,
    output TEXT,
    chain_execution_id TEXT
);
CREATE INDEX IF NOT EXISTS idx_operations_node_id ON operations(node_id);
CREATE INDEX IF NOT EXISTS idx_operations_status ON operations(status);
CREATE INDEX IF NOT EXISTS idx_operations_created_at ON operations(created_at);

-- Session transactions table
CREATE TABLE IF NOT EXISTS session_transactions (
    transaction_id TEXT PRIMARY KEY,
    node_id TEXT NOT NULL,
    prompt_text TEXT NOT NULL,
    request_sent_at TEXT NOT NULL,
    response_received_at TEXT,
    response_text TEXT,
    status TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_transactions_node_id ON session_transactions(node_id);
CREATE INDEX IF NOT EXISTS idx_transactions_request_sent_at ON session_transactions(request_sent_at);

-- Operation definitions table
CREATE TABLE IF NOT EXISTS operation_definitions (
    full_name TEXT PRIMARY KEY,
    category TEXT NOT NULL,
    short_name TEXT NOT NULL,
    name TEXT NOT NULL,
    description TEXT NOT NULL,
    agent_info TEXT NOT NULL DEFAULT '',
    timeout BIGINT NOT NULL DEFAULT 60,
    operation_prompt TEXT NOT NULL DEFAULT '',
    mode TEXT NOT NULL DEFAULT 'one-shot',
    agent_iterations BIGINT NOT NULL DEFAULT 5,
    operation_chain TEXT NOT NULL DEFAULT '[]',
    disabled SMALLINT NOT NULL DEFAULT 0,
    yolo_mode SMALLINT NOT NULL DEFAULT 0,
    model_ref TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_op_defs_category ON operation_definitions(category);

-- Intercepted traffic table
CREATE TABLE IF NOT EXISTS intercepted_traffic (
    id BIGSERIAL PRIMARY KEY,
    timestamp TEXT NOT NULL,
    node_id TEXT NOT NULL,
    agent_short_name TEXT NOT NULL,
    intercept_method TEXT NOT NULL DEFAULT 'proxy',
    direction TEXT NOT NULL,
    method TEXT,
    url TEXT NOT NULL,
    host TEXT NOT NULL,
    request_headers TEXT,
    request_body BYTEA,
    response_status INTEGER,
    response_headers TEXT,
    response_body BYTEA,
    created_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_traffic_node_id ON intercepted_traffic(node_id);
CREATE INDEX IF NOT EXISTS idx_traffic_agent ON intercepted_traffic(agent_short_name);
CREATE INDEX IF NOT EXISTS idx_traffic_timestamp ON intercepted_traffic(timestamp);
CREATE INDEX IF NOT EXISTS idx_traffic_host ON intercepted_traffic(host);
CREATE INDEX IF NOT EXISTS idx_traffic_created_at ON intercepted_traffic(created_at);

-- Intercept rules table
CREATE TABLE IF NOT EXISTS intercept_rules (
    id BIGSERIAL PRIMARY KEY,
    name TEXT NOT NULL,
    regex_pattern TEXT NOT NULL,
    target_direction TEXT NOT NULL,
    scope_type TEXT NOT NULL,
    scope_node_id TEXT,
    scope_agent TEXT,
    enabled SMALLINT NOT NULL DEFAULT 1,
    summarization_prompt TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_rules_enabled ON intercept_rules(enabled);

-- Traffic matches table
CREATE TABLE IF NOT EXISTS traffic_matches (
    id BIGSERIAL PRIMARY KEY,
    traffic_id BIGINT NOT NULL REFERENCES intercepted_traffic(id) ON DELETE CASCADE,
    rule_id BIGINT NOT NULL REFERENCES intercept_rules(id) ON DELETE CASCADE,
    matched_at TEXT NOT NULL,
    summary TEXT
);
CREATE INDEX IF NOT EXISTS idx_matches_traffic ON traffic_matches(traffic_id);
CREATE INDEX IF NOT EXISTS idx_matches_rule ON traffic_matches(rule_id);

-- Operation chains table
CREATE TABLE IF NOT EXISTS operation_chains (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    description TEXT NOT NULL,
    category TEXT NOT NULL,
    definition TEXT NOT NULL,
    disabled SMALLINT NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_chains_category ON operation_chains(category);
CREATE INDEX IF NOT EXISTS idx_chains_name ON operation_chains(name);

-- Chain executions table
CREATE TABLE IF NOT EXISTS chain_executions (
    execution_id TEXT PRIMARY KEY,
    chain_id TEXT NOT NULL,
    chain_name TEXT NOT NULL,
    node_id TEXT NOT NULL,
    agent_short_name TEXT NOT NULL,
    status TEXT NOT NULL,
    elements TEXT NOT NULL,
    outputs TEXT NOT NULL,
    started_at TEXT NOT NULL,
    ended_at TEXT,
    created_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_chain_exec_status ON chain_executions(status);
CREATE INDEX IF NOT EXISTS idx_chain_exec_chain_id ON chain_executions(chain_id);
CREATE INDEX IF NOT EXISTS idx_chain_exec_created_at ON chain_executions(created_at);

-- Discovered endpoints table
CREATE TABLE IF NOT EXISTS discovered_endpoints (
    id TEXT PRIMARY KEY,
    node_id TEXT NOT NULL,
    ip_address TEXT NOT NULL,
    domain TEXT,
    port INTEGER NOT NULL,
    is_https SMALLINT NOT NULL,
    models TEXT NOT NULL,
    base_url TEXT NOT NULL,
    api_key TEXT,
    discovered_at TEXT NOT NULL,
    created_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_discovered_node ON discovered_endpoints(node_id);
CREATE INDEX IF NOT EXISTS idx_discovered_at ON discovered_endpoints(discovered_at);

-- Event log table
CREATE TABLE IF NOT EXISTS event_log (
    id BIGSERIAL PRIMARY KEY,
    source TEXT NOT NULL,
    source_id TEXT NOT NULL DEFAULT '',
    level TEXT NOT NULL,
    message TEXT NOT NULL,
    target TEXT,
    timestamp TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT NOW()::TEXT
);
CREATE INDEX IF NOT EXISTS idx_event_log_source ON event_log(source);
CREATE INDEX IF NOT EXISTS idx_event_log_level ON event_log(source, level);
CREATE INDEX IF NOT EXISTS idx_event_log_timestamp ON event_log(timestamp DESC);

-- Recon results table
CREATE TABLE IF NOT EXISTS recon_results (
    id TEXT PRIMARY KEY,
    node_id TEXT NOT NULL,
    agent_short_name TEXT NOT NULL,
    is_semantic SMALLINT NOT NULL,
    tools_json TEXT NOT NULL,
    config_json TEXT NOT NULL,
    sessions_json TEXT NOT NULL,
    project_paths_json TEXT NOT NULL,
    metadata_json TEXT,
    performed_at TEXT NOT NULL,
    created_at TEXT NOT NULL,
    CONSTRAINT idx_recon_unique_agent UNIQUE (node_id, agent_short_name)
);
CREATE INDEX IF NOT EXISTS idx_recon_node_agent ON recon_results(node_id, agent_short_name);
CREATE INDEX IF NOT EXISTS idx_recon_performed_at ON recon_results(performed_at DESC);

-- Service configuration table (key-value store)
CREATE TABLE IF NOT EXISTS service_config (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

--
-- AgentChat tables - IRC-style multi-agent chat system.
--

-- AgentChat sessions (one active at a time)
CREATE TABLE IF NOT EXISTS agent_chat_sessions (
    id TEXT PRIMARY KEY,
    goal TEXT,
    status TEXT NOT NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

-- AgentChat agents participating in session
CREATE TABLE IF NOT EXISTS agent_chat_agents (
    id TEXT PRIMARY KEY,
    agent_chat_session_id TEXT NOT NULL REFERENCES agent_chat_sessions(id) ON DELETE CASCADE,
    node_id TEXT NOT NULL,
    agent_short_name TEXT NOT NULL,
    nickname TEXT NOT NULL,
    precedence INTEGER NOT NULL,
    current_channel_id TEXT,
    status TEXT NOT NULL,
    agent_session_id TEXT,
    created_at TEXT NOT NULL,
    UNIQUE(agent_chat_session_id, node_id)
);
CREATE INDEX IF NOT EXISTS idx_agent_chat_agents_session ON agent_chat_agents(agent_chat_session_id);

-- AgentChat channels
CREATE TABLE IF NOT EXISTS agent_chat_channels (
    id TEXT PRIMARY KEY,
    agent_chat_session_id TEXT NOT NULL REFERENCES agent_chat_sessions(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    topic TEXT,
    created_by TEXT NOT NULL,
    created_at TEXT NOT NULL,
    UNIQUE(agent_chat_session_id, name)
);

-- AgentChat messages (channel + DM)
CREATE TABLE IF NOT EXISTS agent_chat_messages (
    id BIGSERIAL PRIMARY KEY,
    agent_chat_session_id TEXT NOT NULL REFERENCES agent_chat_sessions(id) ON DELETE CASCADE,
    channel_id TEXT,
    sender_nickname TEXT NOT NULL,
    recipient_nickname TEXT,
    message_type TEXT NOT NULL,
    content TEXT NOT NULL,
    timestamp TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_agent_chat_messages_channel ON agent_chat_messages(channel_id);
CREATE INDEX IF NOT EXISTS idx_agent_chat_messages_timestamp ON agent_chat_messages(timestamp);

-- Lua agent scripts (centrally managed by service)
CREATE TABLE IF NOT EXISTS lua_agent_scripts (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    script TEXT NOT NULL,
    disabled SMALLINT NOT NULL DEFAULT 0,
    is_builtin SMALLINT NOT NULL DEFAULT 0,
    version TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
)
