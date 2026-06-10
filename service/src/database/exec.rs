use anyhow::Result;
use chrono::{DateTime, Utc};
use sqlx::Row;

use super::{Database, DatabasePool};

//
// Shared query-execution layer for the database module. Owns the
// SQLite-vs-PostgreSQL match so that entity modules can be written once:
// queries take a single SQL string with $N placeholders, bind arguments via
// `Arg`, and parse results from backend-agnostic `DbRow`s. The only semantic
// difference between the backends — how boolean flags and insert ids are
// handled — is captured here in `Arg::Bool`, `DbRow::get_bool` and
// `insert_returning_id`.
//

/// A row from either backend, decodable via a single parse function.
pub(crate) enum DbRow {
    Sqlite(sqlx::sqlite::SqliteRow),
    Postgres(sqlx::postgres::PgRow),
}

impl DbRow {
    /// Get a column value by index or name, for types that decode
    /// identically on both backends (i64, i32, String, Option<...>,
    /// Vec<u8>, ...).
    pub fn get<'r, T, I>(&'r self, index: I) -> T
    where
        T: sqlx::Decode<'r, sqlx::Sqlite>
            + sqlx::Type<sqlx::Sqlite>
            + sqlx::Decode<'r, sqlx::Postgres>
            + sqlx::Type<sqlx::Postgres>,
        I: sqlx::ColumnIndex<sqlx::sqlite::SqliteRow> + sqlx::ColumnIndex<sqlx::postgres::PgRow>,
    {
        match self {
            DbRow::Sqlite(row) => row.get(index),
            DbRow::Postgres(row) => row.get(index),
        }
    }

    /// Decode a boolean flag column. Flag columns are INTEGER on SQLite and
    /// SMALLINT (occasionally INTEGER) on PostgreSQL, so this tries the
    /// integer widths each backend actually produces.
    pub fn get_bool<I>(&self, index: I) -> bool
    where
        I: sqlx::ColumnIndex<sqlx::sqlite::SqliteRow>
            + sqlx::ColumnIndex<sqlx::postgres::PgRow>
            + Clone,
    {
        match self {
            DbRow::Sqlite(row) => row
                .try_get::<i64, _>(index.clone())
                .map(|v| v != 0)
                .unwrap_or_else(|_| row.get::<bool, _>(index)),
            DbRow::Postgres(row) => row
                .try_get::<i16, _>(index.clone())
                .map(|v| v != 0)
                .or_else(|_| row.try_get::<i32, _>(index.clone()).map(|v| v != 0))
                .unwrap_or_else(|_| row.get::<bool, _>(index)),
        }
    }

    /// Decode an RFC 3339 timestamp column stored as TEXT.
    pub fn get_timestamp<I>(&self, index: I) -> Result<DateTime<Utc>>
    where
        I: sqlx::ColumnIndex<sqlx::sqlite::SqliteRow> + sqlx::ColumnIndex<sqlx::postgres::PgRow>,
    {
        let raw: String = self.get(index);
        Ok(DateTime::parse_from_rfc3339(&raw)?.with_timezone(&Utc))
    }
}

/// A bind argument usable on either backend. Booleans bind as i64 on SQLite
/// and i16 on PostgreSQL, matching the INTEGER / SMALLINT flag columns in
/// the schemas.
pub(crate) enum Arg {
    I64(i64),
    OptI64(Option<i64>),
    I32(i32),
    OptI32(Option<i32>),
    F64(f64),
    Str(String),
    OptStr(Option<String>),
    Bool(bool),
    Bytes(Vec<u8>),
    OptBytes(Option<Vec<u8>>),
}

impl From<i64> for Arg {
    fn from(v: i64) -> Self {
        Arg::I64(v)
    }
}
impl From<Option<i64>> for Arg {
    fn from(v: Option<i64>) -> Self {
        Arg::OptI64(v)
    }
}
impl From<i32> for Arg {
    fn from(v: i32) -> Self {
        Arg::I32(v)
    }
}
impl From<Option<i32>> for Arg {
    fn from(v: Option<i32>) -> Self {
        Arg::OptI32(v)
    }
}
impl From<f64> for Arg {
    fn from(v: f64) -> Self {
        Arg::F64(v)
    }
}
impl From<&str> for Arg {
    fn from(v: &str) -> Self {
        Arg::Str(v.to_string())
    }
}
impl From<String> for Arg {
    fn from(v: String) -> Self {
        Arg::Str(v)
    }
}
impl From<&String> for Arg {
    fn from(v: &String) -> Self {
        Arg::Str(v.clone())
    }
}
impl From<Option<&str>> for Arg {
    fn from(v: Option<&str>) -> Self {
        Arg::OptStr(v.map(|s| s.to_string()))
    }
}
impl From<Option<String>> for Arg {
    fn from(v: Option<String>) -> Self {
        Arg::OptStr(v)
    }
}
impl From<bool> for Arg {
    fn from(v: bool) -> Self {
        Arg::Bool(v)
    }
}
impl From<Vec<u8>> for Arg {
    fn from(v: Vec<u8>) -> Self {
        Arg::Bytes(v)
    }
}
impl From<Option<Vec<u8>>> for Arg {
    fn from(v: Option<Vec<u8>>) -> Self {
        Arg::OptBytes(v)
    }
}
impl From<DateTime<Utc>> for Arg {
    fn from(v: DateTime<Utc>) -> Self {
        Arg::Str(v.to_rfc3339())
    }
}
impl From<&DateTime<Utc>> for Arg {
    fn from(v: &DateTime<Utc>) -> Self {
        Arg::Str(v.to_rfc3339())
    }
}

