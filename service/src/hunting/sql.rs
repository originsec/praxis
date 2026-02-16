use std::sync::Arc;

use anyhow::{anyhow, Result};
use serde_json::Value;
use sqlx::Row;

use super::parser::ast::{Expr, Literal};
use super::tables::VirtualTable;
use crate::database::{Database, DatabasePool};

//
// SQL parameter types for dynamic query binding.
//

#[derive(Debug, Clone)]
pub enum SqlParam {
    String(String),
    Int(i64),
    Float(f64),
}

//
// SQL-backed table configuration. Tables that can be queried directly via SQL
// declare their schema here so the generic materializer can build and execute
// the query without per-table special-casing.
//

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SqlColumnType {
    Text,
    Integer,
    Blob,
}

#[derive(Debug, Clone)]
pub struct SqlColumn {
    pub kql_name: &'static str,
    pub sql_expr: &'static str,
    pub col_type: SqlColumnType,
    #[allow(dead_code)]
    pub nullable: bool,
}

#[derive(Debug, Clone)]
pub struct SqlTableConfig {
    pub from_clause: &'static str,
    pub columns: Vec<SqlColumn>,
    pub order_by: &'static str,
}

impl VirtualTable {
    pub fn sql_config(&self) -> Option<SqlTableConfig> {
        match self {
            VirtualTable::EventLogs => Some(SqlTableConfig {
                from_clause: "event_log",
                columns: vec![
                    SqlColumn { kql_name: "timestamp", sql_expr: "timestamp", col_type: SqlColumnType::Text, nullable: false },
                    SqlColumn { kql_name: "source", sql_expr: "source", col_type: SqlColumnType::Text, nullable: false },
                    SqlColumn { kql_name: "source_id", sql_expr: "source_id", col_type: SqlColumnType::Text, nullable: false },
                    SqlColumn { kql_name: "level", sql_expr: "level", col_type: SqlColumnType::Text, nullable: false },
                    SqlColumn { kql_name: "target", sql_expr: "target", col_type: SqlColumnType::Text, nullable: true },
                    SqlColumn { kql_name: "message", sql_expr: "message", col_type: SqlColumnType::Text, nullable: false },
                ],
                order_by: "timestamp DESC",
            }),

            VirtualTable::TrafficLogs => Some(SqlTableConfig {
                from_clause: "intercepted_traffic",
                columns: vec![
                    SqlColumn { kql_name: "timestamp", sql_expr: "timestamp", col_type: SqlColumnType::Text, nullable: false },
                    SqlColumn { kql_name: "traffic_id", sql_expr: "id", col_type: SqlColumnType::Integer, nullable: false },
                    SqlColumn { kql_name: "node_id", sql_expr: "node_id", col_type: SqlColumnType::Text, nullable: false },
                    SqlColumn { kql_name: "agent_short_name", sql_expr: "agent_short_name", col_type: SqlColumnType::Text, nullable: false },
                    SqlColumn { kql_name: "intercept_method", sql_expr: "intercept_method", col_type: SqlColumnType::Text, nullable: false },
                    SqlColumn { kql_name: "direction", sql_expr: "direction", col_type: SqlColumnType::Text, nullable: false },
                    SqlColumn { kql_name: "method", sql_expr: "method", col_type: SqlColumnType::Text, nullable: true },
                    SqlColumn { kql_name: "url", sql_expr: "url", col_type: SqlColumnType::Text, nullable: false },
                    SqlColumn { kql_name: "host", sql_expr: "host", col_type: SqlColumnType::Text, nullable: false },
                    SqlColumn { kql_name: "request_headers", sql_expr: "request_headers", col_type: SqlColumnType::Text, nullable: true },
                    SqlColumn { kql_name: "request_body", sql_expr: "request_body", col_type: SqlColumnType::Blob, nullable: true },
                    SqlColumn { kql_name: "response_status", sql_expr: "response_status", col_type: SqlColumnType::Integer, nullable: true },
                    SqlColumn { kql_name: "response_headers", sql_expr: "response_headers", col_type: SqlColumnType::Text, nullable: true },
                    SqlColumn { kql_name: "response_body", sql_expr: "response_body", col_type: SqlColumnType::Blob, nullable: true },
                ],
                order_by: "timestamp DESC",
            }),

            VirtualTable::TrafficMatchLogs => Some(SqlTableConfig {
                from_clause: "traffic_matches tm \
                    JOIN intercepted_traffic it ON tm.traffic_id = it.id \
                    JOIN intercept_rules ir ON tm.rule_id = ir.id",
                columns: vec![
                    SqlColumn { kql_name: "timestamp", sql_expr: "tm.matched_at", col_type: SqlColumnType::Text, nullable: false },
                    SqlColumn { kql_name: "traffic_id", sql_expr: "tm.traffic_id", col_type: SqlColumnType::Integer, nullable: false },
                    SqlColumn { kql_name: "node_id", sql_expr: "it.node_id", col_type: SqlColumnType::Text, nullable: false },
                    SqlColumn { kql_name: "agent_short_name", sql_expr: "it.agent_short_name", col_type: SqlColumnType::Text, nullable: false },
                    SqlColumn { kql_name: "rule_id", sql_expr: "tm.rule_id", col_type: SqlColumnType::Integer, nullable: false },
                    SqlColumn { kql_name: "rule_name", sql_expr: "ir.name", col_type: SqlColumnType::Text, nullable: false },
                    SqlColumn { kql_name: "summary", sql_expr: "tm.summary", col_type: SqlColumnType::Text, nullable: true },
                    SqlColumn { kql_name: "method", sql_expr: "it.method", col_type: SqlColumnType::Text, nullable: true },
                    SqlColumn { kql_name: "url", sql_expr: "it.url", col_type: SqlColumnType::Text, nullable: false },
                    SqlColumn { kql_name: "host", sql_expr: "it.host", col_type: SqlColumnType::Text, nullable: false },
                    SqlColumn { kql_name: "direction", sql_expr: "it.direction", col_type: SqlColumnType::Text, nullable: false },
                    SqlColumn { kql_name: "response_status", sql_expr: "it.response_status", col_type: SqlColumnType::Integer, nullable: true },
                ],
                order_by: "tm.matched_at DESC",
            }),

            _ => None,
        }
    }
}

