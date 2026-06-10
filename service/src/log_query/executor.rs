use std::sync::Arc;

use anyhow::{Result, anyhow};
use serde_json::Value;
use tokio::sync::RwLock;

use super::parser::ast::{Expr, JoinKey, Literal, Operator, Source, Statement, TabularExpression};
use super::parser::parser::parse;
use super::sql::{build_sql_where, materialize_sql_table};

use crate::config::ServiceConfig;
use crate::config::service_config::{LOG_QUERY_ROW_LIMIT, LOG_QUERY_ROW_LIMIT_DEFAULT};
use crate::database::Database;
use crate::state::NodeRegistry;

use super::tables::{
    VirtualTable, materialize_agent_logs, materialize_node_logs, materialize_recon_logs,
    materialize_recon_session_logs, materialize_recon_tool_logs, materialize_toolkit_actions_log,
    resolve_table,
};

pub struct LogQueryResult {
    pub columns: Vec<String>,
    pub rows: Vec<Vec<Value>>,
    pub total_count: usize,
}

pub async fn execute_log_query(
    query: &str,
    database: &Arc<Database>,
    node_registry: &Arc<NodeRegistry>,
    service_config: &Arc<RwLock<ServiceConfig>>,
) -> Result<LogQueryResult> {
    let statements = parse(query)
        .map(|(_, stmts)| stmts)
        .map_err(|e| anyhow!("KQL parse error: {}", e))?;

    if statements.is_empty() {
        return Err(anyhow!("Empty query"));
    }

    if statements.len() > 1 {
        return Err(anyhow!(
            "Multiple statements not supported; use a single query"
        ));
    }

    let tabular = match &statements[0] {
        Statement::TabularExpression(te) => te,
        Statement::Let(..) => {
            return Err(anyhow!("'let' statements are not supported"));
        }
    };

    //
    // Resolve the source table.
    //

    let table_name = match &tabular.source {
        Source::Reference(name) => name.clone(),
        _ => return Err(anyhow!("Only table references are supported as source")),
    };

    let table = resolve_table(&table_name)
        .ok_or_else(|| anyhow!(
            "Unknown table '{}'. Available tables: TrafficLogs, TrafficMatchLogs, NodeLogs, AgentLogs, ReconLogs, ReconToolLogs, ReconSessionLogs, EventLogs, ToolkitActionsLog, OperationLogs, ChainExecutionLogs",
            table_name
        ))?;

    //
    // Materialize the table. For SQL-backed tables, push all leading where
    // predicates and the first take/limit down to SQL. For in-memory and
    // JSON-expanded tables, fetch the full dataset.
    //

    //
    // Read the configurable row limit from service config.
    //

    let row_cap = {
        let cfg = service_config.read().await;
        cfg.get(LOG_QUERY_ROW_LIMIT)
            .and_then(|s| s.parse().ok())
            .unwrap_or(LOG_QUERY_ROW_LIMIT_DEFAULT)
    };

    let (columns, mut rows) =
        materialize_table(table, database, node_registry, &tabular.operators, row_cap).await?;

    //
    // Apply pipeline operators sequentially. Operators whose predicates were
    // pushed down still run here — they just filter an already-narrowed set,
    // so they're near-free.
    //

    let mut current_columns = columns;

    for operator in &tabular.operators {
        match operator {
            Operator::Where(expr) => {
                validate_column_refs(expr, &current_columns)?;
                rows = rows
                    .into_iter()
                    .filter(|row| eval_where_expr(expr, &current_columns, row))
                    .collect();
            }

            Operator::Project(projections) => {
                for (_, expr) in projections {
                    validate_column_refs(expr, &current_columns)?;
                }

                let proj_names: Vec<String> = projections
                    .iter()
                    .map(|(alias, expr)| {
                        if let Some(name) = alias {
                            name.clone()
                        } else if let Expr::Ident(name) = expr {
                            name.clone()
                        } else {
                            "?".to_string()
                        }
                    })
                    .collect();

                let indices: Vec<Option<usize>> = projections
                    .iter()
                    .map(|(_, expr)| {
                        if let Expr::Ident(name) = expr {
                            current_columns
                                .iter()
                                .position(|c| c.eq_ignore_ascii_case(name))
                        } else {
                            None
                        }
                    })
                    .collect();

                rows = rows
                    .into_iter()
                    .map(|row| {
                        indices
                            .iter()
                            .map(|idx| idx.and_then(|i| row.get(i).cloned()).unwrap_or(Value::Null))
                            .collect()
                    })
                    .collect();

                current_columns = proj_names;
            }

            Operator::Take(n) => {
                let limit = (*n as usize).min(row_cap);
                rows.truncate(limit);
            }

            Operator::Sort(col_names) => {
                if let Some(col_name) = col_names.first() {
                    if let Some(idx) = current_columns
                        .iter()
                        .position(|c| c.eq_ignore_ascii_case(col_name))
                    {
                        rows.sort_by(|a, b| cmp_values(a.get(idx), b.get(idx)));
                    }
                }
            }

            Operator::Extend(extensions) => {
                for (alias, expr) in extensions {
                    let col_name = alias.clone().unwrap_or_else(|| {
                        if let Expr::Ident(n) = expr {
                            n.clone()
                        } else {
                            "extended".to_string()
                        }
                    });
                    current_columns.push(col_name);
                    for row in &mut rows {
                        let val = eval_expr(expr, &current_columns, row);
                        row.push(val);
                    }
                }
            }

            Operator::Count => {
                let count = rows.len();
                current_columns = vec!["count".to_string()];
                rows = vec![vec![Value::Number(count.into())]];
            }

            Operator::Distinct(col_names) => {
                let indices: Vec<usize> = col_names
                    .iter()
                    .filter_map(|name| {
                        current_columns
                            .iter()
                            .position(|c| c.eq_ignore_ascii_case(name))
                    })
                    .collect();

                let mut seen = std::collections::HashSet::new();
                rows = rows
                    .into_iter()
                    .filter(|row| {
                        let key: Vec<String> = indices
                            .iter()
                            .map(|&i| row.get(i).map(|v| v.to_string()).unwrap_or_default())
                            .collect();
                        seen.insert(key)
                    })
                    .collect();
            }

            Operator::Summarize(aggregations, group_by) => {
                apply_summarize(&mut current_columns, &mut rows, aggregations, group_by);
            }

            Operator::ProjectAway(col_names) => {
                let remove_indices: std::collections::HashSet<usize> = col_names
                    .iter()
                    .filter_map(|name| {
                        current_columns
                            .iter()
                            .position(|c| c.eq_ignore_ascii_case(name))
                    })
                    .collect();

                let keep_indices: Vec<usize> = (0..current_columns.len())
                    .filter(|i| !remove_indices.contains(i))
                    .collect();

                current_columns = keep_indices
                    .iter()
                    .map(|&i| current_columns[i].clone())
                    .collect();
                rows = rows
                    .into_iter()
                    .map(|row| {
                        keep_indices
                            .iter()
                            .map(|&i| row.get(i).cloned().unwrap_or(Value::Null))
                            .collect()
                    })
                    .collect();
            }

            Operator::Top(n, sort_expr, asc, nulls_last) => {
                if let Expr::Ident(col_name) = sort_expr {
                    if let Some(idx) = current_columns
                        .iter()
                        .position(|c| c.eq_ignore_ascii_case(col_name))
                    {
                        rows.sort_by(|a, b| {
                            let cmp = cmp_values(a.get(idx), b.get(idx));
                            if *asc { cmp } else { cmp.reverse() }
                        });
                    }
                }
                let _ = nulls_last;
                let limit = (*n as usize).min(row_cap);
                rows.truncate(limit);
            }

            Operator::Join(_options, right_expr, join_keys) => {
                let (right_columns, right_rows) =
                    materialize_tabular_expression(right_expr, database, node_registry, row_cap)
                        .await?;

                apply_join(
                    &mut current_columns,
                    &mut rows,
                    &right_columns,
                    &right_rows,
                    join_keys,
                );
            }

            other => {
                return Err(anyhow!(
                    "Unsupported operator: {:?}",
                    std::mem::discriminant(other)
                ));
            }
        }
    }

    //
    // Apply the hard cap.
    //

    let total_count = rows.len();
    if rows.len() > row_cap {
        rows.truncate(row_cap);
    }

    Ok(LogQueryResult {
        columns: current_columns,
        rows,
        total_count,
    })
}

