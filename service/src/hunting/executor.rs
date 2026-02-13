use std::sync::Arc;

use anyhow::{anyhow, Result};
use serde_json::Value;

use super::parser::ast::{Expr, JoinKey, Literal, Operator, Source, Statement, TabularExpression};
use super::parser::parser::parse;

use crate::database::Database;
use crate::state::NodeRegistry;

use super::tables::{
    VirtualTable, materialize_agent_logs, materialize_event_logs,
    materialize_node_logs, materialize_recon_logs,
    materialize_recon_metadata_logs, materialize_recon_session_logs,
    materialize_recon_tool_logs, resolve_table, table_columns,
};

const MAX_RESULT_ROWS: usize = 10_000;

pub struct HuntingResult {
    pub columns: Vec<String>,
    pub rows: Vec<Vec<Value>>,
    pub total_count: usize,
}

//
// Hints extracted from the KQL AST before materialization. These narrow the SQL
// query so we don't pull the entire table into memory. The in-memory operators
// still run on the result — pushdown is purely an optimisation, not a
// correctness requirement.
//

#[derive(Debug, Default)]
struct PushdownHints {
    node_id: Option<String>,
    agent_short_name: Option<String>,
    direction: Option<String>,
    host: Option<String>,
    url_pattern: Option<String>,
    rule_id: Option<i64>,
    source: Option<String>,
    level: Option<String>,
    take_limit: Option<usize>,
}

pub async fn execute_hunting_query(
    query: &str,
    database: &Arc<Database>,
    node_registry: &Arc<NodeRegistry>,
) -> Result<HuntingResult> {
    let statements = parse(query)
        .map(|(_, stmts)| stmts)
        .map_err(|e| anyhow!("KQL parse error: {}", e))?;

    if statements.is_empty() {
        return Err(anyhow!("Empty query"));
    }

    if statements.len() > 1 {
        return Err(anyhow!("Multiple statements not supported; use a single query"));
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
            "Unknown table '{}'. Available tables: TrafficLogs, TrafficMatchLogs, NodeLogs, AgentLogs, ReconLogs, ReconToolLogs, ReconSessionLogs, ReconMetadataLogs, EventLogs",
            table_name
        ))?;

    //
    // Extract pushdown hints from the operator pipeline. We scan all leading
    // `where` and `take` operators for simple predicates we can fold into the
    // SQL query.
    //

    let hints = extract_pushdown_hints(&tabular.operators, table);

    //
    // Materialize the table data, narrowed by pushdown hints.
    //

    let (columns, mut rows) = materialize_table(table, database, node_registry, &hints).await?;

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
                            current_columns.iter().position(|c| c.eq_ignore_ascii_case(name))
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
                            .map(|idx| {
                                idx.and_then(|i| row.get(i).cloned())
                                    .unwrap_or(Value::Null)
                            })
                            .collect()
                    })
                    .collect();

                current_columns = proj_names;
            }

            Operator::Take(n) => {
                let limit = (*n as usize).min(MAX_RESULT_ROWS);
                rows.truncate(limit);
            }

            Operator::Sort(col_names) => {
                if let Some(col_name) = col_names.first() {
                    if let Some(idx) = current_columns.iter().position(|c| c.eq_ignore_ascii_case(col_name)) {
                        rows.sort_by(|a, b| {
                            cmp_values(a.get(idx), b.get(idx))
                        });
                    }
                }
            }

            Operator::Extend(extensions) => {
                for (alias, expr) in extensions {
                    let col_name = alias.clone().unwrap_or_else(|| {
                        if let Expr::Ident(n) = expr { n.clone() } else { "extended".to_string() }
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
                        current_columns.iter().position(|c| c.eq_ignore_ascii_case(name))
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
                        current_columns.iter().position(|c| c.eq_ignore_ascii_case(name))
                    })
                    .collect();

                let keep_indices: Vec<usize> = (0..current_columns.len())
                    .filter(|i| !remove_indices.contains(i))
                    .collect();

                current_columns = keep_indices.iter().map(|&i| current_columns[i].clone()).collect();
                rows = rows
                    .into_iter()
                    .map(|row| keep_indices.iter().map(|&i| row.get(i).cloned().unwrap_or(Value::Null)).collect())
                    .collect();
            }

            Operator::Top(n, sort_expr, asc, nulls_last) => {
                if let Expr::Ident(col_name) = sort_expr {
                    if let Some(idx) = current_columns.iter().position(|c| c.eq_ignore_ascii_case(col_name)) {
                        rows.sort_by(|a, b| {
                            let cmp = cmp_values(a.get(idx), b.get(idx));
                            if *asc { cmp } else { cmp.reverse() }
                        });
                    }
                }
                let _ = nulls_last;
                let limit = (*n as usize).min(MAX_RESULT_ROWS);
                rows.truncate(limit);
            }

            Operator::Join(_options, right_expr, join_keys) => {
                let (right_columns, right_rows) = materialize_tabular_expression(
                    right_expr, database, node_registry,
                ).await?;

                apply_join(
                    &mut current_columns, &mut rows,
                    &right_columns, &right_rows,
                    join_keys,
                );
            }

            other => {
                return Err(anyhow!("Unsupported operator: {:?}", std::mem::discriminant(other)));
            }
        }
    }

    //
    // Apply the hard cap.
    //

    let total_count = rows.len();
    if rows.len() > MAX_RESULT_ROWS {
        rows.truncate(MAX_RESULT_ROWS);
    }

    Ok(HuntingResult {
        columns: current_columns,
        rows,
        total_count,
    })
}

