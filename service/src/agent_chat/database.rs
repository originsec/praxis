use anyhow::Result;
use chrono::{DateTime, Utc};
use sqlx::Row;

use crate::database::{Database, DatabasePool};

//
// AgentChat database operations.
//

/// Session record from database
#[derive(Debug, Clone)]
pub struct AgentChatSessionRecord {
    pub id: String,
    pub goal: Option<String>,
    pub status: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Agent record from database
#[derive(Debug, Clone)]
pub struct AgentChatAgentRecord {
    pub id: String,
    pub agent_chat_session_id: String,
    pub node_id: String,
    pub agent_short_name: String,
    pub nickname: String,
    pub precedence: i32,
    pub current_channel_id: Option<String>,
    pub status: String,
    pub agent_session_id: Option<String>,
    pub created_at: DateTime<Utc>,
}

/// Channel record from database
#[derive(Debug, Clone)]
pub struct AgentChatChannelRecord {
    pub id: String,
    pub agent_chat_session_id: String,
    pub name: String,
    pub topic: Option<String>,
    pub created_by: String,
    pub created_at: DateTime<Utc>,
}

/// Message record from database
#[derive(Debug, Clone)]
pub struct AgentChatMessageRecord {
    pub id: i64,
    pub agent_chat_session_id: String,
    pub channel_id: Option<String>,
    pub sender_nickname: String,
    pub recipient_nickname: Option<String>,
    pub message_type: String,
    pub content: String,
    pub timestamp: DateTime<Utc>,
}

impl Database {
    //
    // Session operations.
    //

    /// Create a new AgentChat session
    pub async fn create_agent_chat_session(&self, id: &str, goal: Option<&str>) -> Result<()> {
        let now = Utc::now();
        let sql = "INSERT INTO agent_chat_sessions (id, goal, status, created_at, updated_at)
                   VALUES ($1, $2, $3, $4, $5)";

        match &self.pool {
            DatabasePool::Sqlite(pool) => {
                sqlx::query(sql)
                    .bind(id)
                    .bind(goal)
                    .bind("active")
                    .bind(now.to_rfc3339())
                    .bind(now.to_rfc3339())
                    .execute(pool)
                    .await?;
            }
            DatabasePool::Postgres(pool) => {
                sqlx::query(sql)
                    .bind(id)
                    .bind(goal)
                    .bind("active")
                    .bind(now.to_rfc3339())
                    .bind(now.to_rfc3339())
                    .execute(pool)
                    .await?;
            }
        }
        Ok(())
    }

    /// Get the active AgentChat session
    pub async fn get_active_agent_chat_session(&self) -> Result<Option<AgentChatSessionRecord>> {
        let sql = "SELECT id, goal, status, created_at, updated_at
                   FROM agent_chat_sessions WHERE status = 'active' LIMIT 1";

        match &self.pool {
            DatabasePool::Sqlite(pool) => {
                let row_opt = sqlx::query(sql).fetch_optional(pool).await?;
                if let Some(row) = row_opt {
                    Ok(Some(AgentChatSessionRecord {
                        id: row.get(0),
                        goal: row.get(1),
                        status: row.get(2),
                        created_at: DateTime::parse_from_rfc3339(row.get::<String, _>(3).as_str())?
                            .with_timezone(&Utc),
                        updated_at: DateTime::parse_from_rfc3339(row.get::<String, _>(4).as_str())?
                            .with_timezone(&Utc),
                    }))
                } else {
                    Ok(None)
                }
            }
            DatabasePool::Postgres(pool) => {
                let row_opt = sqlx::query(sql).fetch_optional(pool).await?;
                if let Some(row) = row_opt {
                    Ok(Some(AgentChatSessionRecord {
                        id: row.get(0),
                        goal: row.get(1),
                        status: row.get(2),
                        created_at: DateTime::parse_from_rfc3339(row.get::<String, _>(3).as_str())?
                            .with_timezone(&Utc),
                        updated_at: DateTime::parse_from_rfc3339(row.get::<String, _>(4).as_str())?
                            .with_timezone(&Utc),
                    }))
                } else {
                    Ok(None)
                }
            }
        }
    }