/// Build a `Vec<Arg>` from heterogeneous values via `Into<Arg>`.
macro_rules! db_args {
    ($($value:expr),* $(,)?) => {
        vec![$(crate::database::exec::Arg::from($value)),*]
    };
}
pub(crate) use db_args;

fn bind_sqlite<'q>(
    mut query: sqlx::query::Query<'q, sqlx::Sqlite, sqlx::sqlite::SqliteArguments<'q>>,
    args: &'q [Arg],
) -> sqlx::query::Query<'q, sqlx::Sqlite, sqlx::sqlite::SqliteArguments<'q>> {
    for arg in args {
        query = match arg {
            Arg::I64(v) => query.bind(v),
            Arg::OptI64(v) => query.bind(v),
            Arg::I32(v) => query.bind(v),
            Arg::OptI32(v) => query.bind(v),
            Arg::F64(v) => query.bind(v),
            Arg::Str(v) => query.bind(v),
            Arg::OptStr(v) => query.bind(v),
            Arg::Bool(v) => query.bind(if *v { 1i64 } else { 0i64 }),
            Arg::Bytes(v) => query.bind(v.as_slice()),
            Arg::OptBytes(v) => query.bind(v.as_deref()),
        };
    }
    query
}

fn bind_postgres<'q>(
    mut query: sqlx::query::Query<'q, sqlx::Postgres, sqlx::postgres::PgArguments>,
    args: &'q [Arg],
) -> sqlx::query::Query<'q, sqlx::Postgres, sqlx::postgres::PgArguments> {
    for arg in args {
        query = match arg {
            Arg::I64(v) => query.bind(v),
            Arg::OptI64(v) => query.bind(v),
            Arg::I32(v) => query.bind(v),
            Arg::OptI32(v) => query.bind(v),
            Arg::F64(v) => query.bind(v),
            Arg::Str(v) => query.bind(v),
            Arg::OptStr(v) => query.bind(v),
            Arg::Bool(v) => query.bind(if *v { 1i16 } else { 0i16 }),
            Arg::Bytes(v) => query.bind(v.as_slice()),
            Arg::OptBytes(v) => query.bind(v.as_deref()),
        };
    }
    query
}

impl Database {
    /// Fetch all rows for a query.
    pub(crate) async fn db_fetch_all(&self, sql: &str, args: Vec<Arg>) -> Result<Vec<DbRow>> {
        match &self.pool {
            DatabasePool::Sqlite(pool) => {
                let rows = bind_sqlite(sqlx::query(sql), &args).fetch_all(pool).await?;
                Ok(rows.into_iter().map(DbRow::Sqlite).collect())
            }
            DatabasePool::Postgres(pool) => {
                let rows = bind_postgres(sqlx::query(sql), &args)
                    .fetch_all(pool)
                    .await?;
                Ok(rows.into_iter().map(DbRow::Postgres).collect())
            }
        }
    }

    /// Fetch zero or one row for a query.
    pub(crate) async fn db_fetch_optional(
        &self,
        sql: &str,
        args: Vec<Arg>,
    ) -> Result<Option<DbRow>> {
        match &self.pool {
            DatabasePool::Sqlite(pool) => {
                let row = bind_sqlite(sqlx::query(sql), &args)
                    .fetch_optional(pool)
                    .await?;
                Ok(row.map(DbRow::Sqlite))
            }
            DatabasePool::Postgres(pool) => {
                let row = bind_postgres(sqlx::query(sql), &args)
                    .fetch_optional(pool)
                    .await?;
                Ok(row.map(DbRow::Postgres))
            }
        }
    }

    /// Fetch exactly one row for a query.
    pub(crate) async fn db_fetch_one(&self, sql: &str, args: Vec<Arg>) -> Result<DbRow> {
        match &self.pool {
            DatabasePool::Sqlite(pool) => {
                let row = bind_sqlite(sqlx::query(sql), &args).fetch_one(pool).await?;
                Ok(DbRow::Sqlite(row))
            }
            DatabasePool::Postgres(pool) => {
                let row = bind_postgres(sqlx::query(sql), &args)
                    .fetch_one(pool)
                    .await?;
                Ok(DbRow::Postgres(row))
            }
        }
    }

    /// Execute a statement, returning the number of affected rows.
    pub(crate) async fn db_execute(&self, sql: &str, args: Vec<Arg>) -> Result<u64> {
        match &self.pool {
            DatabasePool::Sqlite(pool) => Ok(bind_sqlite(sqlx::query(sql), &args)
                .execute(pool)
                .await?
                .rows_affected()),
            DatabasePool::Postgres(pool) => Ok(bind_postgres(sqlx::query(sql), &args)
                .execute(pool)
                .await?
                .rows_affected()),
        }
    }

    /// Execute an INSERT and return the generated id. Takes the bare INSERT
    /// statement (no RETURNING clause): SQLite executes it and reads
    /// `last_insert_rowid()`, PostgreSQL appends `RETURNING id`.
    pub(crate) async fn db_insert_returning_id(&self, sql: &str, args: Vec<Arg>) -> Result<i64> {
        match &self.pool {
            DatabasePool::Sqlite(pool) => {
                bind_sqlite(sqlx::query(sql), &args).execute(pool).await?;
                let row = sqlx::query("SELECT last_insert_rowid()")
                    .fetch_one(pool)
                    .await?;
                Ok(row.get::<i64, _>(0))
            }
            DatabasePool::Postgres(pool) => {
                let sql_returning = format!("{} RETURNING id", sql);
                let row = bind_postgres(sqlx::query(&sql_returning), &args)
                    .fetch_one(pool)
                    .await?;
                Ok(row.get::<i64, _>(0))
            }
        }
    }
}