//
// Translate a KQL where expression into a SQL WHERE clause with positional
// parameters. Returns the SQL fragment and bound parameter values. The
// col_map closure maps KQL column names to SQL column expressions.
//

pub fn build_sql_where(
    where_exprs: &[&Expr],
    col_map: &dyn Fn(&str) -> Option<String>,
    param_offset: usize,
) -> Result<(String, Vec<SqlParam>)> {
    let mut params = Vec::new();
    let mut conditions = Vec::new();
    let mut idx = param_offset;

    for expr in where_exprs {
        let (sql, new_params) = expr_to_sql(expr, &mut idx, col_map)?;
        conditions.push(sql);
        params.extend(new_params);
    }

    let clause = if conditions.is_empty() {
        String::new()
    } else {
        conditions.join(" AND ")
    };

    Ok((clause, params))
}

fn expr_to_sql(
    expr: &Expr,
    idx: &mut usize,
    col_map: &dyn Fn(&str) -> Option<String>,
) -> Result<(String, Vec<SqlParam>)> {
    match expr {
        Expr::Equals(l, r) => binary_op(l, r, "=", idx, col_map),
        Expr::NotEquals(l, r) => binary_op(l, r, "!=", idx, col_map),
        Expr::Less(l, r) => binary_op(l, r, "<", idx, col_map),
        Expr::Greater(l, r) => binary_op(l, r, ">", idx, col_map),
        Expr::LessOrEqual(l, r) => binary_op(l, r, "<=", idx, col_map),
        Expr::GreaterOrEqual(l, r) => binary_op(l, r, ">=", idx, col_map),

        Expr::And(l, r) => {
            let (ls, lp) = expr_to_sql(l, idx, col_map)?;
            let (rs, rp) = expr_to_sql(r, idx, col_map)?;
            Ok((format!("({} AND {})", ls, rs), [lp, rp].concat()))
        }

        Expr::Or(l, r) => {
            let (ls, lp) = expr_to_sql(l, idx, col_map)?;
            let (rs, rp) = expr_to_sql(r, idx, col_map)?;
            Ok((format!("({} OR {})", ls, rs), [lp, rp].concat()))
        }

        Expr::Add(l, r) => binary_op(l, r, "+", idx, col_map),
        Expr::Substract(l, r) => binary_op(l, r, "-", idx, col_map),
        Expr::Multiply(l, r) => binary_op(l, r, "*", idx, col_map),
        Expr::Divide(l, r) => binary_op(l, r, "/", idx, col_map),
        Expr::Modulo(l, r) => binary_op(l, r, "%", idx, col_map),

        Expr::Ident(name) => {
            let sql_col = col_map(name)
                .ok_or_else(|| anyhow!("Column '{}' cannot be pushed to SQL", name))?;
            Ok((sql_col, vec![]))
        }

        Expr::Literal(lit) => literal_to_sql(lit, idx),

        Expr::Func(name, args) => func_to_sql(name, args, idx, col_map),

        _ => Err(anyhow!("Expression cannot be translated to SQL")),
    }
}

