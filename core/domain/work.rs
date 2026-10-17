use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
#[derive(Clone, Serialize, Deserialize)]
pub struct WorkItem {
    pub id: Uuid,
    pub service: String,
    pub text: String,
    pub done: bool,
    pub created_at: DateTime<Utc>,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Memory {
    pub id: Uuid,
    pub service: String,
    pub key: String,
    pub text: String,
    pub updated_at: DateTime<Utc>,
}
