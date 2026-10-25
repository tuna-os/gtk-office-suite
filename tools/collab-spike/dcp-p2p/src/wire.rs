//! JSON for `Stamped<T>`, which dcp-core does not make serialisable: the encoding the node process
//! (`src/bin/dcp-p2p-node.rs`) and the relay spike's driver (`tests/wan.rs`) share.

use dcp_core::Stamped;
use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::{json, Value};

pub fn stamped_to_json<T: Serialize>(s: &Stamped<T>) -> Value {
    json!({
        "id": s.id,
        "ts_ms": s.ts_ms,
        "sender": s.sender,
        "body": serde_json::to_value(&s.body).unwrap_or(Value::Null),
        "device": s.device,
    })
}

pub fn stamped_from_json<T: DeserializeOwned>(v: &Value) -> Result<Stamped<T>, String> {
    let text = |k: &str| v.get(k).and_then(Value::as_str).map(str::to_owned).ok_or_else(|| format!("stamped: missing {k}"));
    Ok(Stamped {
        id: text("id")?,
        ts_ms: v.get("ts_ms").and_then(Value::as_u64).ok_or("stamped: missing ts_ms")?,
        sender: text("sender")?,
        body: serde_json::from_value(v.get("body").cloned().unwrap_or(Value::Null)).map_err(|e| format!("stamped body: {e}"))?,
        device: v.get("device").and_then(Value::as_str).map(str::to_owned),
    })
}

pub fn stamped_list_to_json<T: Serialize>(v: &[Stamped<T>]) -> Value {
    Value::Array(v.iter().map(stamped_to_json).collect())
}

pub fn stamped_list_from_json<T: DeserializeOwned>(v: &Value) -> Result<Vec<Stamped<T>>, String> {
    v.as_array().ok_or("expected an array")?.iter().map(stamped_from_json).collect()
}