//
// Pre-scan the operator pipeline and extract hints that can narrow the SQL
// query. We only look at leading `where` and `take` operators — once we hit
// anything that reshapes the data (project, extend, summarize …) we stop,
// because columns may no longer map 1:1 to DB columns.
//

fn extract_pushdown_hints(operators: &[Operator], table: VirtualTable) -> PushdownHints {
    let mut hints = PushdownHints::default();
    let columns = table_columns(table);

    for op in operators {
        match op {
            Operator::Where(expr) => {
                collect_where_hints(expr, &columns, &mut hints);
            }
            Operator::Take(n) => {
                let limit = (*n as usize).min(MAX_RESULT_ROWS);
                hints.take_limit = Some(
                    hints.take_limit.map(|prev| prev.min(limit)).unwrap_or(limit),
                );
            }
            //
            // Stop scanning once we hit a column-reshaping operator.
            //
            Operator::Project(_)
            | Operator::ProjectAway(_)
            | Operator::Extend(_)
            | Operator::Summarize(_, _) => break,
            _ => {}
        }
    }

    hints
}

//
// Walk a `where` expression and extract simple `column == "literal"` and
// `column contains "literal"` predicates that map to known DB filter fields.
// We only handle AND-connected top-level predicates — anything complex is left
// for the in-memory evaluator.
//

fn collect_where_hints(expr: &Expr, columns: &[&str], hints: &mut PushdownHints) {
    match expr {
        Expr::And(lhs, rhs) => {
            collect_where_hints(lhs, columns, hints);
            collect_where_hints(rhs, columns, hints);
        }

        Expr::Equals(lhs, rhs) => {
            if let (Expr::Ident(col), Expr::Literal(Literal::String(val)))
            | (Expr::Literal(Literal::String(val)), Expr::Ident(col)) = (lhs.as_ref(), rhs.as_ref())
            {
                match col.to_lowercase().as_str() {
                    "node_id" => hints.node_id = Some(val.clone()),
                    "agent_short_name" => hints.agent_short_name = Some(val.clone()),
                    "direction" => hints.direction = Some(val.clone()),
                    "host" => hints.host = Some(val.clone()),
                    "source" => hints.source = Some(val.clone()),
                    "level" => hints.level = Some(val.clone()),
                    _ => {}
                }
            }
            if let (Expr::Ident(col), Expr::Literal(Literal::Int(Some(val))))
            | (Expr::Literal(Literal::Int(Some(val))), Expr::Ident(col)) = (lhs.as_ref(), rhs.as_ref())
            {
                if col.eq_ignore_ascii_case("rule_id") {
                    hints.rule_id = Some(*val as i64);
                }
            }
            if let (Expr::Ident(col), Expr::Literal(Literal::Long(Some(val))))
            | (Expr::Literal(Literal::Long(Some(val))), Expr::Ident(col)) = (lhs.as_ref(), rhs.as_ref())
            {
                if col.eq_ignore_ascii_case("rule_id") {
                    hints.rule_id = Some(*val);
                }
            }
        }

        //
        // `host contains "openai"` → push as url_pattern (substring match via
        // the existing regex-based filter in query_traffic).
        //
        Expr::Func(name, args) if name.eq_ignore_ascii_case("contains") || name.eq_ignore_ascii_case("has") => {
            if let [Expr::Ident(col), Expr::Literal(Literal::String(val))] = args.as_slice() {
                match col.to_lowercase().as_str() {
                    "url" | "host" => {
                        hints.url_pattern = Some(val.clone());
                    }
                    _ => {}
                }
            }
        }

        _ => {}
    }
}