//
// Materialize a table. SQL-backed tables try to push all leading where/take
// operators down to the database. If any expression can't be translated, the
// table is fetched with just a limit and in-memory operators handle the rest.
//

async fn materialize_table(
    table: VirtualTable,
    database: &Arc<Database>,
    node_registry: &Arc<NodeRegistry>,
    operators: &[Operator],
    row_cap: usize,
) -> Result<(Vec<String>, Vec<Vec<Value>>)> {
    if let Some(config) = table.sql_config() {
        //
        // Build a column name mapper from KQL names to SQL expressions.
        //

        let col_map = |kql_name: &str| -> Option<String> {
            config
                .columns
                .iter()
                .find(|c| c.kql_name.eq_ignore_ascii_case(kql_name))
                .map(|c| c.sql_expr.to_string())
        };

        //
        // Collect leading where expressions and the first take limit,
        // stopping at any column-reshaping operator.
        //

        let mut where_exprs: Vec<&Expr> = Vec::new();
        let mut take_limit: Option<usize> = None;

        for op in operators {
            match op {
                Operator::Where(expr) => where_exprs.push(expr),
                Operator::Take(n) => {
                    let n = (*n as usize).min(row_cap);
                    take_limit = Some(take_limit.map(|prev| prev.min(n)).unwrap_or(n));
                }
                Operator::Sort(_) => {}
                _ => break,
            }
        }

        //
        // Try to translate all where expressions to SQL.
        //

        match build_sql_where(&where_exprs, &col_map, 0) {
            Ok((where_clause, params)) => {
                let limit = take_limit.unwrap_or(row_cap).min(row_cap);
                return materialize_sql_table(database, &config, &where_clause, &params, limit)
                    .await;
            }
            Err(_) => {
                //
                // Fallback: fetch with just a limit, let in-memory handle
                // where filtering.
                //

                return materialize_sql_table(database, &config, "", &[], row_cap).await;
            }
        }
    }

    //
    // Non-SQL tables: in-memory or JSON-expanded from database.
    //

    match table {
        VirtualTable::NodeLogs => Ok(materialize_node_logs(node_registry).await),
        VirtualTable::AgentLogs => Ok(materialize_agent_logs(node_registry).await),
        VirtualTable::ReconLogs => materialize_recon_logs(database).await,
        VirtualTable::ReconToolLogs => materialize_recon_tool_logs(database).await,
        VirtualTable::ReconSessionLogs => materialize_recon_session_logs(database).await,
        VirtualTable::ToolkitActionsLog => materialize_toolkit_actions_log(database).await,
        _ => Err(anyhow!("Table has no materializer")),
    }
}

