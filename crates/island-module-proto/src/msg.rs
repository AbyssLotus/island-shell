//! JSON-RPC 2.0 messages, and the typed params of the protocol's own methods (§4.6–§4.9).

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::code;

#[derive(Debug, Clone, PartialEq)]
pub enum Message {
    /// Expects a response with the same id.
    Request { id: u64, method: String, params: Value },
    Notification { method: String, params: Value },
    Response { id: u64, result: Result<Value, RpcError> },
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct RpcError {
    pub code: i64,
    pub message: String,
}

impl RpcError {
    pub fn new(code: i64, message: impl Into<String>) -> Self {
        RpcError { code, message: message.into() }
    }
    /// A failure whose message the UI shows as is.
    pub fn failed(message: impl Into<String>) -> Self {
        Self::new(code::FAILED, message)
    }
    pub fn not_set_up(message: impl Into<String>) -> Self {
        Self::new(code::NOT_SET_UP, message)
    }
    pub fn offline(message: impl Into<String>) -> Self {
        Self::new(code::OFFLINE, message)
    }
    pub fn unknown_method(method: &str) -> Self {
        Self::new(code::UNKNOWN_METHOD, format!("Unknown method {method}"))
    }
    pub fn bad_params(why: impl std::fmt::Display) -> Self {
        Self::new(code::BAD_PARAMS, format!("Bad params: {why}"))
    }
}

impl std::fmt::Display for RpcError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for RpcError {}

impl From<String> for RpcError {
    fn from(message: String) -> Self {
        RpcError::failed(message)
    }
}

impl From<&str> for RpcError {
    fn from(message: &str) -> Self {
        RpcError::failed(message)
    }
}

impl Message {
    pub fn request(id: u64, method: &str, params: impl Serialize) -> Self {
        Message::Request { id, method: method.into(), params: to_value(params) }
    }
    pub fn notification(method: &str, params: impl Serialize) -> Self {
        Message::Notification { method: method.into(), params: to_value(params) }
    }
    pub fn ok(id: u64, result: impl Serialize) -> Self {
        Message::Response { id, result: Ok(to_value(result)) }
    }
    pub fn err(id: u64, error: RpcError) -> Self {
        Message::Response { id, result: Err(error) }
    }

    pub fn to_json(&self) -> Value {
        let mut m = Map::new();
        m.insert("jsonrpc".into(), "2.0".into());
        match self {
            Message::Request { id, method, params } => {
                m.insert("id".into(), (*id).into());
                m.insert("method".into(), method.clone().into());
                if !params.is_null() {
                    m.insert("params".into(), params.clone());
                }
            }
            Message::Notification { method, params } => {
                m.insert("method".into(), method.clone().into());
                if !params.is_null() {
                    m.insert("params".into(), params.clone());
                }
            }
            Message::Response { id, result } => {
                m.insert("id".into(), (*id).into());
                match result {
                    Ok(v) => m.insert("result".into(), v.clone()),
                    Err(e) => m.insert("error".into(), serde_json::to_value(e).unwrap_or(Value::Null)),
                };
            }
        }
        Value::Object(m)
    }

    /// Classifies a parsed JSON object. Ids are unsigned integers (both sides only ever send those).
    pub fn from_json(v: Value) -> Result<Self, String> {
        let Value::Object(mut m) = v else { return Err("a message must be a JSON object".into()) };
        let id = match m.remove("id") {
            None | Some(Value::Null) => None,
            Some(Value::Number(n)) => Some(n.as_u64().ok_or("ids are unsigned integers")?),
            Some(_) => return Err("ids are unsigned integers".into()),
        };
        let params = m.remove("params").unwrap_or(Value::Null);
        match (m.remove("method"), id) {
            (Some(Value::String(method)), Some(id)) => Ok(Message::Request { id, method, params }),
            (Some(Value::String(method)), None) => Ok(Message::Notification { method, params }),
            (Some(_), _) => Err("method must be a string".into()),
            (None, Some(id)) => {
                if let Some(e) = m.remove("error") {
                    let e: RpcError = serde_json::from_value(e).map_err(|e| format!("bad error object: {e}"))?;
                    Ok(Message::Response { id, result: Err(e) })
                } else if let Some(r) = m.remove("result") {
                    Ok(Message::Response { id, result: Ok(r) })
                } else {
                    Err("a response needs result or error".into())
                }
            }
            (None, None) => Err("not a request, notification or response".into()),
        }
    }
}

fn to_value(v: impl Serialize) -> Value {
    serde_json::to_value(v).unwrap_or(Value::Null)
}

// ---------- typed params ----------

/// `island.init` (base → module).
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Default)]
pub struct Init {
    pub api: u32,
    pub id: String,
    pub base_version: String,
    pub data_dir: String,
    pub cache_dir: String,
    /// Where 0.1.x kept its files; the manifest's `legacy` names are relative to it.
    pub legacy_dir: String,
    /// The manifest's `legacy` list, for convenience.
    #[serde(default)]
    pub legacy: Vec<String>,
    pub locale: String,
    pub theme: Theme,
    /// The module's `settings.json` from its data folder, or `{}`.
    #[serde(default)]
    pub settings: Value,
    /// Hosts the module said it reaches (`hosts` in the manifest); the SDK's HTTP helper allows only these.
    #[serde(default)]
    pub hosts: Vec<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Default)]
pub struct Theme {
    pub dark: bool,
    pub accent: String,
}

