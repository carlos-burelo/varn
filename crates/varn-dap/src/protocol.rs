use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Deserialize)]
pub struct Request {
    pub seq: i64,
    pub command: String,
    pub arguments: Option<Value>,
}

#[derive(Debug, Serialize)]
pub struct Response {
    pub seq: i64,
    pub r#type: &'static str,
    pub request_seq: i64,
    pub success: bool,
    pub command: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body: Option<Value>,
}

#[derive(Debug, Serialize, Clone)]
pub struct Event {
    pub seq: i64,
    pub r#type: &'static str,
    pub event: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body: Option<Value>,
}

impl Response {
    pub fn ok(seq: i64, request_seq: i64, command: &str, body: Option<Value>) -> Self {
        Self {
            seq,
            r#type: "response",
            request_seq,
            success: true,
            command: command.to_owned(),
            message: None,
            body,
        }
    }

    pub fn err(seq: i64, request_seq: i64, command: &str, message: String) -> Self {
        Self {
            seq,
            r#type: "response",
            request_seq,
            success: false,
            command: command.to_owned(),
            message: Some(message),
            body: None,
        }
    }
}

impl Event {
    pub fn new(seq: i64, event: &'static str, body: Option<Value>) -> Self {
        Self {
            seq,
            r#type: "event",
            event,
            body,
        }
    }
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct LaunchArgs {
    pub program: Option<String>,
    #[serde(default)]
    pub stop_on_entry: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Source {
    pub path: Option<String>,
    pub name: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetBreakpointsArgs {
    pub source: Source,
    pub breakpoints: Option<Vec<SourceBreakpoint>>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceBreakpoint {
    pub line: u32,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadIdArgs {
    pub thread_id: i64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FrameIdArgs {
    pub frame_id: i64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VariablesArgs {
    pub variables_reference: i64,
}