//
// Materialize a full TabularExpression (source + operators). Used for the
// right-hand side of a join.
//

async fn materialize_tabular_expression(
    expr: &TabularExpression,
    database: &Arc<Database>,
    node_registry: &Arc<NodeRegistry>,
    row_cap: usize,
) -> Result<(Vec<String>, Vec<Vec<Value>>)> {
    let table_name = match &expr.source {
        Source::Reference(name) => name.clone(),
        _ => {
            return Err(anyhow!(
                "Join: only table references are supported as right-side source"
            ));
        }
    };

    let table = resolve_table(&table_name)
        .ok_or_else(|| anyhow!("Join: unknown table '{}'", table_name))?;

    let (mut columns, mut rows) =
        materialize_table(table, database, node_registry, &expr.operators, row_cap).await?;

    //
    // Apply any operators on the right side (e.g. where filters).
    //

    for operator in &expr.operators {
        match operator {
            Operator::Where(filter_expr) => {
                rows = rows
                    .into_iter()
                    .filter(|row| eval_where_expr(filter_expr, &columns, row))
                    .collect();
            }
            Operator::Take(n) => {
                rows.truncate((*n as usize).min(row_cap));
            }
            Operator::Project(projections) => {
                let proj_names: Vec<String> = projections
                    .iter()
                    .map(|(alias, e)| {
                        alias.clone().unwrap_or_else(|| {
                            if let Expr::Ident(name) = e {
                                name.clone()
                            } else {
                                "?".to_string()
                            }
                        })
                    })
                    .collect();
                let indices: Vec<Option<usize>> = projections
                    .iter()
                    .map(|(_, e)| {
                        if let Expr::Ident(name) = e {
                            columns.iter().position(|c| c.eq_ignore_ascii_case(name))
                        } else {
                            None
                        }
                    })
                    .collect();
                rows = rows
                    .into_iter()
                    .map(|row| {
                        indices
                            .iter()
                            .map(|idx| idx.and_then(|i| row.get(i).cloned()).unwrap_or(Value::Null))
                            .collect()
                    })
                    .collect();
                columns = proj_names;
            }
            _ => {}
        }
    }

    Ok((columns, rows))
}

