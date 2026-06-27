use httpmock::{Method::GET, Method::PATCH, Method::POST, MockServer};
use plane_ai::{
    agent::runner::Runner,
    config::Config,
    db::{
        project_controls::{ProjectControlsRepo, UpsertProjectControl},
        runs::{NewAgentRun, RunsRepo},
        webhook_events::{InsertWebhookEventResult, NewWebhookEvent, WebhookEventsRepo},
    },
    http::AppState,
    plane::{webhooks::PlaneWebhookPayload, PlaneClient},
    workers::webhook_processor,
};
use serde_json::json;
use sqlx::{postgres::PgPoolOptions, PgPool, Row};
use std::sync::{LazyLock, Mutex};
use url::Url;
use uuid::Uuid;

static TEST_DB_LOCK: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));
#[ignore = "requires Postgres"]
#[tokio::test]
async fn fake_runner_webhook_flow_succeeds_and_is_idempotent() {
    let server = MockServer::start_async().await;
    let _guard = TEST_DB_LOCK.lock().unwrap();
    let database_url = std::env::var("DATABASE_URL")
        .expect("DATABASE_URL must point at a disposable Postgres database");
    let pool = setup_database(&database_url).await;

    let workspace = "demo";
    let project_id = Uuid::parse_str("11111111-1111-1111-1111-111111111111").unwrap();
    let work_item_id = Uuid::parse_str("22222222-2222-2222-2222-222222222222").unwrap();
    let comment_id = Uuid::parse_str("33333333-3333-3333-3333-333333333333").unwrap();
    let actor_id = Uuid::parse_str("44444444-4444-4444-4444-444444444444").unwrap();
    let existing_label_id = Uuid::parse_str("55555555-5555-5555-5555-555555555555").unwrap();
    let running_label_id = Uuid::parse_str("66666666-6666-6666-6666-666666666666").unwrap();
    let review_ready_label_id = Uuid::parse_str("77777777-7777-7777-7777-777777777777").unwrap();
    let in_progress_state_id = Uuid::parse_str("88888888-8888-8888-8888-888888888888").unwrap();
    let human_review_state_id = Uuid::parse_str("99999999-9999-9999-9999-999999999999").unwrap();

    seed_project_control(&pool, workspace, project_id, true, true).await;
    let state = build_state(
        &database_url,
        &server,
        workspace,
        project_id,
        Uuid::parse_str("aaaaaaaa-0000-0000-0000-000000000000").unwrap(),
    )
    .await;

    let get_work_item_path =
        format!("/api/v1/workspaces/{workspace}/projects/{project_id}/work-items/{work_item_id}/");
    server
        .mock_async(|when, then| {
            when.method(GET).path(get_work_item_path.clone());
            then.status(200)
                .header("content-type", "application/json")
                .json_body(json!({
                    "id": work_item_id,
                    "name": "Automate Plane workflow",
                    "priority": "high",
                    "state": in_progress_state_id,
                    "description_html": "<p>Acceptance criteria</p>",
                    "labels": [{ "id": existing_label_id, "name": "existing" }],
                    "assignees": []
                }));
        })
        .await;
    server
        .mock_async(|when, then| {
            when.method(GET)
                .path(format!(
                    "/api/v1/workspaces/{workspace}/projects/{project_id}/work-items/{work_item_id}/comments/"
                ));
            then.status(200)
                .header("content-type", "application/json")
                .json_body(json!([]));
        })
        .await;
    server
        .mock_async(|when, then| {
            when.method(GET)
                .path(format!(
                    "/api/v1/workspaces/{workspace}/projects/{project_id}/work-items/{work_item_id}/activities/"
                ));
            then.status(200)
                .header("content-type", "application/json")
                .json_body(json!([]));
        })
        .await;
    server
        .mock_async(|when, then| {
            when.method(GET).path(format!(
                "/api/v1/workspaces/{workspace}/projects/{project_id}/labels/"
            ));
            then.status(200)
                .header("content-type", "application/json")
                .json_body(json!([
                    { "id": existing_label_id, "name": "existing" },
                    { "id": running_label_id, "name": "agent-running" },
                    { "id": review_ready_label_id, "name": "agent-review-ready" }
                ]));
        })
        .await;
    server
        .mock_async(|when, then| {
            when.method(GET).path(format!(
                "/api/v1/workspaces/{workspace}/projects/{project_id}/states/"
            ));
            then.status(200)
                .header("content-type", "application/json")
                .json_body(json!([
                    { "id": in_progress_state_id, "name": "In Progress" },
                    { "id": human_review_state_id, "name": "Human Review" }
                ]));
        })
        .await;

    let accepted_comment = server
        .mock_async(|when, then| {
            when.method(POST)
                .path(format!(
                    "/api/v1/workspaces/{workspace}/projects/{project_id}/work-items/{work_item_id}/comments/"
                ))
                .body_contains("Agent accepted this task.");
            then.status(200)
                .header("content-type", "application/json")
                .json_body(json!({ "id": Uuid::new_v4(), "comment_html": "accepted" }));
        })
        .await;
    let completed_comment = server
        .mock_async(|when, then| {
            when.method(POST)
                .path(format!(
                    "/api/v1/workspaces/{workspace}/projects/{project_id}/work-items/{work_item_id}/comments/"
                ))
                .body_contains("Agent completed implementation.");
            then.status(200)
                .header("content-type", "application/json")
                .json_body(json!({ "id": Uuid::new_v4(), "comment_html": "completed" }));
        })
        .await;
    server
        .mock_async(|when, then| {
            when.method(PATCH).path(get_work_item_path.clone());
            then.status(200)
                .header("content-type", "application/json")
                .json_body(json!({ "id": work_item_id, "name": "patched" }));
        })
        .await;

    let payload = make_comment_payload(
        workspace,
        project_id,
        work_item_id,
        comment_id,
        actor_id,
        "<p>@agent summarize the task and propose next steps</p>",
    );
    let first = insert_event(&pool, &payload, Some("delivery-1")).await;
    assert!(matches!(first, InsertWebhookEventResult::Inserted(_)));
    let event_id = match first {
        InsertWebhookEventResult::Inserted(id) => id,
        InsertWebhookEventResult::Duplicate => unreachable!(),
    };
    webhook_processor::process_webhook_event(state.clone(), event_id)
        .await
        .expect("webhook should process");

    let duplicate = insert_event(&pool, &payload, Some("delivery-1")).await;
    assert!(matches!(duplicate, InsertWebhookEventResult::Duplicate));

    accepted_comment.assert_async().await;
    completed_comment.assert_async().await;

    let runs = RunsRepo::list_runs(&pool, 10)
        .await
        .expect("runs should list");
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0].status, "succeeded");
    assert!(runs[0]
        .final_response
        .as_deref()
        .unwrap_or_default()
        .contains("succeeded"));

    let event_count: i64 = sqlx::query("SELECT COUNT(*) AS count FROM plane_webhook_events")
        .fetch_one(&pool)
        .await
        .unwrap()
        .get("count");
    let active_run_count: i64 =
        sqlx::query("SELECT COUNT(*) AS count FROM agent_runs WHERE plane_work_item_id = $1")
            .bind(work_item_id)
            .fetch_one(&pool)
            .await
            .unwrap()
            .get("count");
    assert_eq!(event_count, 1);
    assert_eq!(active_run_count, 1);
}

