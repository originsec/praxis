//
// Edit engine for the change ledger. Applies a ChangeOp to the text of an
// agent config file while preserving everything the op does not touch:
//
// - JSON / JSONC: edited through a concrete syntax tree (jsonc-parser), so
//   comments, ordering and formatting survive byte-for-byte.
// - TOML: edited through toml_edit, which is likewise format-preserving.
// - YAML: transcoded to JSON, edited with the JSON engine and emitted back
//   as YAML. Data and key order are preserved; comments and custom layout
//   are not (no reliable lossless YAML editor exists for Rust today).
// - Markdown / text: begin/end marker blocks keyed by change id.
//
// Every key-path edit is cross-checked against a plain serde_json reference
// implementation: the edited file is re-parsed and must equal the reference
// result, otherwise the edit is refused rather than risk corrupting the file
// on the endpoint.
//

use anyhow::{Result, anyhow, bail};
use common::config_change::{ChangeOp, ConfigFileKind, format_key_path, parse_key_path};
use jsonc_parser::ParseOptions;
use jsonc_parser::cst::{CstInputValue, CstObject, CstRootNode};
use serde_json::{Map, Value};

pub struct EditOutcome {
    pub content: String,
    pub undo: Option<Vec<ChangeOp>>,
}

//
// Apply `op` to `before` (None = file does not exist). `change_id` labels
// marker blocks.
//

pub fn apply_op(
    kind: ConfigFileKind,
    before: Option<&str>,
    op: &ChangeOp,
    change_id: &str,
) -> Result<EditOutcome> {
    let text = before.unwrap_or("");
    match op {
        ChangeOp::Overwrite { contents } => Ok(EditOutcome {
            content: contents.clone(),
            undo: None,
        }),
        ChangeOp::AppendBlock { content } => {
            require_blocks(kind)?;
            let (content, strip_leading_newline) = append_block(text, change_id, content)?;
            Ok(EditOutcome {
                content,
                undo: Some(vec![ChangeOp::RemoveBlock {
                    block_id: change_id.to_string(),
                    strip_leading_newline,
                }]),
            })
        }
        ChangeOp::RemoveBlock {
            block_id,
            strip_leading_newline,
        } => {
            require_blocks(kind)?;
            let content = remove_block(text, block_id, *strip_leading_newline, false)?;
            Ok(EditOutcome {
                content,
                undo: None,
            })
        }
        _ => {
            if !kind.supports_key_ops() {
                bail!(
                    "operation '{}' needs a JSON, JSONC, TOML or YAML file (this is {})",
                    op.name(),
                    kind.as_str()
                );
            }
            let before_value = semantic(kind, text)?;
            let undo = inverse_ops(&before_value, op);
            let content = apply_key_op(kind, text, op, false)?;
            Ok(EditOutcome {
                content,
                undo: Some(undo),
            })
        }
    }
}

//
// Apply recorded undo ops on top of the current file content. Missing keys,
// list entries or blocks are treated as already undone.
//

pub fn apply_undo(kind: ConfigFileKind, current: &str, undo: &[ChangeOp]) -> Result<String> {
    let mut text = current.to_string();
    for op in undo {
        text = match op {
            ChangeOp::RemoveBlock {
                block_id,
                strip_leading_newline,
            } => remove_block(&text, block_id, *strip_leading_newline, true)?,
            ChangeOp::Overwrite { .. } | ChangeOp::AppendBlock { .. } => {
                bail!("'{}' is not a valid undo step", op.name())
            }
            _ => apply_key_op(kind, &text, op, true)?,
        };
    }
    Ok(text)
}

pub fn unified_diff(old: &str, new: &str, old_label: &str, new_label: &str) -> String {
    similar::TextDiff::from_lines(old, new)
        .unified_diff()
        .context_radius(3)
        .header(old_label, new_label)
        .to_string()
}

fn require_blocks(kind: ConfigFileKind) -> Result<()> {
    if kind.supports_blocks() {
        Ok(())
    } else {
        bail!(
            "marker blocks are only supported for markdown/text files (this is {})",
            kind.as_str()
        )
    }
}

// ---------------------------------------------------------------------------
// Marker blocks
// ---------------------------------------------------------------------------

pub fn block_begin_marker(id: &str) -> String {
    format!("<!-- praxis-change:begin {} -->", id)
}

pub fn block_end_marker(id: &str) -> String {
    format!("<!-- praxis-change:end {} -->", id)
}

fn append_block(text: &str, id: &str, content: &str) -> Result<(String, bool)> {
    let begin = block_begin_marker(id);
    let end = block_end_marker(id);
    if text.contains(&begin) || content.contains(&begin) || content.contains(&end) {
        bail!("a block for change {} already exists", id);
    }
    let lead = !text.is_empty() && !text.ends_with('\n');
    let mut out = String::with_capacity(text.len() + content.len() + begin.len() * 2 + 4);
    out.push_str(text);
    if lead {
        out.push('\n');
    }
    out.push_str(&begin);
    out.push('\n');
    out.push_str(content);
    if !content.is_empty() && !content.ends_with('\n') {
        out.push('\n');
    }
    out.push_str(&end);
    out.push('\n');
    Ok((out, lead))
}