fn binary_op(
    l: &Expr, r: &Expr, op: &str,
    idx: &mut usize,
    col_map: &dyn Fn(&str) -> Option<String>,
) -> Result<(String, Vec<SqlParam>)> {
    let (ls, lp) = expr_to_sql(l, idx, col_map)?;
    let (rs, rp) = expr_to_sql(r, idx, col_map)?;
    Ok((format!("({} {} {})", ls, op, rs), [lp, rp].concat()))
}

fn literal_to_sql(lit: &Literal, idx: &mut usize) -> Result<(String, Vec<SqlParam>)> {
    match lit {
        Literal::String(s) => {
            *idx += 1;
            Ok((format!("${}", *idx), vec![SqlParam::String(s.clone())]))
        }
        Literal::Int(Some(n)) => {
            *idx += 1;
            Ok((format!("${}", *idx), vec![SqlParam::Int(*n as i64)]))
        }
        Literal::Long(Some(n)) => {
            *idx += 1;
            Ok((format!("${}", *idx), vec![SqlParam::Int(*n)]))
        }
        Literal::Real(Some(n)) => {
            *idx += 1;
            Ok((format!("${}", *idx), vec![SqlParam::Float(*n as f64)]))
        }
        Literal::Decimal(Some(n)) => {
            *idx += 1;
            Ok((format!("${}", *idx), vec![SqlParam::Float(*n)]))
        }
        Literal::Bool(Some(b)) => Ok((if *b { "1" } else { "0" }.to_string(), vec![])),
        Literal::Bool(None) | Literal::Int(None) | Literal::Long(None)
        | Literal::Real(None) | Literal::Decimal(None) => Ok(("NULL".to_string(), vec![])),
        _ => Err(anyhow!("Unsupported literal type for SQL")),
    }
}

//
// Translate KQL function calls to SQL. Case-insensitive string functions use
// LOWER() since KQL matching is case-insensitive by default. LIKE patterns
// escape user values to prevent wildcard injection.
//

fn func_to_sql(
    name: &str,
    args: &[Expr],
    idx: &mut usize,
    col_map: &dyn Fn(&str) -> Option<String>,
) -> Result<(String, Vec<SqlParam>)> {
    match name.to_lowercase().as_str() {
        "contains" | "has" => like_func(args, idx, col_map, "%{}%", false),
        "!contains" | "!has" | "notcontains" => like_func(args, idx, col_map, "%{}%", true),
        "startswith" => like_func(args, idx, col_map, "{}%", false),
        "endswith" => like_func(args, idx, col_map, "%{}", false),

        "isnull" | "isempty" => {
            let (col_sql, col_params) = single_arg_sql(args, idx, col_map)?;
            Ok((format!("({} IS NULL OR {} = '')", col_sql, col_sql), col_params))
        }
        "isnotnull" | "isnotempty" => {
            let (col_sql, col_params) = single_arg_sql(args, idx, col_map)?;
            Ok((format!("({} IS NOT NULL AND {} != '')", col_sql, col_sql), col_params))
        }
        "tolower" => {
            let (inner, p) = single_arg_sql(args, idx, col_map)?;
            Ok((format!("LOWER({})", inner), p))
        }
        "toupper" => {
            let (inner, p) = single_arg_sql(args, idx, col_map)?;
            Ok((format!("UPPER({})", inner), p))
        }
        "strlen" => {
            let (inner, p) = single_arg_sql(args, idx, col_map)?;
            Ok((format!("LENGTH({})", inner), p))
        }
        "tostring" => {
            let (inner, p) = single_arg_sql(args, idx, col_map)?;
            Ok((format!("CAST({} AS TEXT)", inner), p))
        }
        "toint" | "tolong" => {
            let (inner, p) = single_arg_sql(args, idx, col_map)?;
            Ok((format!("CAST({} AS INTEGER)", inner), p))
        }
        "now" => {
            *idx += 1;
            let now = chrono::Utc::now().to_rfc3339();
            Ok((format!("${}", *idx), vec![SqlParam::String(now)]))
        }
        _ => Err(anyhow!("Function '{}' cannot be translated to SQL", name)),
    }
}

fn like_func(
    args: &[Expr],
    idx: &mut usize,
    col_map: &dyn Fn(&str) -> Option<String>,
    pattern_fmt: &str,
    negate: bool,
) -> Result<(String, Vec<SqlParam>)> {
    if args.len() != 2 {
        return Err(anyhow!("LIKE function requires exactly 2 arguments"));
    }
    let (col_sql, mut params) = expr_to_sql(&args[0], idx, col_map)?;

    //
    // Extract the literal value for the LIKE pattern. Escape SQL wildcards
    // in the user value so % and _ are matched literally.
    //

    let val = match &args[1] {
        Expr::Literal(Literal::String(s)) => s.clone(),
        _ => return Err(anyhow!("LIKE pattern must be a string literal")),
    };

    let escaped = val
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_");
    let pattern = pattern_fmt.replace("{}", &escaped).to_lowercase();

    *idx += 1;
    params.push(SqlParam::String(pattern));

    let op = if negate { "NOT LIKE" } else { "LIKE" };
    Ok((format!("LOWER({}) {} ${} ESCAPE '\\'", col_sql, op, *idx), params))
}

