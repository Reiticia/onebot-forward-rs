use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Api {
    pub action: String,
    pub params: HashMap<String, Value>,
    pub echo: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiResponse {
    pub status: String,
    pub retcode: i32,
    pub data: Value,
    pub echo: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    pub time: i64,
    pub self_id: i64,
    pub post_type: String,
    pub meta_event_type: Option<String>,
    pub sub_type: Option<String>,
    pub user_id: Option<i64>,
    pub group_id: Option<i64>,
    #[serde(flatten)]
    pub extra: HashMap<String, Value>,
}