#[ignore = "requires Postgres"]
#[tokio::test]
async fn missing_project_control_comments_and_creates_no_run() {
    let server = MockServer::start_async().await;
    let _guard = TEST_DB_LOCK.lock().unwrap();
    let database_url = std::env::var("DATABASE_URL")
        .expect("DATABASE_URL must point at a disposable Postgres database");
    let pool = setup_database(&database_url).await;

    let workspace = "demo";
    let project_id = Uuid::parse_str("aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa").unwrap();
    let work_item_id = Uuid::parse_str("bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb").unwrap();
    let comment_id = Uuid::parse_str("cccccccc-cccc-cccc-cccc-cccccccccccc").unwrap();
    let actor_id = Uuid::parse_str("dddddddd-dddd-dddd-dddd-dddddddddddd").unwrap();

    let state = build_state(
        &database_url,
        &server,
        workspace,
        project_id,
        Uuid::parse_str("bbbbbbbb-0000-0000-0000-000000000000").unwrap(),
    )
    .await;
    server
        .mock_async(|when, then| {
            when.method(GET).path(format!(
                "/api/v1/workspaces/{workspace}/projects/{project_id}/work-items/{work_item_id}/"
            ));
            then.status(200)
                .header("content-type", "application/json")
                .json_body(json!({
                    "id": work_item_id,
                    "name": "Need TOR",
                    "description_html": "<p>Acceptance criteria</p>",
                    "labels": [],
                    "assignees": []
                }));
        })
        .await;
    let blocker = server
        .mock_async(|when, then| {
            when.method(POST)
                .path(format!(
                    "/api/v1/workspaces/{workspace}/projects/{project_id}/work-items/{work_item_id}/comments/"
                ))
                .body_contains("Agent needs approved project control before starting. Missing: approved TOR / Project Control for this project.");
            then.status(200)
                .header("content-type", "application/json")
                .json_body(json!({ "id": Uuid::new_v4() }));
        })
        .await;

    let payload = make_comment_payload(
        workspace,
        project_id,
        work_item_id,
        comment_id,
        actor_id,
        "<p>@agent please start</p>",
    );
    let event_id = inserted_event_id(insert_event(&pool, &payload, Some("missing-control")).await);
    webhook_processor::process_webhook_event(state, event_id)
        .await
        .unwrap();

    blocker.assert_async().await;
    let runs = RunsRepo::list_runs(&pool, 10).await.unwrap();
    assert!(runs.is_empty());
}

