use axum::{
    body::Bytes,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::{get, post, put},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tower_http::trace::TraceLayer;
use uuid::Uuid;

use crate::{
    db::{
        project_controls::{ProjectControl, ProjectControlsRepo, UpsertProjectControl},
        runs::{AgentRun, AgentRunDetail, RunsRepo},
        webhook_events::{InsertWebhookEventResult, NewWebhookEvent, WebhookEventsRepo},
    },
    error::{AppError, AppResult},
    http::AppState,
    plane::webhooks::{verify_signature, PlaneWebhookPayload},
    ui,
    workers::webhook_processor,
};

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/webhooks/plane", post(plane_webhook))
        .route(
            "/api/projects/{project_id}/control",
            put(put_project_control).get(get_project_control),
        )
        .route("/runs", get(list_runs))
        .route("/runs/{run_id}", get(get_run))
        .route("/health", get(health))
        .merge(ui::routes())
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

async fn health() -> impl IntoResponse {
    StatusCode::NO_CONTENT
}

async fn plane_webhook(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> AppResult<StatusCode> {
    if let Some(secret) = state.config.plane_webhook_secret.as_deref() {
        let signature = headers
            .get("X-Plane-Signature")
            .and_then(|value| value.to_str().ok())
            .ok_or_else(|| AppError::Unauthorized("missing webhook signature".to_string()))?;
        if !verify_signature(secret, &body, signature) {
            return Err(AppError::Unauthorized(
                "invalid webhook signature".to_string(),
            ));
        }
    }

    let delivery_id = headers
        .get("X-Plane-Delivery")
        .and_then(|value| value.to_str().ok())
        .map(ToOwned::to_owned);
    let plane_event = headers
        .get("X-Plane-Event")
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .to_string();

    let payload: PlaneWebhookPayload =
        serde_json::from_slice(&body).map_err(|error| AppError::BadRequest(error.to_string()))?;
    let identifiers = extract_identifiers(&payload);

    let new_event = NewWebhookEvent {
        plane_delivery_id: delivery_id,
        plane_event: plane_event,
        plane_action: payload.action.clone(),
        plane_webhook_id: Some(payload.webhook_id),
        plane_workspace_id: payload.workspace_id,
        plane_workspace_slug: payload.workspace_slug.clone(),
        plane_project_id: identifiers.project_id,
        plane_work_item_id: identifiers.work_item_id,
        plane_comment_id: identifiers.comment_id,
        plane_actor_id: identifiers.actor_id,
        payload: serde_json::to_value(&payload)?,
        processing_status: identifiers
            .has_required_ids()
            .then(|| "received".to_string())
            .or_else(|| Some("ignored".to_string())),
        error_message: identifiers
            .has_required_ids()
            .then(|| None)
            .unwrap_or_else(|| Some("missing required Plane identifiers".to_string())),
    };

    let insert_result = WebhookEventsRepo::insert_webhook_event(&state.pool, &new_event).await?;
    let InsertWebhookEventResult::Inserted(event_id) = insert_result else {
        return Ok(StatusCode::NO_CONTENT);
    };

    if !identifiers.has_required_ids() {
        return Ok(StatusCode::NO_CONTENT);
    }

    WebhookEventsRepo::mark_webhook_event_queued(&state.pool, event_id).await?;
    let pool = state.pool.clone();
    tokio::spawn(async move {
        if let Err(error) = webhook_processor::process_webhook_event(state, event_id).await {
            let _ = WebhookEventsRepo::mark_webhook_event_failed(
                &pool,
                event_id,
                Some(&error.to_string()),
            )
            .await;
        }
    });
    Ok(StatusCode::NO_CONTENT)
}

async fn put_project_control(
    State(state): State<AppState>,
    Path(project_id): Path<Uuid>,
    Json(request): Json<PutProjectControlRequest>,
) -> AppResult<Json<ProjectControl>> {
    let control = ProjectControlsRepo::upsert_project_control(
        &state.pool,
        &UpsertProjectControl {
            plane_workspace_slug: request.plane_workspace_slug,
            plane_project_id: project_id,
            source: "automation_db".to_string(),
            tor_markdown: request.tor_markdown,
            approved_scope: request.approved_scope,
            budget_man_days: request.budget_man_days,
            billing_rate_per_day: request.billing_rate_per_day,
            internal_cost_rate_per_day: request.internal_cost_rate_per_day,
            human_reviewer_id: request.human_reviewer_id,
            brief_status: request.brief_status,
        },
    )
    .await?;
    Ok(Json(control))
}

async fn get_project_control(
    State(state): State<AppState>,
    Path(project_id): Path<Uuid>,
) -> AppResult<Json<ProjectControl>> {
    let control = ProjectControlsRepo::get_project_control(&state.pool, project_id)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("project control for {project_id} not found")))?;
    Ok(Json(control))
}

async fn list_runs(State(state): State<AppState>) -> AppResult<Json<Vec<AgentRun>>> {
    Ok(Json(RunsRepo::list_runs(&state.pool, 100).await?))
}

async fn get_run(
    State(state): State<AppState>,
    Path(run_id): Path<Uuid>,
) -> AppResult<Json<AgentRunDetail>> {
    let detail = RunsRepo::get_run_detail(&state.pool, run_id)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("run {run_id} not found")))?;
    Ok(Json(detail))
}

#[derive(Debug, Clone, Deserialize)]
struct PutProjectControlRequest {
    plane_workspace_slug: String,
    tor_markdown: String,
    approved_scope: Value,
    budget_man_days: Option<f64>,
    billing_rate_per_day: Option<f64>,
    internal_cost_rate_per_day: Option<f64>,
    human_reviewer_id: Option<Uuid>,
    brief_status: String,
}

#[derive(Debug, Clone, Copy, Serialize)]
struct WebhookIdentifiers {
    project_id: Option<Uuid>,
    work_item_id: Option<Uuid>,
    comment_id: Option<Uuid>,
    actor_id: Option<Uuid>,
}

impl WebhookIdentifiers {
    fn has_required_ids(self) -> bool {
        self.project_id.is_some() && self.work_item_id.is_some() && self.comment_id.is_some()
    }
}

fn extract_identifiers(payload: &PlaneWebhookPayload) -> WebhookIdentifiers {
    WebhookIdentifiers {
        project_id: extract_uuid(payload.data.get("project")),
        work_item_id: extract_uuid(payload.data.get("issue")),
        comment_id: extract_uuid(payload.data.get("id")),
        actor_id: extract_uuid(payload.data.get("actor")).or_else(|| {
            payload
                .activity
                .as_ref()
                .and_then(|activity| activity.get("actor"))
                .and_then(|value| extract_uuid(Some(value)))
        }),
    }
}

fn extract_uuid(value: Option<&Value>) -> Option<Uuid> {
    match value {
        Some(Value::String(text)) => Uuid::parse_str(text).ok(),
        Some(Value::Object(map)) => map
            .get("id")
            .or_else(|| map.get("project_id"))
            .or_else(|| map.get("issue_id"))
            .and_then(|inner| extract_uuid(Some(inner))),
        _ => None,
    }
}
