use hmac::{Hmac, Mac};
use httpmock::{Method::PATCH, Method::POST, MockServer};
use plane_ai::agent::trigger::{contains_agent_mention, strip_html_for_mentions};
use plane_ai::plane::{
    client::PlaneClient,
    types::{Priority, WorkItemPatch},
    webhooks::verify_signature,
};
use serde_json::json;
use sha2::Sha256;
use url::Url;
use uuid::Uuid;

type HmacSha256 = Hmac<Sha256>;

#[test]
fn contains_agent_mention_matches_boundary_token() {
    let mentions = vec!["@agent".to_string(), "@ai".to_string()];
    assert!(contains_agent_mention(
        "<p>@agent please investigate</p>",
        &mentions
    ));
    assert!(!contains_agent_mention(
        "<p>@agentic is not a trigger</p>",
        &mentions
    ));
}

#[test]
fn strip_html_for_mentions_returns_plain_text() {
    let plain = strip_html_for_mentions("<p>Hello <strong>@agent</strong></p>");
    assert!(plain.contains("Hello"));
    assert!(plain.contains("@agent"));
}

#[test]
fn verify_signature_accepts_valid_hmac_and_rejects_tamper() {
    let secret = "topsecret";
    let body = br#"{\"event\":\"issue_comment\"}"#;
    let mut mac = HmacSha256::new_from_slice(secret.as_bytes()).unwrap();
    mac.update(body);
    let signature = hex::encode(mac.finalize().into_bytes());

    assert!(verify_signature(secret, body, &signature));
    assert!(!verify_signature(
        secret,
        br#"{\"event\":\"issue\"}"#,
        &signature
    ));
}

#[tokio::test]
async fn create_comment_sends_expected_headers_and_payload() {
    let server = MockServer::start_async().await;
    let workspace = "demo";
    let project_id = Uuid::parse_str("11111111-1111-1111-1111-111111111111").unwrap();
    let item_id = Uuid::parse_str("22222222-2222-2222-2222-222222222222").unwrap();
    let comment_id = Uuid::parse_str("33333333-3333-3333-3333-333333333333").unwrap();
    let external_id = "accepted-123";

    let mock = server
        .mock_async(|when, then| {
            when.method(POST)
                .path(format!(
                    "/api/v1/workspaces/{workspace}/projects/{project_id}/work-items/{item_id}/comments/"
                ))
                .header("x-api-key", "plane_api_123")
                .header("content-type", "application/json")
                .json_body(json!({
                    "comment_html": "<p>Agent accepted this task.</p>",
                    "external_source": "plane-ai",
                    "external_id": external_id,
                }));
            then.status(200)
                .header("content-type", "application/json")
                .json_body(json!({
                    "id": comment_id,
                    "comment_html": "<p>Agent accepted this task.</p>",
                }));
        })
        .await;

    let client = PlaneClient::new(
        Url::parse(&server.base_url()).unwrap(),
        "plane_api_123".to_string(),
    );
    let comment = client
        .create_comment(
            workspace,
            project_id,
            item_id,
            "<p>Agent accepted this task.</p>",
            Some(external_id),
        )
        .await
        .expect("comment should be created");

    mock.assert_async().await;
    assert_eq!(comment.id, Some(comment_id));
    assert_eq!(
        comment.comment_html.as_deref(),
        Some("<p>Agent accepted this task.</p>")
    );
}

#[tokio::test]
async fn create_comment_treats_conflict_as_idempotent_success() {
    let server = MockServer::start_async().await;
    let workspace = "demo";
    let project_id = Uuid::parse_str("11111111-1111-1111-1111-111111111111").unwrap();
    let item_id = Uuid::parse_str("22222222-2222-2222-2222-222222222222").unwrap();
    let comment_id = Uuid::parse_str("33333333-3333-3333-3333-333333333333").unwrap();

    let mock = server
        .mock_async(|when, then| {
            when.method(POST);
            then.status(409)
                .header("content-type", "application/json")
                .json_body(json!({ "id": comment_id, "detail": "duplicate external id" }));
        })
        .await;

    let client = PlaneClient::new(
        Url::parse(&server.base_url()).unwrap(),
        "plane_api_123".to_string(),
    );
    let comment = client
        .create_comment(
            workspace,
            project_id,
            item_id,
            "<p>Duplicate</p>",
            Some("duplicate-1"),
        )
        .await
        .expect("409 should be treated as idempotent success");

    mock.assert_async().await;
    assert_eq!(comment.id, Some(comment_id));
}

#[tokio::test]
async fn patch_work_item_uses_canonical_field_names() {
    let server = MockServer::start_async().await;
    let workspace = "demo";
    let project_id = Uuid::parse_str("11111111-1111-1111-1111-111111111111").unwrap();
    let item_id = Uuid::parse_str("22222222-2222-2222-2222-222222222222").unwrap();
    let state_id = Uuid::parse_str("44444444-4444-4444-4444-444444444444").unwrap();
    let user_id = Uuid::parse_str("55555555-5555-5555-5555-555555555555").unwrap();
    let label_id = Uuid::parse_str("66666666-6666-6666-6666-666666666666").unwrap();

    let mock = server
        .mock_async(|when, then| {
            when.method(PATCH)
                .path(format!(
                    "/api/v1/workspaces/{workspace}/projects/{project_id}/work-items/{item_id}/"
                ))
                .header("x-api-key", "plane_api_123")
                .json_body(json!({
                    "state": state_id,
                    "assignees": [user_id],
                    "labels": [label_id],
                    "priority": "high",
                    "target_date": "2026-07-01",
                }));
            then.status(200)
                .header("content-type", "application/json")
                .json_body(json!({
                    "id": item_id,
                    "name": "Test item",
                    "priority": "high",
                    "state": state_id,
                    "description_html": "<p>Acceptance criteria</p>",
                    "labels": [{ "id": label_id, "name": "existing" }],
                    "assignees": [{ "id": user_id }]
                }));
        })
        .await;

    let client = PlaneClient::new(
        Url::parse(&server.base_url()).unwrap(),
        "plane_api_123".to_string(),
    );
    client
        .patch_work_item(
            workspace,
            project_id,
            item_id,
            WorkItemPatch {
                state: Some(state_id),
                assignees: Some(vec![user_id]),
                labels: Some(vec![label_id]),
                priority: Some(Priority::High),
                target_date: Some("2026-07-01".to_string()),
            },
        )
        .await
        .expect("patch should succeed");

    mock.assert_async().await;
}