#[ignore = "requires Postgres"]
#[tokio::test]
async fn missing_acceptance_criteria_comments_and_creates_no_run() {
    let server = MockServer::start_async().await;
    let _guard = TEST_DB_LOCK.lock().unwrap();
    let database_url = std::env::var("DATABASE_URL")
        .expect("DATABASE_URL must point at a disposable Postgres database");
    let pool = setup_database(&database_url).await;

    let workspace = "demo";
    let project_id = Uuid::parse_str("eeeeeeee-eeee-eeee-eeee-eeeeeeeeeeee").unwrap();
    let work_item_id = Uuid::parse_str("ffffffff-ffff-ffff-ffff-ffffffffffff").unwrap();
    let comment_id = Uuid::parse_str("abababab-abab-abab-abab-abababababab").unwrap();
    let actor_id = Uuid::parse_str("cdcdcdcd-cdcd-cdcd-cdcd-cdcdcdcdcdcd").unwrap();
    let blocked_label_id = Uuid::parse_str("12121212-1212-1212-1212-121212121212").unwrap();

    seed_project_control(&pool, workspace, project_id, true, false).await;
    let state = build_state(
        &database_url,
        &server,
        workspace,
        project_id,
        Uuid::parse_str("cccccccc-0000-0000-0000-000000000000").unwrap(),
    )
    .await;
    let work_item_path =
        format!("/api/v1/workspaces/{workspace}/projects/{project_id}/work-items/{work_item_id}/");
    server
        .mock_async(|when, then| {
            when.method(GET).path(work_item_path.clone());
            then.status(200)
                .header("content-type", "application/json")
                .json_body(json!({
                    "id": work_item_id,
                    "name": "Need AC",
                    "description_html": "<p>No checklist</p>",
                    "labels": [],
                    "assignees": []
                }));
        })
        .await;
    server
        .mock_async(|when, then| {
            when.method(GET).path(format!(
                "/api/v1/workspaces/{workspace}/projects/{project_id}/labels/"
            ));
            then.status(200)
                .header("content-type", "application/json")
                .json_body(json!([{ "id": blocked_label_id, "name": "agent-blocked" }]));
        })
        .await;
    let blocker = server
        .mock_async(|when, then| {
            when.method(POST)
                .path(format!(
                    "/api/v1/workspaces/{workspace}/projects/{project_id}/work-items/{work_item_id}/comments/"
                ))
                .body_contains("Agent needs acceptance criteria before starting this work item.");
            then.status(200)
                .header("content-type", "application/json")
                .json_body(json!({ "id": Uuid::new_v4() }));
        })
        .await;
    let blocked_patch = server
        .mock_async(|when, then| {
            when.method(PATCH)
                .path(work_item_path.clone())
                .json_body(json!({ "labels": [blocked_label_id] }));
            then.status(200)
                .header("content-type", "application/json")
                .json_body(json!({ "id": work_item_id, "name": "patched" }));
        })
        .await;

    let payload = make_comment_payload(
        workspace,
        project_id,
        work_item_id,
        comment_id,
        actor_id,
        "<p>@agent please start</p>",
    );
    let event_id = inserted_event_id(insert_event(&pool, &payload, Some("missing-ac")).await);
    webhook_processor::process_webhook_event(state, event_id)
        .await
        .unwrap();

    blocker.assert_async().await;
    blocked_patch.assert_async().await;
    let runs = RunsRepo::list_runs(&pool, 10).await.unwrap();
    assert!(runs.is_empty());
}