    /// Get a AgentChat session by ID
    pub async fn get_agent_chat_session(&self, id: &str) -> Result<Option<AgentChatSessionRecord>> {
        let sql = "SELECT id, goal, status, created_at, updated_at
                   FROM agent_chat_sessions WHERE id = $1";

        match &self.pool {
            DatabasePool::Sqlite(pool) => {
                let row_opt = sqlx::query(sql).bind(id).fetch_optional(pool).await?;
                if let Some(row) = row_opt {
                    Ok(Some(AgentChatSessionRecord {
                        id: row.get(0),
                        goal: row.get(1),
                        status: row.get(2),
                        created_at: DateTime::parse_from_rfc3339(row.get::<String, _>(3).as_str())?
                            .with_timezone(&Utc),
                        updated_at: DateTime::parse_from_rfc3339(row.get::<String, _>(4).as_str())?
                            .with_timezone(&Utc),
                    }))
                } else {
                    Ok(None)
                }
            }
            DatabasePool::Postgres(pool) => {
                let row_opt = sqlx::query(sql).bind(id).fetch_optional(pool).await?;
                if let Some(row) = row_opt {
                    Ok(Some(AgentChatSessionRecord {
                        id: row.get(0),
                        goal: row.get(1),
                        status: row.get(2),
                        created_at: DateTime::parse_from_rfc3339(row.get::<String, _>(3).as_str())?
                            .with_timezone(&Utc),
                        updated_at: DateTime::parse_from_rfc3339(row.get::<String, _>(4).as_str())?
                            .with_timezone(&Utc),
                    }))
                } else {
                    Ok(None)
                }
            }
        }
    }

    /// Update session status
    pub async fn update_agent_chat_session_status(&self, id: &str, status: &str) -> Result<()> {
        let sql = "UPDATE agent_chat_sessions SET status = $1, updated_at = $2 WHERE id = $3";
        let now = Utc::now();

        match &self.pool {
            DatabasePool::Sqlite(pool) => {
                sqlx::query(sql)
                    .bind(status)
                    .bind(now.to_rfc3339())
                    .bind(id)
                    .execute(pool)
                    .await?;
            }
            DatabasePool::Postgres(pool) => {
                sqlx::query(sql)
                    .bind(status)
                    .bind(now.to_rfc3339())
                    .bind(id)
                    .execute(pool)
                    .await?;
            }
        }
        Ok(())
    }

    //
    // Agent operations.
    //

    /// Add an agent to a AgentChat session
    pub async fn add_agent_chat_agent(
        &self,
        id: &str,
        session_id: &str,
        node_id: &str,
        agent_short_name: &str,
        nickname: &str,
        precedence: i32,
    ) -> Result<()> {
        let now = Utc::now();
        let sql = "INSERT INTO agent_chat_agents (id, agent_chat_session_id, node_id, agent_short_name, nickname, precedence, status, created_at)
                   VALUES ($1, $2, $3, $4, $5, $6, $7, $8)";

        match &self.pool {
            DatabasePool::Sqlite(pool) => {
                sqlx::query(sql)
                    .bind(id)
                    .bind(session_id)
                    .bind(node_id)
                    .bind(agent_short_name)
                    .bind(nickname)
                    .bind(precedence)
                    .bind("initializing")
                    .bind(now.to_rfc3339())
                    .execute(pool)
                    .await?;
            }
            DatabasePool::Postgres(pool) => {
                sqlx::query(sql)
                    .bind(id)
                    .bind(session_id)
                    .bind(node_id)
                    .bind(agent_short_name)
                    .bind(nickname)
                    .bind(precedence)
                    .bind("initializing")
                    .bind(now.to_rfc3339())
                    .execute(pool)
                    .await?;
            }
        }
        Ok(())
    }

