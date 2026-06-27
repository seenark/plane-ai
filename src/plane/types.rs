use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Priority {
    None,
    Low,
    Medium,
    High,
    Urgent,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Project {
    pub id: Uuid,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub identifier: Option<String>,
    #[serde(default)]
    pub archived_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct State {
    pub id: Uuid,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub group: Option<String>,
    #[serde(default)]
    pub color: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Label {
    pub id: Uuid,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub color: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectMember {
    pub id: Uuid,
    #[serde(default)]
    pub member_id: Option<Uuid>,
    #[serde(default)]
    pub role: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Comment {
    #[serde(default)]
    pub id: Option<Uuid>,
    #[serde(default)]
    pub comment_html: Option<String>,
    #[serde(default)]
    pub created_at: Option<String>,
    #[serde(default)]
    pub actor: Option<Value>,
    #[serde(default)]
    pub external_id: Option<String>,
}

impl Comment {
    pub fn from_conflict_id(id: Uuid) -> Self {
        Self {
            id: Some(id),
            comment_html: None,
            created_at: None,
            actor: None,
            external_id: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Activity {
    pub id: Uuid,
    #[serde(default)]
    pub verb: Option<String>,
    #[serde(default)]
    pub field: Option<String>,
    #[serde(default)]
    pub actor: Option<Value>,
    #[serde(default)]
    pub created_at: Option<String>,
    #[serde(flatten)]
    pub extra: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct WorkItem {
    pub id: Uuid,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub description_html: Option<String>,
    #[serde(default)]
    pub priority: Option<Priority>,
    #[serde(default)]
    pub state: Option<Uuid>,
    #[serde(default)]
    pub state_id: Option<Uuid>,
    #[serde(default)]
    pub assignees: Vec<Value>,
    #[serde(default)]
    pub labels: Vec<Value>,
    #[serde(default)]
    pub archived_at: Option<String>,
    #[serde(default)]
    pub target_date: Option<String>,
}

impl WorkItem {
    pub fn current_state_id(&self) -> Option<Uuid> {
        self.state.or(self.state_id)
    }

    pub fn label_ids(&self) -> Vec<Uuid> {
        extract_uuid_list(&self.labels)
    }

    pub fn assignee_ids(&self) -> Vec<Uuid> {
        extract_uuid_list(&self.assignees)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct WorkItemPatch {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state: Option<Uuid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub assignees: Option<Vec<Uuid>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub labels: Option<Vec<Uuid>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub priority: Option<Priority>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_date: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CreateCommentRequest<'a> {
    pub comment_html: &'a str,
    pub external_source: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub external_id: Option<&'a str>,
}

impl<'a> CreateCommentRequest<'a> {
    pub fn new(comment_html: &'a str, external_id: Option<&'a str>) -> Self {
        Self {
            comment_html,
            external_source: "plane-ai",
            external_id,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct IdResponse {
    #[serde(default, deserialize_with = "deserialize_optional_uuid")]
    pub id: Option<Uuid>,
}

fn deserialize_optional_uuid<'de, D>(deserializer: D) -> Result<Option<Uuid>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = Option::<Value>::deserialize(deserializer)?;
    Ok(value.and_then(|value| extract_uuid(&value)))
}

fn extract_uuid_list(values: &[Value]) -> Vec<Uuid> {
    values.iter().filter_map(extract_uuid).collect()
}

fn extract_uuid(value: &Value) -> Option<Uuid> {
    match value {
        Value::String(raw) => Uuid::parse_str(raw).ok(),
        Value::Object(map) => map
            .get("id")
            .and_then(extract_uuid)
            .or_else(|| map.get("member_id").and_then(extract_uuid))
            .or_else(|| map.get("user").and_then(extract_uuid)),
        _ => None,
    }
}