#[ignore = "requires Postgres"]
#[tokio::test]
async fn active_run_duplicate_comments_and_creates_no_second_active_run() {
    let server = MockServer::start_async().await;
    let _guard = TEST_DB_LOCK.lock().unwrap();
    let database_url = std::env::var("DATABASE_URL")
        .expect("DATABASE_URL must point at a disposable Postgres database");
    let pool = setup_database(&database_url).await;

    let workspace = "demo";
    let project_id = Uuid::parse_str("13131313-1313-1313-1313-131313131313").unwrap();
    let work_item_id = Uuid::parse_str("14141414-1414-1414-1414-141414141414").unwrap();
    let comment_id = Uuid::parse_str("15151515-1515-1515-1515-151515151515").unwrap();
    let actor_id = Uuid::parse_str("16161616-1616-1616-1616-161616161616").unwrap();

    seed_project_control(&pool, workspace, project_id, true, true).await;
    let state = build_state(
        &database_url,
        &server,
        workspace,
        project_id,
        Uuid::parse_str("dddddddd-0000-0000-0000-000000000000").unwrap(),
    )
    .await;
    server
        .mock_async(|when, then| {
            when.method(GET).path(format!(
                "/api/v1/workspaces/{workspace}/projects/{project_id}/work-items/{work_item_id}/"
            ));
            then.status(200)
                .header("content-type", "application/json")
                .json_body(json!({
                    "id": work_item_id,
                    "name": "Already claimed",
                    "description_html": "<p>Acceptance criteria</p>",
                    "labels": [],
                    "assignees": []
                }));
        })
        .await;
    let duplicate = server
        .mock_async(|when, then| {
            when.method(POST)
                .path(format!(
                    "/api/v1/workspaces/{workspace}/projects/{project_id}/work-items/{work_item_id}/comments/"
                ))
                .body_contains("Agent is already running this task.");
            then.status(200)
                .header("content-type", "application/json")
                .json_body(json!({ "id": Uuid::new_v4() }));
        })
        .await;

    let existing_run = RunsRepo::create_run(
        &pool,
        &NewAgentRun {
            plane_workspace_slug: workspace.to_string(),
            plane_project_id: project_id,
            plane_work_item_id: work_item_id,
            trigger_comment_id: Some(comment_id),
            trigger_user_id: Some(actor_id),
            status: "queued".to_string(),
            runner_mode: "fake".to_string(),
            runner_command: Some("omp".to_string()),
            prompt: None,
        },
    )
    .await
    .unwrap();

    let payload = make_comment_payload(
        workspace,
        project_id,
        work_item_id,
        comment_id,
        actor_id,
        "<p>@agent please start</p>",
    );
    let event_id = inserted_event_id(insert_event(&pool, &payload, Some("duplicate-active")).await);
    webhook_processor::process_webhook_event(state, event_id)
        .await
        .unwrap();

    duplicate.assert_async().await;
    let active_run = RunsRepo::get_active_run(&pool, work_item_id).await.unwrap();
    assert_eq!(active_run.as_ref().map(|run| run.id), Some(existing_run.id));
    let count: i64 =
        sqlx::query("SELECT COUNT(*) AS count FROM agent_runs WHERE plane_work_item_id = $1")
            .bind(work_item_id)
            .fetch_one(&pool)
            .await
            .unwrap()
            .get("count");
    assert_eq!(count, 1);
}

async fn build_state(
    database_url: &str,
    server: &MockServer,
    workspace: &str,
    project_id: Uuid,
    agent_user_id: Uuid,
) -> AppState {
    let config = Config::from_iter(vec![
        ("DATABASE_URL".to_string(), database_url.to_string()),
        ("PLANE_BASE_URL".to_string(), server.base_url()),
        ("PLANE_API_KEY".to_string(), "plane_api_123".to_string()),
        ("PLANE_WORKSPACE_SLUG".to_string(), workspace.to_string()),
        ("ALLOWED_PROJECT_IDS".to_string(), project_id.to_string()),
        ("AGENT_USER_ID".to_string(), agent_user_id.to_string()),
        ("RUNNER_MODE".to_string(), "fake".to_string()),
        ("AGENT_COMMAND".to_string(), "omp".to_string()),
    ])
    .unwrap();
    let pool = PgPoolOptions::new()
        .max_connections(5)
        .connect(database_url)
        .await
        .unwrap();
    AppState {
        plane: PlaneClient::new(Url::parse(&server.base_url()).unwrap(), "plane_api_123"),
        runner: Runner::from_config(&config),
        pool,
        config,
    }
}