//
// Inner join: for each left row, find matching right rows by key equality and
// produce merged rows with columns from both sides. Right-side columns that
// duplicate a left-side name are prefixed with the right table name or
// suffixed with `1`.
//

fn apply_join(
    left_columns: &mut Vec<String>,
    left_rows: &mut Vec<Vec<Value>>,
    right_columns: &[String],
    right_rows: &[Vec<Value>],
    join_keys: &[JoinKey],
) {
    let left_key_indices: Vec<usize> = join_keys
        .iter()
        .filter_map(|k| {
            left_columns
                .iter()
                .position(|c| c.eq_ignore_ascii_case(&k.left))
        })
        .collect();
    let right_key_indices: Vec<usize> = join_keys
        .iter()
        .filter_map(|k| {
            right_columns
                .iter()
                .position(|c| c.eq_ignore_ascii_case(&k.right))
        })
        .collect();

    if left_key_indices.is_empty() || left_key_indices.len() != right_key_indices.len() {
        return;
    }

    //
    // Determine which right columns to add (skip join key columns that already
    // exist on the left).
    //

    let left_names_lower: std::collections::HashSet<String> =
        left_columns.iter().map(|c| c.to_lowercase()).collect();

    let right_col_mapping: Vec<(usize, String)> = right_columns
        .iter()
        .enumerate()
        .filter(|(_, name)| !left_names_lower.contains(&name.to_lowercase()))
        .map(|(i, name)| (i, name.clone()))
        .collect();

    //
    // Build a lookup index on the right side keyed by join values.
    //

    let mut right_index: std::collections::HashMap<Vec<String>, Vec<usize>> =
        std::collections::HashMap::new();

    for (row_idx, row) in right_rows.iter().enumerate() {
        let key: Vec<String> = right_key_indices
            .iter()
            .map(|&i| row.get(i).map(|v| v.to_string()).unwrap_or_default())
            .collect();
        right_index.entry(key).or_default().push(row_idx);
    }

    //
    // Produce joined rows.
    //

    let mut joined_rows = Vec::new();
    for left_row in left_rows.iter() {
        let left_key: Vec<String> = left_key_indices
            .iter()
            .map(|&i| left_row.get(i).map(|v| v.to_string()).unwrap_or_default())
            .collect();

        if let Some(matching_indices) = right_index.get(&left_key) {
            for &right_idx in matching_indices {
                let right_row = &right_rows[right_idx];
                let mut merged = left_row.clone();
                for (col_idx, _) in &right_col_mapping {
                    merged.push(right_row.get(*col_idx).cloned().unwrap_or(Value::Null));
                }
                joined_rows.push(merged);
            }
        }
    }

    //
    // Update columns and rows.
    //

    for (_, name) in &right_col_mapping {
        left_columns.push(name.clone());
    }
    *left_rows = joined_rows;
}

//
// Validate that all Ident references in an expression exist as columns.
//