fn remove_block(
    text: &str,
    id: &str,
    strip_leading_newline: bool,
    lenient: bool,
) -> Result<String> {
    let begin = block_begin_marker(id);
    let end = block_end_marker(id);

    let begins: Vec<usize> = text.match_indices(&begin).map(|(i, _)| i).collect();
    let Some(&start) = begins.first() else {
        if lenient {
            return Ok(text.to_string());
        }
        bail!("no marker block for change {} found", id);
    };
    if begins.len() > 1 {
        bail!("marker block for change {} appears more than once", id);
    }
    if start > 0 && text.as_bytes()[start - 1] != b'\n' {
        bail!(
            "begin marker for change {} is not at the start of a line",
            id
        );
    }
    let Some(end_rel) = text[start..].find(&end) else {
        bail!("end marker for change {} is missing", id);
    };
    let mut stop = start + end_rel + end.len();
    if text[stop..].starts_with("\r\n") {
        stop += 2;
    } else if text[stop..].starts_with('\n') {
        stop += 1;
    }
    let mut from = start;
    if strip_leading_newline && from > 0 && text.as_bytes()[from - 1] == b'\n' {
        from -= 1;
    }
    let mut out = String::with_capacity(text.len());
    out.push_str(&text[..from]);
    out.push_str(&text[stop..]);
    Ok(out)
}

// ---------------------------------------------------------------------------
// Key-path ops: dispatch + verification
// ---------------------------------------------------------------------------

fn apply_key_op(kind: ConfigFileKind, text: &str, op: &ChangeOp, lenient: bool) -> Result<String> {
    let mut expected = semantic(kind, text)?;
    semantic_apply(&mut expected, op, lenient)?;

    let edited = match kind {
        ConfigFileKind::Json | ConfigFileKind::Jsonc => json_apply(text, op, lenient)?,
        ConfigFileKind::Toml => toml_apply(text, op, lenient)?,
        ConfigFileKind::Yaml => yaml_apply(text, op, lenient)?,
        _ => bail!(
            "key-path operations are not supported for {}",
            kind.as_str()
        ),
    };

    let actual = semantic(kind, &edited).map_err(|e| {
        anyhow!(
            "edit produced an unparseable file ({}); refusing to write",
            e
        )
    })?;
    if actual != expected {
        bail!(
            "edit verification failed for '{}': result does not match the expected document; refusing to write",
            op.summary()
        );
    }
    Ok(edited)
}

//
// Parse a document into a serde_json::Value. Empty input is an empty
// object (JSON/TOML) or null (YAML), so ops can create files from scratch.
//

pub fn semantic(kind: ConfigFileKind, text: &str) -> Result<Value> {
    match kind {
        ConfigFileKind::Json | ConfigFileKind::Jsonc => {
            let root = CstRootNode::parse(text, &ParseOptions::default())
                .map_err(|e| anyhow!("invalid JSON: {}", e))?;
            Ok(root
                .to_serde_value()
                .unwrap_or_else(|| Value::Object(Map::new())))
        }
        ConfigFileKind::Toml => {
            let doc: toml_edit::DocumentMut =
                text.parse().map_err(|e| anyhow!("invalid TOML: {}", e))?;
            Ok(toml_item_to_json(doc.as_item()))
        }
        ConfigFileKind::Yaml => {
            if text.trim().is_empty() {
                return Ok(Value::Null);
            }
            serde_yaml_ng::from_str::<Value>(text).map_err(|e| anyhow!("invalid YAML: {}", e))
        }
        _ => bail!("{} files have no structured form", kind.as_str()),
    }
}

// ---------------------------------------------------------------------------
// Reference implementation on serde_json::Value
// ---------------------------------------------------------------------------

fn not_found(lenient: bool, what: String) -> Result<()> {
    if lenient { Ok(()) } else { Err(anyhow!(what)) }
}

fn ensure_object(v: &mut Value) {
    if v.is_null() {
        *v = Value::Object(Map::new());
    }
}

//
// Walk to the object at `path`. With `create`, missing (or null) objects
// along the way are created; otherwise None is returned when absent.
//

fn object_at_mut<'a>(
    root: &'a mut Value,
    path: &[String],
    create: bool,
) -> Result<Option<&'a mut Map<String, Value>>> {
    if create {
        ensure_object(root);
    }
    let mut cur = root;
    for (i, seg) in path.iter().enumerate() {
        let obj = match cur {
            Value::Object(m) => m,
            _ => bail!("'{}' is not an object", format_key_path(&path[..i])),
        };
        if !obj.contains_key(seg) {
            if !create {
                return Ok(None);
            }
            obj.insert(seg.clone(), Value::Object(Map::new()));
        }
        cur = obj.get_mut(seg).unwrap();
        if create {
            ensure_object(cur);
        }
    }
    match cur {
        Value::Object(m) => Ok(Some(m)),
        Value::Null if !create => Ok(None),
        _ => bail!("'{}' is not an object", format_key_path(path)),
    }
}

fn split_last(path: &[String]) -> Result<(&[String], &String)> {
    path.split_last()
        .map(|(last, parent)| (parent, last))
        .ok_or_else(|| anyhow!("this operation needs a non-empty key path"))
}

fn semantic_apply(root: &mut Value, op: &ChangeOp, lenient: bool) -> Result<()> {
    match op {
        ChangeOp::Set { key_path, value } => {
            let path = parse_key_path(key_path);
            if path.is_empty() {
                *root = value.clone();
                return Ok(());
            }
            let (parent, last) = split_last(&path)?;
            let obj = object_at_mut(root, parent, true)?.unwrap();
            obj.insert(last.clone(), value.clone());
            Ok(())
        }
        ChangeOp::Merge { key_path, value } => {
            let Value::Object(patch) = value else {
                bail!("merge needs an object value");
            };
            let path = parse_key_path(key_path);
            let obj = object_at_mut(root, &path, true)?.unwrap();
            deep_merge(obj, patch);
            Ok(())
        }
        ChangeOp::Remove { key_path } => {
            let path = parse_key_path(key_path);
            let (parent, last) = split_last(&path)?;
            let removed = object_at_mut(root, parent, false)?.and_then(|obj| obj.remove(last));
            match removed {
                Some(_) => Ok(()),
                None => not_found(lenient, format!("key '{}' not found", key_path)),
            }
        }
        ChangeOp::ListAppend { key_path, value } => {
            let arr = semantic_array(root, key_path, true)?.unwrap();
            if !arr.contains(value) {
                arr.push(value.clone());
            }
            Ok(())
        }
        ChangeOp::ListRemove { key_path, value } => match semantic_array(root, key_path, false)? {
            Some(arr) => match arr.iter().position(|v| v == value) {
                Some(idx) => {
                    arr.remove(idx);
                    Ok(())
                }
                None => not_found(lenient, format!("entry not found in '{}'", key_path)),
            },
            None => not_found(lenient, format!("list '{}' not found", key_path)),
        },
        _ => bail!("'{}' is not a key-path operation", op.name()),
    }
}