    /// Get all agents for a AgentChat session
    pub async fn get_agent_chat_agents(
        &self,
        session_id: &str,
    ) -> Result<Vec<AgentChatAgentRecord>> {
        let sql = "SELECT id, agent_chat_session_id, node_id, agent_short_name, nickname, precedence, current_channel_id, status, agent_session_id, created_at
                   FROM agent_chat_agents WHERE agent_chat_session_id = $1 ORDER BY precedence";

        let mut agents = Vec::new();

        match &self.pool {
            DatabasePool::Sqlite(pool) => {
                let rows = sqlx::query(sql).bind(session_id).fetch_all(pool).await?;
                for row in rows {
                    agents.push(AgentChatAgentRecord {
                        id: row.get(0),
                        agent_chat_session_id: row.get(1),
                        node_id: row.get(2),
                        agent_short_name: row.get(3),
                        nickname: row.get(4),
                        precedence: row.get(5),
                        current_channel_id: row.get(6),
                        status: row.get(7),
                        agent_session_id: row.get(8),
                        created_at: DateTime::parse_from_rfc3339(row.get::<String, _>(9).as_str())?
                            .with_timezone(&Utc),
                    });
                }
            }
            DatabasePool::Postgres(pool) => {
                let rows = sqlx::query(sql).bind(session_id).fetch_all(pool).await?;
                for row in rows {
                    agents.push(AgentChatAgentRecord {
                        id: row.get(0),
                        agent_chat_session_id: row.get(1),
                        node_id: row.get(2),
                        agent_short_name: row.get(3),
                        nickname: row.get(4),
                        precedence: row.get(5),
                        current_channel_id: row.get(6),
                        status: row.get(7),
                        agent_session_id: row.get(8),
                        created_at: DateTime::parse_from_rfc3339(row.get::<String, _>(9).as_str())?
                            .with_timezone(&Utc),
                    });
                }
            }
        }
        Ok(agents)
    }

    /// Update agent status
    pub async fn update_agent_chat_agent_status(&self, id: &str, status: &str) -> Result<()> {
        let sql = "UPDATE agent_chat_agents SET status = $1 WHERE id = $2";

        match &self.pool {
            DatabasePool::Sqlite(pool) => {
                sqlx::query(sql).bind(status).bind(id).execute(pool).await?;
            }
            DatabasePool::Postgres(pool) => {
                sqlx::query(sql).bind(status).bind(id).execute(pool).await?;
            }
        }
        Ok(())
    }

    /// Update agent's current channel
    pub async fn update_agent_chat_agent_channel(
        &self,
        id: &str,
        channel_id: Option<&str>,
    ) -> Result<()> {
        let sql = "UPDATE agent_chat_agents SET current_channel_id = $1 WHERE id = $2";

        match &self.pool {
            DatabasePool::Sqlite(pool) => {
                sqlx::query(sql)
                    .bind(channel_id)
                    .bind(id)
                    .execute(pool)
                    .await?;
            }
            DatabasePool::Postgres(pool) => {
                sqlx::query(sql)
                    .bind(channel_id)
                    .bind(id)
                    .execute(pool)
                    .await?;
            }
        }
        Ok(())
    }

    /// Update agent's session ID (from the node's agent session)
    pub async fn update_agent_chat_agent_session_id(
        &self,
        id: &str,
        agent_session_id: Option<&str>,
    ) -> Result<()> {
        let sql = "UPDATE agent_chat_agents SET agent_session_id = $1 WHERE id = $2";

        match &self.pool {
            DatabasePool::Sqlite(pool) => {
                sqlx::query(sql)
                    .bind(agent_session_id)
                    .bind(id)
                    .execute(pool)
                    .await?;
            }
            DatabasePool::Postgres(pool) => {
                sqlx::query(sql)
                    .bind(agent_session_id)
                    .bind(id)
                    .execute(pool)
                    .await?;
            }
        }
        Ok(())
    }