fn validate_column_refs(expr: &Expr, columns: &[String]) -> Result<()> {
    match expr {
        Expr::Ident(name) => {
            if !columns.iter().any(|c| c.eq_ignore_ascii_case(name)) {
                return Err(anyhow!(
                    "Unknown column '{}'. Available columns: {}",
                    name,
                    columns.join(", ")
                ));
            }
            Ok(())
        }
        Expr::Equals(l, r)
        | Expr::NotEquals(l, r)
        | Expr::Less(l, r)
        | Expr::Greater(l, r)
        | Expr::LessOrEqual(l, r)
        | Expr::GreaterOrEqual(l, r)
        | Expr::And(l, r)
        | Expr::Or(l, r)
        | Expr::Add(l, r)
        | Expr::Substract(l, r)
        | Expr::Multiply(l, r)
        | Expr::Divide(l, r)
        | Expr::Modulo(l, r)
        | Expr::Index(l, r) => {
            validate_column_refs(l, columns)?;
            validate_column_refs(r, columns)
        }
        Expr::Func(_, args) => {
            for arg in args {
                validate_column_refs(arg, columns)?;
            }
            Ok(())
        }
        Expr::Literal(_) => Ok(()),
    }
}

//
// Expression evaluation for where clauses.
//

fn eval_where_expr(expr: &Expr, columns: &[String], row: &[Value]) -> bool {
    match eval_expr(expr, columns, row) {
        Value::Bool(b) => b,
        _ => false,
    }
}

fn eval_expr(expr: &Expr, columns: &[String], row: &[Value]) -> Value {
    match expr {
        Expr::Ident(name) => columns
            .iter()
            .position(|c| c.eq_ignore_ascii_case(name))
            .and_then(|i| row.get(i))
            .cloned()
            .unwrap_or(Value::Null),

        Expr::Literal(lit) => literal_to_value(lit),

        Expr::Equals(lhs, rhs) => {
            let l = eval_expr(lhs, columns, row);
            let r = eval_expr(rhs, columns, row);
            Value::Bool(values_equal(&l, &r))
        }

        Expr::NotEquals(lhs, rhs) => {
            let l = eval_expr(lhs, columns, row);
            let r = eval_expr(rhs, columns, row);
            Value::Bool(!values_equal(&l, &r))
        }

        Expr::And(lhs, rhs) => {
            let l = eval_where_expr(lhs, columns, row);
            let r = eval_where_expr(rhs, columns, row);
            Value::Bool(l && r)
        }

        Expr::Or(lhs, rhs) => {
            let l = eval_where_expr(lhs, columns, row);
            let r = eval_where_expr(rhs, columns, row);
            Value::Bool(l || r)
        }

        Expr::Less(lhs, rhs) => {
            let l = eval_expr(lhs, columns, row);
            let r = eval_expr(rhs, columns, row);
            Value::Bool(cmp_values(Some(&l), Some(&r)).is_lt())
        }

        Expr::Greater(lhs, rhs) => {
            let l = eval_expr(lhs, columns, row);
            let r = eval_expr(rhs, columns, row);
            Value::Bool(cmp_values(Some(&l), Some(&r)).is_gt())
        }

        Expr::LessOrEqual(lhs, rhs) => {
            let l = eval_expr(lhs, columns, row);
            let r = eval_expr(rhs, columns, row);
            Value::Bool(!cmp_values(Some(&l), Some(&r)).is_gt())
        }

        Expr::GreaterOrEqual(lhs, rhs) => {
            let l = eval_expr(lhs, columns, row);
            let r = eval_expr(rhs, columns, row);
            Value::Bool(!cmp_values(Some(&l), Some(&r)).is_lt())
        }

        Expr::Add(lhs, rhs) => {
            let l = eval_expr(lhs, columns, row);
            let r = eval_expr(rhs, columns, row);
            numeric_op(&l, &r, |a, b| a + b)
        }

        Expr::Substract(lhs, rhs) => {
            let l = eval_expr(lhs, columns, row);
            let r = eval_expr(rhs, columns, row);
            numeric_op(&l, &r, |a, b| a - b)
        }

        Expr::Multiply(lhs, rhs) => {
            let l = eval_expr(lhs, columns, row);
            let r = eval_expr(rhs, columns, row);
            numeric_op(&l, &r, |a, b| a * b)
        }

        Expr::Divide(lhs, rhs) => {
            let l = eval_expr(lhs, columns, row);
            let r = eval_expr(rhs, columns, row);
            numeric_op(&l, &r, |a, b| if b != 0.0 { a / b } else { f64::NAN })
        }

        Expr::Modulo(lhs, rhs) => {
            let l = eval_expr(lhs, columns, row);
            let r = eval_expr(rhs, columns, row);
            numeric_op(&l, &r, |a, b| if b != 0.0 { a % b } else { f64::NAN })
        }

        Expr::Func(name, args) => eval_func(name, args, columns, row),

        Expr::Index(_, _) => Value::Null,
    }
}