fn semantic_array<'a>(
    root: &'a mut Value,
    key_path: &str,
    create: bool,
) -> Result<Option<&'a mut Vec<Value>>> {
    let path = parse_key_path(key_path);
    let slot: &mut Value = if path.is_empty() {
        if create && root.is_null() {
            *root = Value::Array(Vec::new());
        }
        root
    } else {
        let (parent, last) = split_last(&path)?;
        let Some(obj) = object_at_mut(root, parent, create)? else {
            return Ok(None);
        };
        if !obj.contains_key(last) {
            if !create {
                return Ok(None);
            }
            obj.insert(last.clone(), Value::Array(Vec::new()));
        }
        obj.get_mut(last).unwrap()
    };
    match slot {
        Value::Array(a) => Ok(Some(a)),
        Value::Null if !create => Ok(None),
        _ => bail!("'{}' is not a list", key_path),
    }
}

fn deep_merge(target: &mut Map<String, Value>, patch: &Map<String, Value>) {
    for (k, v) in patch {
        match (target.get_mut(k), v) {
            (Some(Value::Object(existing)), Value::Object(sub)) => deep_merge(existing, sub),
            _ => {
                target.insert(k.clone(), v.clone());
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Inverse ops (structured undo)
// ---------------------------------------------------------------------------

fn lookup<'a>(root: &'a Value, path: &[String]) -> Option<&'a Value> {
    let mut cur = root;
    for seg in path {
        cur = cur.as_object()?.get(seg)?;
    }
    Some(cur)
}

//
// Undo for setting `path`: restore the old value, or remove the highest
// ancestor the op had to create so no empty scaffolding is left behind.
//

fn undo_for_set(before: &Value, path: &[String]) -> ChangeOp {
    if let Some(old) = lookup(before, path) {
        return ChangeOp::Set {
            key_path: format_key_path(path),
            value: old.clone(),
        };
    }
    let mut created = path.len();
    while created > 1 && lookup(before, &path[..created - 1]).is_none() {
        created -= 1;
    }
    ChangeOp::Remove {
        key_path: format_key_path(&path[..created]),
    }
}

fn merge_undo(
    before: &Value,
    base: &[String],
    patch: &Map<String, Value>,
    out: &mut Vec<ChangeOp>,
) {
    for (k, v) in patch {
        let mut path = base.to_vec();
        path.push(k.clone());
        match (lookup(before, &path), v) {
            (Some(Value::Object(_)), Value::Object(sub)) => merge_undo(before, &path, sub, out),
            _ => out.push(undo_for_set(before, &path)),
        }
    }
}

fn inverse_ops(before: &Value, op: &ChangeOp) -> Vec<ChangeOp> {
    match op {
        ChangeOp::Set { key_path, .. } => {
            let path = parse_key_path(key_path);
            if path.is_empty() {
                return vec![ChangeOp::Set {
                    key_path: String::new(),
                    value: before.clone(),
                }];
            }
            vec![undo_for_set(before, &path)]
        }
        ChangeOp::Merge { key_path, value } => {
            let mut out = Vec::new();
            if let Value::Object(patch) = value {
                merge_undo(before, &parse_key_path(key_path), patch, &mut out);
            }
            out
        }
        ChangeOp::Remove { key_path } => match lookup(before, &parse_key_path(key_path)) {
            Some(old) => vec![ChangeOp::Set {
                key_path: key_path.clone(),
                value: old.clone(),
            }],
            None => Vec::new(),
        },
        ChangeOp::ListAppend { key_path, value } => {
            let path = parse_key_path(key_path);
            match lookup(before, &path) {
                Some(Value::Array(a)) if a.contains(value) => Vec::new(),
                Some(_) => vec![ChangeOp::ListRemove {
                    key_path: key_path.clone(),
                    value: value.clone(),
                }],
                None if path.is_empty() => Vec::new(),
                None => vec![undo_for_set(before, &path)],
            }
        }
        ChangeOp::ListRemove { key_path, value } => {
            match lookup(before, &parse_key_path(key_path)) {
                Some(Value::Array(a)) if a.contains(value) => vec![ChangeOp::ListAppend {
                    key_path: key_path.clone(),
                    value: value.clone(),
                }],
                _ => Vec::new(),
            }
        }
        _ => Vec::new(),
    }
}

// ---------------------------------------------------------------------------
// JSON / JSONC via concrete syntax tree
// ---------------------------------------------------------------------------

fn to_cst(v: &Value) -> CstInputValue {
    match v {
        Value::Null => CstInputValue::Null,
        Value::Bool(b) => CstInputValue::Bool(*b),
        Value::Number(n) => CstInputValue::Number(n.to_string()),
        Value::String(s) => CstInputValue::String(s.clone()),
        Value::Array(a) => CstInputValue::Array(a.iter().map(to_cst).collect()),
        Value::Object(m) => {
            CstInputValue::Object(m.iter().map(|(k, v)| (k.clone(), to_cst(v))).collect())
        }
    }
}

fn cst_object_at(root: &CstRootNode, path: &[String], create: bool) -> Result<Option<CstObject>> {
    let mut obj = match root.value() {
        None if create => root.object_value_or_set(),
        None => return Ok(None),
        Some(v) => match v.as_object() {
            Some(o) => o,
            None if create && v.as_null_keyword().is_some() => root.object_value_or_set(),
            None if v.as_null_keyword().is_some() => return Ok(None),
            None => bail!("document root is not an object"),
        },
    };
    for (i, seg) in path.iter().enumerate() {
        let next = match obj.get(seg) {
            Some(prop) => match prop.value() {
                Some(v) if v.as_object().is_some() => v.as_object().unwrap(),
                Some(v) if create && v.as_null_keyword().is_some() => prop.object_value_or_set(),
                Some(v) if v.as_null_keyword().is_some() => return Ok(None),
                _ => bail!("'{}' is not an object", format_key_path(&path[..=i])),
            },
            None if create => {
                obj.append(seg, CstInputValue::Object(Vec::new()));
                obj.get(seg).and_then(|p| p.object_value()).unwrap()
            }
            None => return Ok(None),
        };
        obj = next;
    }
    Ok(Some(obj))
}

fn cst_set(obj: &CstObject, key: &str, value: &Value) {
    match obj.get(key) {
        Some(prop) => prop.set_value(to_cst(value)),
        None => {
            obj.append(key, to_cst(value));
        }
    }
}

fn cst_merge(obj: &CstObject, patch: &Map<String, Value>) {
    for (k, v) in patch {
        let existing = obj.get(k).and_then(|p| p.object_value());
        match (existing, v) {
            (Some(sub), Value::Object(sub_patch)) => cst_merge(&sub, sub_patch),
            _ => cst_set(obj, k, v),
        }
    }
}

fn json_apply(text: &str, op: &ChangeOp, lenient: bool) -> Result<String> {
    let root = CstRootNode::parse(text, &ParseOptions::default())
        .map_err(|e| anyhow!("invalid JSON: {}", e))?;

    match op {
        ChangeOp::Set { key_path, value } => {
            let path = parse_key_path(key_path);
            if path.is_empty() {
                root.set_value(to_cst(value));
            } else {
                let (parent, last) = split_last(&path)?;
                let obj = cst_object_at(&root, parent, true)?.unwrap();
                cst_set(&obj, last, value);
            }
        }
        ChangeOp::Merge { key_path, value } => {
            let Value::Object(patch) = value else {
                bail!("merge needs an object value");
            };
            let obj = cst_object_at(&root, &parse_key_path(key_path), true)?.unwrap();
            cst_merge(&obj, patch);
        }
        ChangeOp::Remove { key_path } => {
            let path = parse_key_path(key_path);
            let (parent, last) = split_last(&path)?;
            match cst_object_at(&root, parent, false)?.and_then(|o| o.get(last)) {
                Some(prop) => prop.remove(),
                None => not_found(lenient, format!("key '{}' not found", key_path))?,
            }
        }
        ChangeOp::ListAppend { key_path, value } | ChangeOp::ListRemove { key_path, value } => {
            let append = matches!(op, ChangeOp::ListAppend { .. });
            let path = parse_key_path(key_path);
            let arr = if path.is_empty() {
                match root.value() {
                    None if append => Some(root.array_value_or_set()),
                    None => None,
                    Some(v) => Some(
                        v.as_array()
                            .ok_or_else(|| anyhow!("document root is not a list"))?,
                    ),
                }
            } else {
                let (parent, last) = split_last(&path)?;
                match cst_object_at(&root, parent, append)? {
                    None => None,
                    Some(obj) => match obj.get(last) {
                        Some(prop) => match prop.value() {
                            Some(v) if v.as_array().is_some() => v.as_array(),
                            Some(v) if v.as_null_keyword().is_some() && append => {
                                Some(prop.array_value_or_set())
                            }
                            Some(v) if v.as_null_keyword().is_some() => None,
                            _ => bail!("'{}' is not a list", key_path),
                        },
                        None if append => {
                            obj.append(last, CstInputValue::Array(Vec::new()));
                            obj.get(last).and_then(|p| p.array_value())
                        }
                        None => None,
                    },
                }
            };
            let existing = arr.as_ref().and_then(|a| {
                a.elements()
                    .into_iter()
                    .find(|e| e.to_serde_value().as_ref() == Some(value))
            });
            match (append, arr, existing) {
                (true, Some(_), Some(_)) => {}
                (true, Some(a), None) => {
                    a.append(to_cst(value));
                }
                (false, Some(_), Some(e)) => e.remove(),
                (false, _, _) => not_found(lenient, format!("entry not found in '{}'", key_path))?,
                (true, None, _) => bail!("could not create list '{}'", key_path),
            }
        }
        _ => bail!("'{}' is not a key-path operation", op.name()),
    }

    Ok(root.to_string())
}

// ---------------------------------------------------------------------------
// YAML: transcode through the JSON engine
// ---------------------------------------------------------------------------

fn yaml_apply(text: &str, op: &ChangeOp, lenient: bool) -> Result<String> {
    let json_text = if text.trim().is_empty() {
        String::new()
    } else {
        let doc: serde_yaml_ng::Value =
            serde_yaml_ng::from_str(text).map_err(|e| anyhow!("invalid YAML: {}", e))?;
        serde_json::to_string_pretty(&doc)
            .map_err(|e| anyhow!("YAML document cannot be edited structurally: {}", e))?
    };
    let edited = json_apply(&json_text, op, lenient)?;
    let doc: serde_yaml_ng::Value = serde_json::from_str(&edited)?;
    Ok(serde_yaml_ng::to_string(&doc)?)
}

// ---------------------------------------------------------------------------
// TOML via toml_edit
// ---------------------------------------------------------------------------

fn toml_item_to_json(item: &toml_edit::Item) -> Value {
    match item {
        toml_edit::Item::None => Value::Null,
        toml_edit::Item::Value(v) => toml_value_to_json(v),
        toml_edit::Item::Table(t) => Value::Object(
            t.iter()
                .map(|(k, v)| (k.to_string(), toml_item_to_json(v)))
                .collect(),
        ),
        toml_edit::Item::ArrayOfTables(a) => Value::Array(
            a.iter()
                .map(|t| {
                    Value::Object(
                        t.iter()
                            .map(|(k, v)| (k.to_string(), toml_item_to_json(v)))
                            .collect(),
                    )
                })
                .collect(),
        ),
    }
}

fn toml_value_to_json(v: &toml_edit::Value) -> Value {
    match v {
        toml_edit::Value::String(s) => Value::String(s.value().clone()),
        toml_edit::Value::Integer(i) => Value::from(*i.value()),
        toml_edit::Value::Float(f) => serde_json::Number::from_f64(*f.value())
            .map(Value::Number)
            .unwrap_or(Value::Null),
        toml_edit::Value::Boolean(b) => Value::Bool(*b.value()),
        toml_edit::Value::Datetime(d) => Value::String(d.value().to_string()),
        toml_edit::Value::Array(a) => Value::Array(a.iter().map(toml_value_to_json).collect()),
        toml_edit::Value::InlineTable(t) => Value::Object(
            t.iter()
                .map(|(k, v)| (k.to_string(), toml_value_to_json(v)))
                .collect(),
        ),
    }
}

fn json_to_toml_value(v: &Value) -> Result<toml_edit::Value> {
    Ok(match v {
        Value::Null => bail!("TOML cannot represent null values"),
        Value::Bool(b) => (*b).into(),
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                i.into()
            } else if let Some(f) = n.as_f64() {
                f.into()
            } else {
                bail!("number {} does not fit in TOML", n)
            }
        }
        Value::String(s) => s.as_str().into(),
        Value::Array(a) => {
            let mut arr = toml_edit::Array::new();
            for e in a {
                arr.push(json_to_toml_value(e)?);
            }
            toml_edit::Value::Array(arr)
        }
        Value::Object(m) => {
            let mut t = toml_edit::InlineTable::new();
            for (k, e) in m {
                t.insert(k, json_to_toml_value(e)?);
            }
            toml_edit::Value::InlineTable(t)
        }
    })
}