/// `island.event` (base → module) and `host.publish` (module → base).
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct TopicData {
    pub topic: String,
    #[serde(default)]
    pub data: Value,
}

/// `host.emit`: becomes the Tauri event `mod:<id>:<name>`.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct Emit {
    pub name: String,
    #[serde(default)]
    pub data: Value,
}

/// `host.notify`: a notification on the island.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Default)]
pub struct Notify {
    pub title: String,
    #[serde(default)]
    pub body: String,
    /// An icon name from the island's set, or a file in the module.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    /// Shows over fullscreen apps too.
    #[serde(default)]
    pub urgent: bool,
    /// `tab:<id>` or `cube:<id>`, opened when the notification is clicked.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub action: Option<String>,
    /// Which notification sound (sounds.js), if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sound: Option<String>,
}

/// `host.activity`: a live item on the pill (§4.8). `{ id, clear: true }` removes it.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Default)]
pub struct Activity {
    pub id: String,
    #[serde(default)]
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub priority: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub art: Option<String>,
    #[serde(default)]
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sub: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub progress: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub action: Option<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub clear: bool,
}

impl Activity {
    /// The default priority of each kind: call 30, timer 20, media 10, custom 5.
    pub fn effective_priority(&self) -> i32 {
        self.priority.unwrap_or(match self.kind.as_str() {
            "call" => 30,
            "timer" => 20,
            "media" => 10,
            _ => 5,
        })
    }
}

/// `host.calendar`: replaces all of a calendar source's events.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Default)]
pub struct CalendarPut {
    pub source: String,
    #[serde(default)]
    pub events: Vec<CalendarEvent>,
}

/// §4.9. Times are RFC 3339.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Default)]
pub struct CalendarEvent {
    pub id: String,
    pub title: String,
    pub start: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end: Option<String>,
    #[serde(default)]
    pub all_day: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    /// "due", "event" or "reset".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
}

/// `host.log`.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct Log {
    /// "error", "warn" or "info".
    pub level: String,
    pub msg: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn round_trip(m: Message) {
        let text = serde_json::to_string(&m.to_json()).unwrap();
        let back = Message::from_json(serde_json::from_str(&text).unwrap()).unwrap();
        assert_eq!(back, m, "{text}");
    }

    #[test]
    fn every_kind_round_trips() {
        round_trip(Message::request(1, "island.init", Init { api: 1, id: "destiny".into(), ..Default::default() }));
        round_trip(Message::request(2, "profile", json!({ "fresh": true })));
        round_trip(Message::request(3, "island.tick", json!({})));
        round_trip(Message::notification("island.event", TopicData { topic: "presence".into(), data: json!({ "state": "away" }) }));
        round_trip(Message::notification("island.shutdown", json!({})));
        round_trip(Message::notification("host.emit", Emit { name: "status".into(), data: json!([1, 2]) }));
        round_trip(Message::notification("host.notify", Notify { title: "Due tonight".into(), body: "Lab 6".into(), urgent: true, action: Some("tab:canvas".into()), ..Default::default() }));
        round_trip(Message::notification("host.activity", Activity { id: "call".into(), kind: "call".into(), text: "In a call".into(), ..Default::default() }));
        round_trip(Message::notification("host.activity", Activity { id: "call".into(), clear: true, ..Default::default() }));
        round_trip(Message::notification(
            "host.calendar",
            CalendarPut { source: "destiny".into(), events: vec![CalendarEvent { id: "reset".into(), title: "Weekly reset".into(), start: "2026-10-27T17:00:00Z".into(), kind: Some("reset".into()), ..Default::default() }] },
        ));
        round_trip(Message::notification("host.publish", TopicData { topic: "media.state".into(), data: json!({ "playing": true }) }));
        round_trip(Message::notification("host.log", Log { level: "warn".into(), msg: "slow".into() }));
        round_trip(Message::ok(2, json!({ "name": "Guardian" })));
        round_trip(Message::ok(3, Value::Null));
        round_trip(Message::err(4, RpcError::not_set_up("Add your Bungie key")));
    }

    #[test]
    fn classification() {
        let m = |v: Value| Message::from_json(v);
        assert!(matches!(m(json!({ "jsonrpc": "2.0", "id": 7, "method": "x" })), Ok(Message::Request { id: 7, .. })));
        assert!(matches!(m(json!({ "jsonrpc": "2.0", "method": "x" })), Ok(Message::Notification { .. })));
        assert_eq!(m(json!({ "jsonrpc": "2.0", "id": 7, "result": null })), Ok(Message::Response { id: 7, result: Ok(Value::Null) }));
        assert!(m(json!({ "id": 7 })).is_err());
        assert!(m(json!({ "id": -1, "method": "x" })).is_err());
        assert!(m(json!({ "id": "a", "method": "x" })).is_err());
        assert!(m(json!({ "method": 5 })).is_err());
        assert!(m(json!([1, 2])).is_err());
        assert!(m(json!({ "id": 1, "error": { "code": "x" } })).is_err());
    }

    #[test]
    fn priorities() {
        let a = |kind: &str| Activity { kind: kind.into(), ..Default::default() }.effective_priority();
        assert_eq!((a("call"), a("timer"), a("media"), a("custom"), a("")), (30, 20, 10, 5, 5));
        assert_eq!(Activity { kind: "media".into(), priority: Some(40), ..Default::default() }.effective_priority(), 40);
    }
}