fn eval_func(name: &str, args: &[Expr], columns: &[String], row: &[Value]) -> Value {
    let evaluated: Vec<Value> = args.iter().map(|a| eval_expr(a, columns, row)).collect();

    match name.to_lowercase().as_str() {
        "contains" | "has" => {
            if let (Some(Value::String(haystack)), Some(Value::String(needle))) =
                (evaluated.first(), evaluated.get(1))
            {
                Value::Bool(haystack.to_lowercase().contains(&needle.to_lowercase()))
            } else {
                Value::Bool(false)
            }
        }

        "!contains" | "!has" | "notcontains" => {
            if let (Some(Value::String(haystack)), Some(Value::String(needle))) =
                (evaluated.first(), evaluated.get(1))
            {
                Value::Bool(!haystack.to_lowercase().contains(&needle.to_lowercase()))
            } else {
                Value::Bool(true)
            }
        }

        "startswith" => {
            if let (Some(Value::String(s)), Some(Value::String(prefix))) =
                (evaluated.first(), evaluated.get(1))
            {
                Value::Bool(s.to_lowercase().starts_with(&prefix.to_lowercase()))
            } else {
                Value::Bool(false)
            }
        }

        "endswith" => {
            if let (Some(Value::String(s)), Some(Value::String(suffix))) =
                (evaluated.first(), evaluated.get(1))
            {
                Value::Bool(s.to_lowercase().ends_with(&suffix.to_lowercase()))
            } else {
                Value::Bool(false)
            }
        }

        "strlen" => {
            if let Some(Value::String(s)) = evaluated.first() {
                Value::Number(s.len().into())
            } else {
                Value::Null
            }
        }

        "tolower" => {
            if let Some(Value::String(s)) = evaluated.first() {
                Value::String(s.to_lowercase())
            } else {
                Value::Null
            }
        }

        "toupper" => {
            if let Some(Value::String(s)) = evaluated.first() {
                Value::String(s.to_uppercase())
            } else {
                Value::Null
            }
        }

        "isnotempty" | "isnotnull" => {
            if let Some(val) = evaluated.first() {
                Value::Bool(!val.is_null() && val.as_str().map(|s| !s.is_empty()).unwrap_or(true))
            } else {
                Value::Bool(false)
            }
        }

        "isnull" | "isempty" => {
            if let Some(val) = evaluated.first() {
                Value::Bool(val.is_null() || val.as_str().map(|s| s.is_empty()).unwrap_or(false))
            } else {
                Value::Bool(true)
            }
        }

        "now" => Value::String(chrono::Utc::now().to_rfc3339()),

        "count" => {
            // count() as aggregation is handled separately in summarize
            Value::Null
        }

        "tostring" => {
            if let Some(val) = evaluated.first() {
                match val {
                    Value::String(s) => Value::String(s.clone()),
                    other => Value::String(other.to_string()),
                }
            } else {
                Value::Null
            }
        }

        "toint" | "tolong" => {
            if let Some(val) = evaluated.first() {
                match val {
                    Value::Number(n) => Value::Number(n.clone()),
                    Value::String(s) => s
                        .parse::<i64>()
                        .ok()
                        .map(|n| Value::Number(n.into()))
                        .unwrap_or(Value::Null),
                    _ => Value::Null,
                }
            } else {
                Value::Null
            }
        }

        _ => Value::Null,
    }
}

//
// Helper functions.
//

fn literal_to_value(lit: &Literal) -> Value {
    match lit {
        Literal::String(s) => Value::String(s.clone()),
        Literal::Int(Some(n)) => Value::Number((*n).into()),
        Literal::Long(Some(n)) => Value::Number((*n).into()),
        Literal::Real(Some(n)) => serde_json::Number::from_f64(*n as f64)
            .map(Value::Number)
            .unwrap_or(Value::Null),
        Literal::Bool(Some(b)) => Value::Bool(*b),
        Literal::Bool(None) => Value::Null,
        _ => Value::Null,
    }
}