//
// Objects placed under a standard table become standard tables
// ([a.b] headers); everything nested inside arrays or inline tables stays
// inline.
//

fn json_to_toml_item(v: &Value, inline: bool) -> Result<toml_edit::Item> {
    match v {
        Value::Object(m) if !inline => {
            let mut t = toml_edit::Table::new();
            for (k, e) in m {
                t.insert(k, json_to_toml_item(e, false)?);
            }
            Ok(toml_edit::Item::Table(t))
        }
        _ => Ok(toml_edit::Item::Value(json_to_toml_value(v)?)),
    }
}

fn toml_item_at<'a>(
    root: &'a mut toml_edit::Item,
    path: &[String],
    create: bool,
) -> Result<Option<&'a mut toml_edit::Item>> {
    let mut cur = root;
    for (i, seg) in path.iter().enumerate() {
        let inline = cur.is_inline_table();
        let table = cur
            .as_table_like_mut()
            .ok_or_else(|| anyhow!("'{}' is not a table", format_key_path(&path[..i])))?;
        if !table.contains_key(seg) {
            if !create {
                return Ok(None);
            }
            let item = if inline {
                toml_edit::Item::Value(toml_edit::Value::InlineTable(toml_edit::InlineTable::new()))
            } else {
                let mut t = toml_edit::Table::new();
                t.set_implicit(true);
                toml_edit::Item::Table(t)
            };
            table.insert(seg, item);
        }
        cur = table.get_mut(seg).unwrap();
    }
    Ok(Some(cur))
}

