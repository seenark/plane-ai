use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use uuid::Uuid;

pub type HmacSha256 = Hmac<Sha256>;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlaneWebhookPayload {
    pub event: String,
    pub action: String,
    pub webhook_id: Uuid,
    pub workspace_id: Uuid,
    pub workspace_slug: String,
    pub data: serde_json::Value,
    pub activity: Option<serde_json::Value>,
}

pub fn verify_signature(secret: &str, raw_body: &[u8], signature_hex: &str) -> bool {
    let Ok(expected_bytes) = hex::decode(signature_hex) else {
        return false;
    };
    let Ok(mut mac) = HmacSha256::new_from_slice(secret.as_bytes()) else {
        return false;
    };
    mac.update(raw_body);
    mac.verify_slice(&expected_bytes).is_ok()
}