fn values_equal(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::String(a), Value::String(b)) => a.eq_ignore_ascii_case(b),
        (Value::Number(a), Value::Number(b)) => a.as_f64() == b.as_f64(),
        (Value::Bool(a), Value::Bool(b)) => a == b,
        (Value::Null, Value::Null) => true,
        //
        // Cross-type comparisons: try to coerce string to number.
        //
        (Value::String(s), Value::Number(n)) | (Value::Number(n), Value::String(s)) => {
            s.parse::<f64>().ok() == n.as_f64()
        }
        _ => false,
    }
}

fn cmp_values(a: Option<&Value>, b: Option<&Value>) -> std::cmp::Ordering {
    use std::cmp::Ordering;

    match (a, b) {
        (None, None) | (Some(Value::Null), Some(Value::Null)) => Ordering::Equal,
        (None | Some(Value::Null), _) => Ordering::Less,
        (_, None | Some(Value::Null)) => Ordering::Greater,
        (Some(Value::Number(a)), Some(Value::Number(b))) => a
            .as_f64()
            .unwrap_or(0.0)
            .partial_cmp(&b.as_f64().unwrap_or(0.0))
            .unwrap_or(Ordering::Equal),
        (Some(Value::String(a)), Some(Value::String(b))) => a.cmp(b),
        (Some(Value::Bool(a)), Some(Value::Bool(b))) => a.cmp(b),
        _ => Ordering::Equal,
    }
}

fn numeric_op(a: &Value, b: &Value, op: fn(f64, f64) -> f64) -> Value {
    let a_num = value_to_f64(a);
    let b_num = value_to_f64(b);
    match (a_num, b_num) {
        (Some(a), Some(b)) => {
            let result = op(a, b);
            if result.fract() == 0.0 && result.is_finite() {
                Value::Number((result as i64).into())
            } else {
                serde_json::Number::from_f64(result)
                    .map(Value::Number)
                    .unwrap_or(Value::Null)
            }
        }
        _ => Value::Null,
    }
}

fn value_to_f64(v: &Value) -> Option<f64> {
    match v {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.parse().ok(),
        _ => None,
    }
}

//
// Summarize operator: grouping + aggregation.
//

fn apply_summarize(
    columns: &mut Vec<String>,
    rows: &mut Vec<Vec<Value>>,
    aggregations: &[(Option<String>, Expr)],
    group_by: &[Expr],
) {
    //
    // Resolve group-by column indices.
    //

    let group_indices: Vec<usize> = group_by
        .iter()
        .filter_map(|e| {
            if let Expr::Ident(name) = e {
                columns.iter().position(|c| c.eq_ignore_ascii_case(name))
            } else {
                None
            }
        })
        .collect();

    let group_names: Vec<String> = group_by
        .iter()
        .filter_map(|e| {
            if let Expr::Ident(name) = e {
                Some(name.clone())
            } else {
                None
            }
        })
        .collect();

    //
    // Group rows by the group-by key.
    //

    let mut groups: indexmap::IndexMap<Vec<String>, Vec<Vec<Value>>> = indexmap::IndexMap::new();
    for row in rows.iter() {
        let key: Vec<String> = group_indices
            .iter()
            .map(|&i| row.get(i).map(|v| v.to_string()).unwrap_or_default())
            .collect();
        groups.entry(key).or_default().push(row.clone());
    }

    //
    // Build the aggregation column names.
    //

    let agg_names: Vec<String> = aggregations
        .iter()
        .map(|(alias, expr)| alias.clone().unwrap_or_else(|| format_agg_name(expr)))
        .collect();

    //
    // Build result columns: group_by columns + aggregation columns.
    //

    let new_columns: Vec<String> = agg_names
        .iter()
        .chain(group_names.iter())
        .cloned()
        .collect();

    //
    // Compute aggregations for each group.
    //

    let mut new_rows = Vec::new();
    for (_key_strs, group_rows) in &groups {
        let mut result_row = Vec::new();

        for (_, agg_expr) in aggregations {
            let val = compute_aggregation(agg_expr, columns, group_rows);
            result_row.push(val);
        }

        //
        // Add group-by values from the first row of the group.
        //

        for &idx in &group_indices {
            let val = group_rows
                .first()
                .and_then(|r| r.get(idx))
                .cloned()
                .unwrap_or(Value::Null);
            result_row.push(val);
        }

        new_rows.push(result_row);
    }

    *columns = new_columns;
    *rows = new_rows;
}