fn toml_set(parent: &mut toml_edit::Item, key: &str, value: &Value) -> Result<()> {
    let inline = !parent.is_table();
    let mut item = json_to_toml_item(value, inline)?;
    let table = parent
        .as_table_like_mut()
        .ok_or_else(|| anyhow!("parent of '{}' is not a table", key))?;

    //
    // Replace an existing value in place so the key keeps its decor
    // (leading comments live on the key), and carry over the trailing
    // comment / spacing of a scalar being replaced.
    //

    match table.get_mut(key) {
        Some(slot) => {
            if let (toml_edit::Item::Value(old), toml_edit::Item::Value(new)) = (&*slot, &mut item)
            {
                *new.decor_mut() = old.decor().clone();
            }
            *slot = item;
        }
        None => {
            table.insert(key, item);
        }
    }
    Ok(())
}

fn toml_merge(target: &mut toml_edit::Item, patch: &Map<String, Value>) -> Result<()> {
    for (k, v) in patch {
        let table = target
            .as_table_like_mut()
            .ok_or_else(|| anyhow!("merge target is not a table"))?;
        match (table.get_mut(k), v) {
            (Some(sub), Value::Object(sub_patch)) if sub.is_table_like() => {
                toml_merge(sub, sub_patch)?
            }
            _ => toml_set(target, k, v)?,
        }
    }
    Ok(())
}

