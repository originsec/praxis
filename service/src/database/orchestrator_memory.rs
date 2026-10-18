use anyhow::Result;

use super::Database;
use super::exec::{Arg, db_args};

//
// Persistent orchestrator memory, scoped by engagement. Each memory is a
// titled note; writing an existing title replaces it.
//

#[derive(Debug, Clone, PartialEq)]
pub struct MemoryEntry {
    pub title: String,
    pub content: String,
    pub updated_at: String,
}

impl Database {
    pub async fn write_orchestrator_memory(
        &self,
        engagement: &str,
        title: &str,
        content: &str,
    ) -> Result<()> {
        let now = chrono::Utc::now().to_rfc3339();
        let id = uuid::Uuid::new_v4().to_string();
        self.db_execute(
            "INSERT INTO orchestrator_memories (id, engagement, title, content, created_at, updated_at) \
             VALUES ($1, $2, $3, $4, $5, $6) \
             ON CONFLICT(engagement, title) DO UPDATE SET content = $4, updated_at = $6",
            db_args![&id, engagement, title, content, &now, &now],
        )
        .await?;
        Ok(())
    }

    pub async fn read_orchestrator_memory(
        &self,
        engagement: &str,
        title: &str,
    ) -> Result<Option<MemoryEntry>> {
        let row = self
            .db_fetch_optional(
                "SELECT title, content, updated_at FROM orchestrator_memories \
                 WHERE engagement = $1 AND title = $2",
                db_args![engagement, title],
            )
            .await?;
        Ok(row.map(|r| MemoryEntry {
            title: r.get(0),
            content: r.get(1),
            updated_at: r.get(2),
        }))
    }

    //
    // Most recently updated first. Content is returned too so callers can
    // build snippets.
    //
    pub async fn list_orchestrator_memories(
        &self,
        engagement: &str,
        limit: i64,
    ) -> Result<Vec<MemoryEntry>> {
        let rows = self
            .db_fetch_all(
                "SELECT title, content, updated_at FROM orchestrator_memories \
                 WHERE engagement = $1 ORDER BY updated_at DESC LIMIT $2",
                db_args![engagement, limit],
            )
            .await?;
        Ok(rows
            .into_iter()
            .map(|r| MemoryEntry {
                title: r.get(0),
                content: r.get(1),
                updated_at: r.get(2),
            })
            .collect())
    }

    //
    // Case-insensitive search: every whitespace-separated term must appear
    // in the title or the content.
    //
    pub async fn search_orchestrator_memories(
        &self,
        engagement: &str,
        query: &str,
        limit: i64,
    ) -> Result<Vec<MemoryEntry>> {
        let terms: Vec<String> = query
            .split_whitespace()
            .map(|t| format!("%{}%", escape_like(&t.to_lowercase())))
            .collect();

        let mut sql = String::from(
            "SELECT title, content, updated_at FROM orchestrator_memories WHERE engagement = $1",
        );
        let mut args: Vec<Arg> = db_args![engagement];
        for term in terms {
            let n = args.len() + 1;
            sql.push_str(&format!(
                " AND (LOWER(title) LIKE ${n} ESCAPE '\\' OR LOWER(content) LIKE ${n} ESCAPE '\\')"
            ));
            args.push(Arg::from(term));
        }
        let n = args.len() + 1;
        sql.push_str(&format!(" ORDER BY updated_at DESC LIMIT ${n}"));
        args.push(Arg::from(limit));

        let rows = self.db_fetch_all(&sql, args).await?;
        Ok(rows
            .into_iter()
            .map(|r| MemoryEntry {
                title: r.get(0),
                content: r.get(1),
                updated_at: r.get(2),
            })
            .collect())
    }
}

fn escape_like(term: &str) -> String {
    term.replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_")
}