    /// Update agent precedence values
    pub async fn update_agent_chat_agent_precedence(&self, agent_ids: &[String]) -> Result<()> {
        for (i, agent_id) in agent_ids.iter().enumerate() {
            let sql = "UPDATE agent_chat_agents SET precedence = $1 WHERE id = $2";

            match &self.pool {
                DatabasePool::Sqlite(pool) => {
                    sqlx::query(sql)
                        .bind(i as i32)
                        .bind(agent_id)
                        .execute(pool)
                        .await?;
                }
                DatabasePool::Postgres(pool) => {
                    sqlx::query(sql)
                        .bind(i as i32)
                        .bind(agent_id)
                        .execute(pool)
                        .await?;
                }
            }
        }
        Ok(())
    }

    /// Remove an agent from a AgentChat session
    pub async fn remove_agent_chat_agent(&self, id: &str) -> Result<bool> {
        let sql = "DELETE FROM agent_chat_agents WHERE id = $1";

        let count = match &self.pool {
            DatabasePool::Sqlite(pool) => sqlx::query(sql)
                .bind(id)
                .execute(pool)
                .await?
                .rows_affected(),
            DatabasePool::Postgres(pool) => sqlx::query(sql)
                .bind(id)
                .execute(pool)
                .await?
                .rows_affected(),
        };
        Ok(count > 0)
    }

    //
    // Channel operations.
    //

    /// Create a new channel
    pub async fn create_agent_chat_channel(
        &self,
        id: &str,
        session_id: &str,
        name: &str,
        created_by: &str,
    ) -> Result<()> {
        let now = Utc::now();
        let sql = "INSERT INTO agent_chat_channels (id, agent_chat_session_id, name, created_by, created_at)
                   VALUES ($1, $2, $3, $4, $5)";

        match &self.pool {
            DatabasePool::Sqlite(pool) => {
                sqlx::query(sql)
                    .bind(id)
                    .bind(session_id)
                    .bind(name)
                    .bind(created_by)
                    .bind(now.to_rfc3339())
                    .execute(pool)
                    .await?;
            }
            DatabasePool::Postgres(pool) => {
                sqlx::query(sql)
                    .bind(id)
                    .bind(session_id)
                    .bind(name)
                    .bind(created_by)
                    .bind(now.to_rfc3339())
                    .execute(pool)
                    .await?;
            }
        }
        Ok(())
    }

    /// Get all channels for a session
    pub async fn get_agent_chat_channels(
        &self,
        session_id: &str,
    ) -> Result<Vec<AgentChatChannelRecord>> {
        let sql = "SELECT id, agent_chat_session_id, name, topic, created_by, created_at
                   FROM agent_chat_channels WHERE agent_chat_session_id = $1 ORDER BY name";

        let mut channels = Vec::new();

        match &self.pool {
            DatabasePool::Sqlite(pool) => {
                let rows = sqlx::query(sql).bind(session_id).fetch_all(pool).await?;
                for row in rows {
                    channels.push(AgentChatChannelRecord {
                        id: row.get(0),
                        agent_chat_session_id: row.get(1),
                        name: row.get(2),
                        topic: row.get(3),
                        created_by: row.get(4),
                        created_at: DateTime::parse_from_rfc3339(row.get::<String, _>(5).as_str())?
                            .with_timezone(&Utc),
                    });
                }
            }
            DatabasePool::Postgres(pool) => {
                let rows = sqlx::query(sql).bind(session_id).fetch_all(pool).await?;
                for row in rows {
                    channels.push(AgentChatChannelRecord {
                        id: row.get(0),
                        agent_chat_session_id: row.get(1),
                        name: row.get(2),
                        topic: row.get(3),
                        created_by: row.get(4),
                        created_at: DateTime::parse_from_rfc3339(row.get::<String, _>(5).as_str())?
                            .with_timezone(&Utc),
                    });
                }
            }
        }
        Ok(channels)
    }

