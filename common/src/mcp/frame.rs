use serde_json::{Value, json};

//
// Build an ACP request envelope with the target node id injected into
// `params._meta.praxis.nodeId` so the service-side AcpNodeProxy knows how
// to route it. Any `_meta.praxis.nodeId` already present in `params` is
// preserved.
//

pub fn build_request_frame(
    request_id: &str,
    node_id: &str,
    method: &str,
    mut params: Value,
) -> Value {
    inject_node_id(&mut params, node_id);
    json!({
        "jsonrpc": "2.0",
        "id": request_id,
        "method": method,
        "params": params,
    })
}

pub fn build_notification_frame(node_id: &str, method: &str, mut params: Value) -> Value {
    inject_node_id(&mut params, node_id);
    json!({
        "jsonrpc": "2.0",
        "method": method,
        "params": params,
    })
}

fn inject_node_id(params: &mut Value, node_id: &str) {
    if !params.is_object() {
        *params = json!({});
    }
    let obj = params.as_object_mut().unwrap();
    let meta = obj.entry("_meta").or_insert_with(|| json!({}));
    if !meta.is_object() {
        *meta = json!({});
    }
    let meta_obj = meta.as_object_mut().unwrap();
    let praxis = meta_obj.entry("praxis").or_insert_with(|| json!({}));
    if !praxis.is_object() {
        *praxis = json!({});
    }
    let praxis_obj = praxis.as_object_mut().unwrap();
    if !praxis_obj.contains_key("nodeId") {
        praxis_obj.insert("nodeId".to_string(), Value::String(node_id.to_string()));
    }
}