fn toml_apply(text: &str, op: &ChangeOp, lenient: bool) -> Result<String> {
    let mut doc: toml_edit::DocumentMut =
        text.parse().map_err(|e| anyhow!("invalid TOML: {}", e))?;
    let root = doc.as_item_mut();

    match op {
        ChangeOp::Set { key_path, value } => {
            let path = parse_key_path(key_path);
            if path.is_empty() {
                bail!("cannot replace the root of a TOML document");
            }
            let (parent, last) = split_last(&path)?;
            let parent = toml_item_at(root, parent, true)?.unwrap();
            toml_set(parent, last, value)?;
        }
        ChangeOp::Merge { key_path, value } => {
            let Value::Object(patch) = value else {
                bail!("merge needs an object value");
            };
            let target = toml_item_at(root, &parse_key_path(key_path), true)?.unwrap();
            if !target.is_table_like() {
                bail!("'{}' is not a table", key_path);
            }
            toml_merge(target, patch)?;
        }
        ChangeOp::Remove { key_path } => {
            let path = parse_key_path(key_path);
            let (parent, last) = split_last(&path)?;
            let removed = toml_item_at(root, parent, false)?
                .and_then(|p| p.as_table_like_mut())
                .and_then(|t| t.remove(last));
            if removed.is_none() {
                not_found(lenient, format!("key '{}' not found", key_path))?;
            }
        }
        ChangeOp::ListAppend { key_path, value } | ChangeOp::ListRemove { key_path, value } => {
            let append = matches!(op, ChangeOp::ListAppend { .. });
            let path = parse_key_path(key_path);
            let (parent, last) = split_last(&path)?;
            let Some(parent) = toml_item_at(root, parent, append)? else {
                return not_found(lenient, format!("list '{}' not found", key_path))
                    .map(|_| doc.to_string());
            };
            let table = parent
                .as_table_like_mut()
                .ok_or_else(|| anyhow!("parent of '{}' is not a table", key_path))?;
            if !table.contains_key(last) {
                if !append {
                    return not_found(lenient, format!("list '{}' not found", key_path))
                        .map(|_| doc.to_string());
                }
                table.insert(
                    last,
                    toml_edit::Item::Value(toml_edit::Value::Array(toml_edit::Array::new())),
                );
            }
            let item = table.get_mut(last).unwrap();
            toml_list_op(item, key_path, value, append, lenient)?;
        }
        _ => bail!("'{}' is not a key-path operation", op.name()),
    }

    Ok(doc.to_string())
}