async fn setup_database(database_url: &str) -> PgPool {
    let pool = PgPoolOptions::new()
        .max_connections(5)
        .connect(database_url)
        .await
        .unwrap();
    sqlx::migrate!("./migrations").run(&pool).await.unwrap();
    sqlx::query(
        "TRUNCATE TABLE plane_webhook_events, agent_runs, agent_run_events, agent_run_artifacts, agent_approvals, project_controls, scope_change_logs, agent_costs RESTART IDENTITY CASCADE",
    )
    .execute(&pool)
    .await
    .unwrap();
    pool
}

async fn seed_project_control(
    pool: &PgPool,
    workspace: &str,
    project_id: Uuid,
    approved: bool,
    include_acceptance_criteria: bool,
) {
    ProjectControlsRepo::upsert_project_control(
        pool,
        &UpsertProjectControl {
            plane_workspace_slug: workspace.to_string(),
            plane_project_id: project_id,
            source: "automation_db".to_string(),
            tor_markdown: "# TOR".to_string(),
            approved_scope: if include_acceptance_criteria {
                json!({ "acceptance_criteria": ["done"] })
            } else {
                json!({})
            },
            budget_man_days: Some(1.0),
            billing_rate_per_day: Some(1000.0),
            internal_cost_rate_per_day: Some(500.0),
            human_reviewer_id: Some(Uuid::new_v4()),
            brief_status: if approved { "approved" } else { "pending" }.to_string(),
        },
    )
    .await
    .unwrap();
}

fn make_comment_payload(
    workspace: &str,
    project_id: Uuid,
    work_item_id: Uuid,
    comment_id: Uuid,
    actor_id: Uuid,
    comment_html: &str,
) -> PlaneWebhookPayload {
    PlaneWebhookPayload {
        event: "issue_comment".to_string(),
        action: "create".to_string(),
        webhook_id: Uuid::new_v4(),
        workspace_id: Uuid::new_v4(),
        workspace_slug: workspace.to_string(),
        data: json!({
            "project": { "id": project_id },
            "issue": { "id": work_item_id },
            "id": comment_id,
            "actor": { "id": actor_id },
            "comment_html": comment_html,
        }),
        activity: Some(json!({ "actor": { "id": actor_id } })),
    }
}

async fn insert_event(
    pool: &PgPool,
    payload: &PlaneWebhookPayload,
    delivery_id: Option<&str>,
) -> InsertWebhookEventResult {
    WebhookEventsRepo::insert_webhook_event(
        pool,
        &NewWebhookEvent {
            plane_delivery_id: delivery_id.map(str::to_string),
            plane_event: payload.event.clone(),
            plane_action: payload.action.clone(),
            plane_webhook_id: Some(payload.webhook_id),
            plane_workspace_id: payload.workspace_id,
            plane_workspace_slug: payload.workspace_slug.clone(),
            plane_project_id: payload
                .data
                .get("project")
                .and_then(|value| value.get("id"))
                .and_then(|value| value.as_str())
                .and_then(|value| Uuid::parse_str(value).ok()),
            plane_work_item_id: payload
                .data
                .get("issue")
                .and_then(|value| value.get("id"))
                .and_then(|value| value.as_str())
                .and_then(|value| Uuid::parse_str(value).ok()),
            plane_comment_id: payload
                .data
                .get("id")
                .and_then(|value| value.as_str())
                .and_then(|value| Uuid::parse_str(value).ok()),
            plane_actor_id: payload
                .data
                .get("actor")
                .and_then(|value| value.get("id"))
                .and_then(|value| value.as_str())
                .and_then(|value| Uuid::parse_str(value).ok()),
            payload: serde_json::to_value(payload).unwrap(),
            processing_status: Some("received".to_string()),
            error_message: None,
        },
    )
    .await
    .unwrap()
}

fn inserted_event_id(result: InsertWebhookEventResult) -> Uuid {
    match result {
        InsertWebhookEventResult::Inserted(id) => id,
        InsertWebhookEventResult::Duplicate => panic!("expected inserted webhook event"),
    }
}
