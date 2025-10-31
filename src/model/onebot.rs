use std::{collections::HashMap, time::SystemTime};

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Api {
    pub action: String,
    pub params: HashMap<String, Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub echo: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiResponse {
    pub status: String,
    pub retcode: i32,
    pub data: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    pub echo: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    pub time: i64,
    pub self_id: i64,
    pub post_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub meta_event_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sub_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_id: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub group_id: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub raw_message: Option<String>,
    #[serde(flatten)]
    pub extra: HashMap<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(dead_code)]
pub struct TestMessage {
    pub r#type: MessageType,
    #[serde(flatten)]
    pub extra: HashMap<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[allow(dead_code)]
pub enum MessageType {
    Test,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectMessage {
    pub time: i64,
    pub self_id: i64,
    pub post_type: String,
    pub meta_event_type: String,
    pub sub_type: String,
}

impl ConnectMessage {
    pub fn new(user_id: i64) -> Self {
        let start = SystemTime::now();
        let time = start.duration_since(SystemTime::UNIX_EPOCH).unwrap().as_millis() as i64;

        Self {
            time,
            self_id: user_id,
            post_type: "meta_event".into(),
            meta_event_type: "lifecycle".into(),
            sub_type: "connect".into(),
        }
    }
}