fn format_agg_name(expr: &Expr) -> String {
    match expr {
        Expr::Func(name, args) => {
            if args.is_empty() {
                format!("{}()", name)
            } else if let Some(Expr::Ident(col)) = args.first() {
                format!("{}({})", name, col)
            } else {
                format!("{}(...)", name)
            }
        }
        Expr::Ident(name) => name.clone(),
        _ => "?".to_string(),
    }
}

fn compute_aggregation(expr: &Expr, columns: &[String], group_rows: &[Vec<Value>]) -> Value {
    match expr {
        Expr::Func(name, args) => match name.to_lowercase().as_str() {
            "count" => Value::Number(group_rows.len().into()),

            "sum" => {
                if let Some(Expr::Ident(col)) = args.first() {
                    let idx = columns.iter().position(|c| c.eq_ignore_ascii_case(col));
                    if let Some(idx) = idx {
                        let sum: f64 = group_rows
                            .iter()
                            .filter_map(|r| r.get(idx).and_then(value_to_f64))
                            .sum();
                        if sum.fract() == 0.0 {
                            Value::Number((sum as i64).into())
                        } else {
                            serde_json::Number::from_f64(sum)
                                .map(Value::Number)
                                .unwrap_or(Value::Null)
                        }
                    } else {
                        Value::Null
                    }
                } else {
                    Value::Null
                }
            }

            "avg" => {
                if let Some(Expr::Ident(col)) = args.first() {
                    let idx = columns.iter().position(|c| c.eq_ignore_ascii_case(col));
                    if let Some(idx) = idx {
                        let vals: Vec<f64> = group_rows
                            .iter()
                            .filter_map(|r| r.get(idx).and_then(value_to_f64))
                            .collect();
                        if vals.is_empty() {
                            Value::Null
                        } else {
                            let avg = vals.iter().sum::<f64>() / vals.len() as f64;
                            serde_json::Number::from_f64(avg)
                                .map(Value::Number)
                                .unwrap_or(Value::Null)
                        }
                    } else {
                        Value::Null
                    }
                } else {
                    Value::Null
                }
            }

            "min" => {
                if let Some(Expr::Ident(col)) = args.first() {
                    let idx = columns.iter().position(|c| c.eq_ignore_ascii_case(col));
                    if let Some(idx) = idx {
                        group_rows
                            .iter()
                            .filter_map(|r| r.get(idx))
                            .filter(|v| !v.is_null())
                            .min_by(|a, b| cmp_values(Some(a), Some(b)))
                            .cloned()
                            .unwrap_or(Value::Null)
                    } else {
                        Value::Null
                    }
                } else {
                    Value::Null
                }
            }

            "max" => {
                if let Some(Expr::Ident(col)) = args.first() {
                    let idx = columns.iter().position(|c| c.eq_ignore_ascii_case(col));
                    if let Some(idx) = idx {
                        group_rows
                            .iter()
                            .filter_map(|r| r.get(idx))
                            .filter(|v| !v.is_null())
                            .max_by(|a, b| cmp_values(Some(a), Some(b)))
                            .cloned()
                            .unwrap_or(Value::Null)
                    } else {
                        Value::Null
                    }
                } else {
                    Value::Null
                }
            }

            "dcount" => {
                if let Some(Expr::Ident(col)) = args.first() {
                    let idx = columns.iter().position(|c| c.eq_ignore_ascii_case(col));
                    if let Some(idx) = idx {
                        let distinct: std::collections::HashSet<String> = group_rows
                            .iter()
                            .filter_map(|r| r.get(idx))
                            .filter(|v| !v.is_null())
                            .map(|v| v.to_string())
                            .collect();
                        Value::Number(distinct.len().into())
                    } else {
                        Value::Null
                    }
                } else {
                    Value::Null
                }
            }

            _ => Value::Null,
        },
        _ => Value::Null,
    }
}