    /// Get a channel by name in a session
    pub async fn get_agent_chat_channel_by_name(
        &self,
        session_id: &str,
        name: &str,
    ) -> Result<Option<AgentChatChannelRecord>> {
        let sql = "SELECT id, agent_chat_session_id, name, topic, created_by, created_at
                   FROM agent_chat_channels WHERE agent_chat_session_id = $1 AND name = $2";

        match &self.pool {
            DatabasePool::Sqlite(pool) => {
                let row_opt = sqlx::query(sql)
                    .bind(session_id)
                    .bind(name)
                    .fetch_optional(pool)
                    .await?;
                if let Some(row) = row_opt {
                    Ok(Some(AgentChatChannelRecord {
                        id: row.get(0),
                        agent_chat_session_id: row.get(1),
                        name: row.get(2),
                        topic: row.get(3),
                        created_by: row.get(4),
                        created_at: DateTime::parse_from_rfc3339(row.get::<String, _>(5).as_str())?
                            .with_timezone(&Utc),
                    }))
                } else {
                    Ok(None)
                }
            }
            DatabasePool::Postgres(pool) => {
                let row_opt = sqlx::query(sql)
                    .bind(session_id)
                    .bind(name)
                    .fetch_optional(pool)
                    .await?;
                if let Some(row) = row_opt {
                    Ok(Some(AgentChatChannelRecord {
                        id: row.get(0),
                        agent_chat_session_id: row.get(1),
                        name: row.get(2),
                        topic: row.get(3),
                        created_by: row.get(4),
                        created_at: DateTime::parse_from_rfc3339(row.get::<String, _>(5).as_str())?
                            .with_timezone(&Utc),
                    }))
                } else {
                    Ok(None)
                }
            }
        }
    }

    /// Update channel topic
    pub async fn update_agent_chat_channel_topic(
        &self,
        id: &str,
        topic: Option<&str>,
    ) -> Result<()> {
        let sql = "UPDATE agent_chat_channels SET topic = $1 WHERE id = $2";

        match &self.pool {
            DatabasePool::Sqlite(pool) => {
                sqlx::query(sql).bind(topic).bind(id).execute(pool).await?;
            }
            DatabasePool::Postgres(pool) => {
                sqlx::query(sql).bind(topic).bind(id).execute(pool).await?;
            }
        }
        Ok(())
    }

    //
    // Message operations.
    //

    /// Insert a new message
    pub async fn insert_agent_chat_message(
        &self,
        session_id: &str,
        channel_id: Option<&str>,
        sender_nickname: &str,
        recipient_nickname: Option<&str>,
        message_type: &str,
        content: &str,
    ) -> Result<i64> {
        let now = Utc::now();

        match &self.pool {
            DatabasePool::Sqlite(pool) => {
                let sql = "INSERT INTO agent_chat_messages (agent_chat_session_id, channel_id, sender_nickname, recipient_nickname, message_type, content, timestamp)
                           VALUES ($1, $2, $3, $4, $5, $6, $7)";
                let result = sqlx::query(sql)
                    .bind(session_id)
                    .bind(channel_id)
                    .bind(sender_nickname)
                    .bind(recipient_nickname)
                    .bind(message_type)
                    .bind(content)
                    .bind(now.to_rfc3339())
                    .execute(pool)
                    .await?;
                Ok(result.last_insert_rowid())
            }
            DatabasePool::Postgres(pool) => {
                let sql = "INSERT INTO agent_chat_messages (agent_chat_session_id, channel_id, sender_nickname, recipient_nickname, message_type, content, timestamp)
                           VALUES ($1, $2, $3, $4, $5, $6, $7) RETURNING id";
                let row = sqlx::query(sql)
                    .bind(session_id)
                    .bind(channel_id)
                    .bind(sender_nickname)
                    .bind(recipient_nickname)
                    .bind(message_type)
                    .bind(content)
                    .bind(now.to_rfc3339())
                    .fetch_one(pool)
                    .await?;
                Ok(row.get(0))
            }
        }
    }