//
// Materialize a table into (columns, rows), using pushdown hints to narrow
// the DB query.
//

async fn materialize_table(
    table: VirtualTable,
    database: &Arc<Database>,
    node_registry: &Arc<NodeRegistry>,
    hints: &PushdownHints,
) -> Result<(Vec<String>, Vec<Vec<Value>>)> {
    match table {
        VirtualTable::NodeLogs => Ok(materialize_node_logs(node_registry).await),
        VirtualTable::AgentLogs => Ok(materialize_agent_logs(node_registry).await),
        VirtualTable::ReconLogs => materialize_recon_logs(database).await,
        VirtualTable::ReconToolLogs => materialize_recon_tool_logs(database).await,
        VirtualTable::ReconSessionLogs => materialize_recon_session_logs(database).await,
        VirtualTable::ReconMetadataLogs => materialize_recon_metadata_logs(database).await,
        VirtualTable::TrafficLogs => materialize_traffic_logs(database, hints).await,
        VirtualTable::TrafficMatchLogs => materialize_traffic_match_logs(database, hints).await,
        VirtualTable::EventLogs => {
            let limit = hints.take_limit.unwrap_or(MAX_RESULT_ROWS).min(MAX_RESULT_ROWS);
            materialize_event_logs(
                database,
                hints.source.as_deref(),
                hints.level.as_deref(),
                limit,
            ).await
        }
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
) -> Result<(Vec<String>, Vec<Vec<Value>>)> {
    let table_name = match &expr.source {
        Source::Reference(name) => name.clone(),
        _ => return Err(anyhow!("Join: only table references are supported as right-side source")),
    };

    let table = resolve_table(&table_name)
        .ok_or_else(|| anyhow!("Join: unknown table '{}'", table_name))?;

    let hints = extract_pushdown_hints(&expr.operators, table);
    let (mut columns, mut rows) = materialize_table(table, database, node_registry, &hints).await?;

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
                rows.truncate((*n as usize).min(MAX_RESULT_ROWS));
            }
            Operator::Project(projections) => {
                let proj_names: Vec<String> = projections
                    .iter()
                    .map(|(alias, e)| {
                        alias.clone().unwrap_or_else(|| {
                            if let Expr::Ident(name) = e { name.clone() } else { "?".to_string() }
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
                        indices.iter().map(|idx| {
                            idx.and_then(|i| row.get(i).cloned()).unwrap_or(Value::Null)
                        }).collect()
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
        .filter_map(|k| left_columns.iter().position(|c| c.eq_ignore_ascii_case(&k.left)))
        .collect();
    let right_key_indices: Vec<usize> = join_keys
        .iter()
        .filter_map(|k| right_columns.iter().position(|c| c.eq_ignore_ascii_case(&k.right)))
        .collect();

    if left_key_indices.is_empty() || left_key_indices.len() != right_key_indices.len() {
        return;
    }

    //
    // Determine which right columns to add (skip join key columns that already
    // exist on the left).
    //

    let left_names_lower: std::collections::HashSet<String> = left_columns
        .iter()
        .map(|c| c.to_lowercase())
        .collect();

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
                    merged.push(
                        right_row.get(*col_idx).cloned().unwrap_or(Value::Null)
                    );
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

fn traffic_direction_from_hint(s: &str) -> Option<common::TrafficDirection> {
    match s.to_lowercase().as_str() {
        "send" => Some(common::TrafficDirection::Send),
        "receive" => Some(common::TrafficDirection::Receive),
        _ => None,
    }
}

async fn materialize_traffic_logs(
    database: &Arc<Database>,
    hints: &PushdownHints,
) -> Result<(Vec<String>, Vec<Vec<Value>>)> {
    let columns: Vec<String> = table_columns(VirtualTable::TrafficLogs)
        .into_iter()
        .map(String::from)
        .collect();

    let filters = common::TrafficLogFilters {
        node_id: hints.node_id.clone(),
        agent_short_name: hints.agent_short_name.clone(),
        direction: hints.direction.as_deref().and_then(traffic_direction_from_hint),
        url_pattern: hints.url_pattern.clone().or_else(|| hints.host.clone()),
        limit: hints.take_limit.unwrap_or(MAX_RESULT_ROWS).min(MAX_RESULT_ROWS),
        ..Default::default()
    };
    let (entries, _) = database.query_traffic(&filters).await?;

    let rows: Vec<Vec<Value>> = entries
        .into_iter()
        .map(|e| {
            let req_headers = e.request_headers
                .as_ref()
                .map(|h| serde_json::to_value(h).unwrap_or(Value::Null))
                .unwrap_or(Value::Null);
            let req_body = e.request_body
                .as_ref()
                .map(|b| Value::String(String::from_utf8_lossy(b).to_string()))
                .unwrap_or(Value::Null);
            let resp_headers = e.response_headers
                .as_ref()
                .map(|h| serde_json::to_value(h).unwrap_or(Value::Null))
                .unwrap_or(Value::Null);
            let resp_body = e.response_body
                .as_ref()
                .map(|b| Value::String(String::from_utf8_lossy(b).to_string()))
                .unwrap_or(Value::Null);

            vec![
                Value::String(e.timestamp.to_rfc3339()),
                e.id.map(|id| Value::Number(id.into())).unwrap_or(Value::Null),
                Value::String(e.node_id),
                Value::String(e.agent_short_name),
                Value::String(format!("{}", e.intercept_method)),
                Value::String(format!("{}", e.direction)),
                e.method.map(Value::String).unwrap_or(Value::Null),
                Value::String(e.url),
                Value::String(e.host),
                req_headers,
                req_body,
                e.response_status.map(|s| Value::Number(s.into())).unwrap_or(Value::Null),
                resp_headers,
                resp_body,
            ]
        })
        .collect();

    Ok((columns, rows))
}

async fn materialize_traffic_match_logs(
    database: &Arc<Database>,
    hints: &PushdownHints,
) -> Result<(Vec<String>, Vec<Vec<Value>>)> {
    let columns: Vec<String> = table_columns(VirtualTable::TrafficMatchLogs)
        .into_iter()
        .map(String::from)
        .collect();

    let limit = hints.take_limit.unwrap_or(MAX_RESULT_ROWS).min(MAX_RESULT_ROWS);
    let (matches, _) = database.query_matches(hints.rule_id, limit, 0).await?;

    let rows: Vec<Vec<Value>> = matches
        .into_iter()
        .map(|m| {
            vec![
                Value::String(m.match_info.matched_at.to_rfc3339()),
                Value::Number(m.match_info.traffic_id.into()),
                Value::String(m.traffic.node_id),
                Value::String(m.traffic.agent_short_name),
                Value::Number(m.match_info.rule_id.into()),
                Value::String(m.match_info.rule_name),
                m.match_info.summary.map(Value::String).unwrap_or(Value::Null),
                m.traffic.method.map(Value::String).unwrap_or(Value::Null),
                Value::String(m.traffic.url),
                Value::String(m.traffic.host),
                Value::String(format!("{}", m.traffic.direction)),
                m.traffic.response_status.map(|s| Value::Number(s.into())).unwrap_or(Value::Null),
            ]
        })
        .collect();

    Ok((columns, rows))
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
        Expr::Ident(name) => {
            columns
                .iter()
                .position(|c| c.eq_ignore_ascii_case(name))
                .and_then(|i| row.get(i))
                .cloned()
                .unwrap_or(Value::Null)
        }

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
        Literal::Real(Some(n)) => {
            serde_json::Number::from_f64(*n as f64)
                .map(Value::Number)
                .unwrap_or(Value::Null)
        }
        Literal::Decimal(Some(n)) => {
            serde_json::Number::from_f64(*n)
                .map(Value::Number)
                .unwrap_or(Value::Null)
        }
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
        (Some(Value::Number(a)), Some(Value::Number(b))) => {
            a.as_f64()
                .unwrap_or(0.0)
                .partial_cmp(&b.as_f64().unwrap_or(0.0))
                .unwrap_or(Ordering::Equal)
        }
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
            if let Expr::Ident(name) = e { Some(name.clone()) } else { None }
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
        .map(|(alias, expr)| {
            alias.clone().unwrap_or_else(|| format_agg_name(expr))
        })
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
        Expr::Func(name, args) => {
            match name.to_lowercase().as_str() {
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
            }
        }
        _ => Value::Null,
    }
}