fn toml_list_op(
    item: &mut toml_edit::Item,
    key_path: &str,
    value: &Value,
    append: bool,
    lenient: bool,
) -> Result<()> {
    if let Some(aot) = item.as_array_of_tables_mut() {
        let pos = aot.iter().position(|t| {
            &Value::Object(
                t.iter()
                    .map(|(k, v)| (k.to_string(), toml_item_to_json(v)))
                    .collect(),
            ) == value
        });
        return match (append, pos) {
            (true, Some(_)) => Ok(()),
            (true, None) => {
                let toml_edit::Item::Table(t) = json_to_toml_item(value, false)? else {
                    bail!(
                        "'{}' is an array of tables; entries must be objects",
                        key_path
                    );
                };
                aot.push(t);
                Ok(())
            }
            (false, Some(idx)) => {
                aot.remove(idx);
                Ok(())
            }
            (false, None) => not_found(lenient, format!("entry not found in '{}'", key_path)),
        };
    }
    let arr = item
        .as_array_mut()
        .ok_or_else(|| anyhow!("'{}' is not a list", key_path))?;
    let pos = arr.iter().position(|v| &toml_value_to_json(v) == value);
    match (append, pos) {
        (true, Some(_)) => Ok(()),
        (true, None) => {
            arr.push(json_to_toml_value(value)?);
            Ok(())
        }
        (false, Some(idx)) => {
            arr.remove(idx);
            Ok(())
        }
        (false, None) => not_found(lenient, format!("entry not found in '{}'", key_path)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const ID: &str = "11111111-2222-3333-4444-555555555555";

    fn set(path: &str, v: Value) -> ChangeOp {
        ChangeOp::Set {
            key_path: path.into(),
            value: v,
        }
    }

    fn apply(kind: ConfigFileKind, text: &str, op: ChangeOp) -> EditOutcome {
        apply_op(kind, Some(text), &op, ID).unwrap()
    }

    //
    // Apply an op, then its recorded undo, and require the original bytes
    // back. Holds for JSON/JSONC/TOML when nothing else touched the file.
    //

    fn round_trip(kind: ConfigFileKind, text: &str, op: ChangeOp) -> String {
        let out = apply(kind, text, op);
        let undone = apply_undo(kind, &out.content, out.undo.as_ref().unwrap()).unwrap();
        assert_eq!(undone, text, "undo did not restore original for {:?}", kind);
        out.content
    }

    const JSONC: &str = r#"{
  // Model selection
  "model": "sonnet", /* trailing */
  "permissions": {
    "allow": ["Bash(ls)"],
    "deny": []
  },
  "env": {
    "EXISTING": "1"
  }
}
"#;

    #[test]
    fn jsonc_set_preserves_comments_and_layout() {
        let out = round_trip(
            ConfigFileKind::Jsonc,
            JSONC,
            set("/env/PRAXIS_DEMO", json!("1")),
        );
        assert!(out.contains("// Model selection"));
        assert!(out.contains("/* trailing */"));
        assert!(out.contains("\"EXISTING\": \"1\""));
        assert!(out.contains("\"PRAXIS_DEMO\": \"1\""));
        let v = semantic(ConfigFileKind::Jsonc, &out).unwrap();
        assert_eq!(v["model"], "sonnet");
        assert_eq!(v["permissions"]["allow"], json!(["Bash(ls)"]));
    }

    #[test]
    fn jsonc_add_and_remove_keyed_entry() {
        let op = set(
            "/mcpServers/demo",
            json!({"command": "echo", "args": ["hello"]}),
        );
        let out = round_trip(ConfigFileKind::Json, JSONC, op);
        let v = semantic(ConfigFileKind::Json, &out).unwrap();
        assert_eq!(v["mcpServers"]["demo"]["command"], "echo");

        let removed = apply(
            ConfigFileKind::Json,
            &out,
            ChangeOp::Remove {
                key_path: "/mcpServers/demo".into(),
            },
        );
        let v = semantic(ConfigFileKind::Json, &removed.content).unwrap();
        assert_eq!(v["mcpServers"], json!({}));
    }

    #[test]
    fn jsonc_list_append_remove_and_idempotence() {
        let op = ChangeOp::ListAppend {
            key_path: "/permissions/allow".into(),
            value: json!("Bash(pwd)"),
        };
        let out = round_trip(ConfigFileKind::Jsonc, JSONC, op.clone());
        let v = semantic(ConfigFileKind::Jsonc, &out).unwrap();
        assert_eq!(v["permissions"]["allow"], json!(["Bash(ls)", "Bash(pwd)"]));

        //
        // Appending an entry that already exists changes nothing and
        // records no undo work.
        //

        let again = apply(ConfigFileKind::Jsonc, &out, op);
        assert_eq!(again.content, out);
        assert!(again.undo.unwrap().is_empty());

        round_trip(
            ConfigFileKind::Jsonc,
            JSONC,
            ChangeOp::ListRemove {
                key_path: "/permissions/allow".into(),
                value: json!("Bash(ls)"),
            },
        );
    }

    #[test]
    fn jsonc_merge_records_per_leaf_undo() {
        let op = ChangeOp::Merge {
            key_path: "/env".into(),
            value: json!({"EXISTING": "2", "NEW": "x"}),
        };
        let out = round_trip(ConfigFileKind::Jsonc, JSONC, op);
        let v = semantic(ConfigFileKind::Jsonc, &out).unwrap();
        assert_eq!(v["env"], json!({"EXISTING": "2", "NEW": "x"}));
    }

    #[test]
    fn json_creates_missing_file_and_nested_parents() {
        let out = apply_op(ConfigFileKind::Json, None, &set("/a/b/c", json!(true)), ID).unwrap();
        assert_eq!(
            semantic(ConfigFileKind::Json, &out.content).unwrap(),
            json!({"a": {"b": {"c": true}}})
        );
        assert_eq!(
            out.undo.unwrap(),
            vec![ChangeOp::Remove {
                key_path: "/a".into()
            }]
        );
    }

    #[test]
    fn remove_missing_key_is_error_but_lenient_in_undo() {
        let err = apply_op(
            ConfigFileKind::Json,
            Some("{}"),
            &ChangeOp::Remove {
                key_path: "/nope".into(),
            },
            ID,
        );
        assert!(err.is_err());
        let undone = apply_undo(
            ConfigFileKind::Json,
            "{}",
            &[ChangeOp::Remove {
                key_path: "/nope".into(),
            }],
        )
        .unwrap();
        assert_eq!(undone, "{}");
    }

    #[test]
    fn set_through_scalar_is_rejected() {
        assert!(
            apply_op(
                ConfigFileKind::Json,
                Some(r#"{"model": "x"}"#),
                &set("/model/inner", json!(1)),
                ID
            )
            .is_err()
        );
    }

    const TOML: &str = r#"# Codex config
model = "o3" # preferred model

[mcp_servers.existing]
command = "existing-server"
args = ["--flag"]

[[profiles]]
name = "default"
"#;

    #[test]
    fn toml_add_table_entry_preserves_rest() {
        let out = round_trip(
            ConfigFileKind::Toml,
            TOML,
            set(
                "/mcp_servers/demo",
                json!({"command": "echo", "args": ["hello"]}),
            ),
        );
        assert!(out.contains("# Codex config"));
        assert!(out.contains("model = \"o3\" # preferred model"));
        assert!(out.contains("[mcp_servers.existing]"));
        assert!(out.contains("[mcp_servers.demo]"));
        let v = semantic(ConfigFileKind::Toml, &out).unwrap();
        assert_eq!(v["mcp_servers"]["demo"]["args"], json!(["hello"]));
        assert_eq!(v["profiles"][0]["name"], "default");
    }

    #[test]
    fn toml_scalar_set_keeps_trailing_comment() {
        let out = round_trip(ConfigFileKind::Toml, TOML, set("/model", json!("o4")));
        assert!(out.contains("# Codex config\nmodel = \"o4\" # preferred model"));
    }

    #[test]
    fn toml_env_var_and_list_ops() {
        let out = round_trip(
            ConfigFileKind::Toml,
            TOML,
            set("/shell_environment_policy/set/PRAXIS_DEMO", json!("1")),
        );
        let v = semantic(ConfigFileKind::Toml, &out).unwrap();
        assert_eq!(v["shell_environment_policy"]["set"]["PRAXIS_DEMO"], "1");

        round_trip(
            ConfigFileKind::Toml,
            TOML,
            ChangeOp::ListAppend {
                key_path: "/mcp_servers/existing/args".into(),
                value: json!("--verbose"),
            },
        );
        let aot = round_trip(
            ConfigFileKind::Toml,
            TOML,
            ChangeOp::ListAppend {
                key_path: "/profiles".into(),
                value: json!({"name": "praxis"}),
            },
        );
        assert!(aot.matches("[[profiles]]").count() == 2);
    }

    #[test]
    fn toml_rejects_null() {
        assert!(
            apply_op(
                ConfigFileKind::Toml,
                Some(TOML),
                &set("/x", Value::Null),
                ID
            )
            .is_err()
        );
    }

    const YAML: &str = "name: demo\nmodels:\n  - name: a\n    provider: x\nextensions:\n  dev:\n    enabled: true\n";

    #[test]
    fn yaml_preserves_data_and_key_order() {
        let out = apply(
            ConfigFileKind::Yaml,
            YAML,
            set("/extensions/demo", json!({"cmd": "echo", "enabled": false})),
        );
        let v = semantic(ConfigFileKind::Yaml, &out.content).unwrap();
        assert_eq!(v["models"][0]["provider"], "x");
        assert_eq!(v["extensions"]["dev"]["enabled"], true);
        assert_eq!(v["extensions"]["demo"]["cmd"], "echo");
        let name = out.content.find("name:").unwrap();
        let models = out.content.find("models:").unwrap();
        let ext = out.content.find("extensions:").unwrap();
        assert!(
            name < models && models < ext,
            "key order changed:\n{}",
            out.content
        );

        //
        // YAML is re-emitted, so the undo is checked semantically.
        //

        let undone = apply_undo(ConfigFileKind::Yaml, &out.content, &out.undo.unwrap()).unwrap();
        assert_eq!(
            semantic(ConfigFileKind::Yaml, &undone).unwrap(),
            semantic(ConfigFileKind::Yaml, YAML).unwrap()
        );
    }

    #[test]
    fn yaml_list_ops() {
        let out = apply(
            ConfigFileKind::Yaml,
            YAML,
            ChangeOp::ListAppend {
                key_path: "/models".into(),
                value: json!({"name": "b", "provider": "y"}),
            },
        );
        let v = semantic(ConfigFileKind::Yaml, &out.content).unwrap();
        assert_eq!(v["models"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn structured_undo_survives_unrelated_later_edit() {
        let out = apply(
            ConfigFileKind::Jsonc,
            JSONC,
            set("/env/PRAXIS_DEMO", json!("1")),
        );

        //
        // Someone else edits a different key after Praxis wrote the file.
        //

        let later = apply(
            ConfigFileKind::Jsonc,
            &out.content,
            set("/model", json!("opus")),
        );
        let undone = apply_undo(ConfigFileKind::Jsonc, &later.content, &out.undo.unwrap()).unwrap();
        let v = semantic(ConfigFileKind::Jsonc, &undone).unwrap();
        assert_eq!(v["model"], "opus");
        assert!(v["env"].get("PRAXIS_DEMO").is_none());
        assert!(undone.contains("// Model selection"));
    }

    #[test]
    fn marker_block_add_remove_is_exact() {
        for original in ["", "# Notes\n", "# Notes\nno trailing newline"] {
            let out = apply(
                ConfigFileKind::Markdown,
                original,
                ChangeOp::AppendBlock {
                    content: "<!-- harmless example -->".into(),
                },
            );
            assert!(out.content.starts_with(original));
            assert!(out.content.contains(&block_begin_marker(ID)));
            assert!(
                out.content
                    .ends_with(&format!("{}\n", block_end_marker(ID)))
            );
            let undone =
                apply_undo(ConfigFileKind::Markdown, &out.content, &out.undo.unwrap()).unwrap();
            assert_eq!(undone, original);
        }
    }

    #[test]
    fn marker_block_removal_keeps_surrounding_edits() {
        let out = apply(
            ConfigFileKind::Markdown,
            "# Top\n",
            ChangeOp::AppendBlock {
                content: "line one\nline two\n".into(),
            },
        );
        let edited = format!("intro\n{}more after\n", out.content);
        let undone = apply_undo(ConfigFileKind::Markdown, &edited, &out.undo.unwrap()).unwrap();
        assert_eq!(undone, "intro\n# Top\nmore after\n");
    }

    #[test]
    fn remove_block_by_id_and_duplicate_guard() {
        let out = apply(
            ConfigFileKind::Text,
            "x\n",
            ChangeOp::AppendBlock {
                content: "y".into(),
            },
        );
        assert!(
            apply_op(
                ConfigFileKind::Text,
                Some(&out.content),
                &ChangeOp::AppendBlock {
                    content: "z".into()
                },
                ID
            )
            .is_err()
        );
        let removed = apply(
            ConfigFileKind::Text,
            &out.content,
            ChangeOp::RemoveBlock {
                block_id: ID.into(),
                strip_leading_newline: false,
            },
        );
        assert_eq!(removed.content, "x\n");
        assert!(
            apply_op(
                ConfigFileKind::Text,
                Some("x\n"),
                &ChangeOp::RemoveBlock {
                    block_id: ID.into(),
                    strip_leading_newline: false
                },
                ID
            )
            .is_err()
        );
    }

    #[test]
    fn op_kind_mismatches_are_rejected() {
        assert!(
            apply_op(
                ConfigFileKind::Markdown,
                Some("x"),
                &set("/a", json!(1)),
                ID
            )
            .is_err()
        );
        assert!(
            apply_op(
                ConfigFileKind::Json,
                Some("{}"),
                &ChangeOp::AppendBlock {
                    content: "x".into()
                },
                ID
            )
            .is_err()
        );
    }

    #[test]
    fn unified_diff_marks_changes() {
        let d = unified_diff("a\nb\n", "a\nc\n", "before", "after");
        assert!(d.contains("-b"));
        assert!(d.contains("+c"));
    }
}