fn single_arg_sql(
    args: &[Expr],
    idx: &mut usize,
    col_map: &dyn Fn(&str) -> Option<String>,
) -> Result<(String, Vec<SqlParam>)> {
    if args.is_empty() {
        return Err(anyhow!("Function requires at least 1 argument"));
    }
    expr_to_sql(&args[0], idx, col_map)
}

//
// Generic SQL materializer. Builds and executes a SELECT query for any
// SQL-backed virtual table, converting rows to serde_json Values.
//

pub async fn materialize_sql_table(
    database: &Arc<Database>,
    config: &SqlTableConfig,
    where_clause: &str,
    params: &[SqlParam],
    limit: usize,
) -> Result<(Vec<String>, Vec<Vec<Value>>)> {
    let kql_columns: Vec<String> = config.columns.iter().map(|c| c.kql_name.to_string()).collect();
    let select_exprs: Vec<&str> = config.columns.iter().map(|c| c.sql_expr).collect();

    let mut sql = format!(
        "SELECT {} FROM {}",
        select_exprs.join(", "),
        config.from_clause,
    );

    if !where_clause.is_empty() {
        sql.push_str(" WHERE ");
        sql.push_str(where_clause);
    }

    sql.push_str(&format!(" ORDER BY {} LIMIT {}", config.order_by, limit));

    let rows = match &database.pool {
        DatabasePool::Sqlite(pool) => {
            let mut query = sqlx::query(&sql);
            for p in params {
                query = match p {
                    SqlParam::String(s) => query.bind(s.as_str()),
                    SqlParam::Int(i) => query.bind(*i),
                    SqlParam::Float(f) => query.bind(*f),
                };
            }
            let db_rows = query.fetch_all(pool).await?;
            convert_rows_sqlite(&db_rows, &config.columns)
        }
        DatabasePool::Postgres(pool) => {
            let mut query = sqlx::query(&sql);
            for p in params {
                query = match p {
                    SqlParam::String(s) => query.bind(s.as_str()),
                    SqlParam::Int(i) => query.bind(*i),
                    SqlParam::Float(f) => query.bind(*f),
                };
            }
            let db_rows = query.fetch_all(pool).await?;
            convert_rows_postgres(&db_rows, &config.columns)
        }
    };

    Ok((kql_columns, rows))
}

fn convert_rows_sqlite(
    db_rows: &[sqlx::sqlite::SqliteRow],
    columns: &[SqlColumn],
) -> Vec<Vec<Value>> {
    db_rows.iter().map(|row| {
        columns.iter().enumerate().map(|(i, col)| {
            match col.col_type {
                SqlColumnType::Text => {
                    row.try_get::<Option<String>, _>(i)
                        .ok()
                        .flatten()
                        .map(Value::String)
                        .unwrap_or(Value::Null)
                }
                SqlColumnType::Integer => {
                    row.try_get::<Option<i64>, _>(i)
                        .ok()
                        .flatten()
                        .map(|n| Value::Number(n.into()))
                        .unwrap_or(Value::Null)
                }
                SqlColumnType::Blob => {
                    row.try_get::<Option<Vec<u8>>, _>(i)
                        .ok()
                        .flatten()
                        .map(|b| Value::String(String::from_utf8_lossy(&b).to_string()))
                        .unwrap_or(Value::Null)
                }
            }
        }).collect()
    }).collect()
}

fn convert_rows_postgres(
    db_rows: &[sqlx::postgres::PgRow],
    columns: &[SqlColumn],
) -> Vec<Vec<Value>> {
    db_rows.iter().map(|row| {
        columns.iter().enumerate().map(|(i, col)| {
            match col.col_type {
                SqlColumnType::Text => {
                    row.try_get::<Option<String>, _>(i)
                        .ok()
                        .flatten()
                        .map(Value::String)
                        .unwrap_or(Value::Null)
                }
                SqlColumnType::Integer => {
                    row.try_get::<Option<i64>, _>(i)
                        .ok()
                        .flatten()
                        .map(|n| Value::Number(n.into()))
                        .unwrap_or(Value::Null)
                }
                SqlColumnType::Blob => {
                    row.try_get::<Option<Vec<u8>>, _>(i)
                        .ok()
                        .flatten()
                        .map(|b| Value::String(String::from_utf8_lossy(&b).to_string()))
                        .unwrap_or(Value::Null)
                }
            }
        }).collect()
    }).collect()
}
