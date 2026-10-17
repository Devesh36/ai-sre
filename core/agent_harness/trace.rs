use serde::{Deserialize, Serialize};
use uuid::Uuid;
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Trace {
    pub run_id: Option<Uuid>,
    pub phase: String,
    pub operation: String,
    pub elapsed_ms: u64,
    pub request_bytes: usize,
    pub evidence_ids: Vec<String>,
}

/// Sanitized typed transport failure; never retains request URLs or credentials.
#[derive(Debug)]
pub struct TransportFault;
impl std::fmt::Display for TransportFault {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("upstream transport failed; check destination, TLS, and timeout")
    }
}
impl std::error::Error for TransportFault {}