    /// Get messages for a channel (or DMs if channel_id is None)
    pub async fn get_agent_chat_messages(
        &self,
        session_id: &str,
        channel_id: Option<&str>,
        limit: u32,
    ) -> Result<Vec<AgentChatMessageRecord>> {
        let (sql, has_channel) = if channel_id.is_some() {
            ("SELECT id, agent_chat_session_id, channel_id, sender_nickname, recipient_nickname, message_type, content, timestamp
              FROM agent_chat_messages WHERE agent_chat_session_id = $1 AND channel_id = $2
              ORDER BY timestamp DESC LIMIT $3", true)
        } else {
            ("SELECT id, agent_chat_session_id, channel_id, sender_nickname, recipient_nickname, message_type, content, timestamp
              FROM agent_chat_messages WHERE agent_chat_session_id = $1 AND channel_id IS NULL
              ORDER BY timestamp DESC LIMIT $2", false)
        };

        let mut messages = Vec::new();

        match &self.pool {
            DatabasePool::Sqlite(pool) => {
                let rows = if has_channel {
                    sqlx::query(sql)
                        .bind(session_id)
                        .bind(channel_id)
                        .bind(limit as i64)
                        .fetch_all(pool)
                        .await?
                } else {
                    sqlx::query(sql)
                        .bind(session_id)
                        .bind(limit as i64)
                        .fetch_all(pool)
                        .await?
                };
                for row in rows {
                    messages.push(AgentChatMessageRecord {
                        id: row.get(0),
                        agent_chat_session_id: row.get(1),
                        channel_id: row.get(2),
                        sender_nickname: row.get(3),
                        recipient_nickname: row.get(4),
                        message_type: row.get(5),
                        content: row.get(6),
                        timestamp: DateTime::parse_from_rfc3339(row.get::<String, _>(7).as_str())?
                            .with_timezone(&Utc),
                    });
                }
            }
            DatabasePool::Postgres(pool) => {
                let rows = if has_channel {
                    sqlx::query(sql)
                        .bind(session_id)
                        .bind(channel_id)
                        .bind(limit as i64)
                        .fetch_all(pool)
                        .await?
                } else {
                    sqlx::query(sql)
                        .bind(session_id)
                        .bind(limit as i64)
                        .fetch_all(pool)
                        .await?
                };
                for row in rows {
                    messages.push(AgentChatMessageRecord {
                        id: row.get(0),
                        agent_chat_session_id: row.get(1),
                        channel_id: row.get(2),
                        sender_nickname: row.get(3),
                        recipient_nickname: row.get(4),
                        message_type: row.get(5),
                        content: row.get(6),
                        timestamp: DateTime::parse_from_rfc3339(row.get::<String, _>(7).as_str())?
                            .with_timezone(&Utc),
                    });
                }
            }
        }

        //
        // Reverse to get chronological order.
        //
        messages.reverse();
        Ok(messages)
    }

    /// Count agents in a channel
    pub async fn count_agent_chat_channel_members(&self, channel_id: &str) -> Result<usize> {
        let sql = "SELECT COUNT(*) FROM agent_chat_agents WHERE current_channel_id = $1";

        let count: i64 = match &self.pool {
            DatabasePool::Sqlite(pool) => {
                let row = sqlx::query(sql).bind(channel_id).fetch_one(pool).await?;
                row.get(0)
            }
            DatabasePool::Postgres(pool) => {
                let row = sqlx::query(sql).bind(channel_id).fetch_one(pool).await?;
                row.get(0)
            }
        };

        Ok(count as usize)
    }
}
