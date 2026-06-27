use serde_json::Value;
use sqlx::Error as SqlxError;
use uuid::Uuid;

use crate::{
    agent::trigger::{contains_agent_mention, strip_html_for_mentions},
    db::{
        project_controls::ProjectControlsRepo,
        runs::{NewAgentRun, ResolveAgentApproval, RunsRepo},
        webhook_events::WebhookEventsRepo,
    },
    error::{AppError, AppResult},
    http::AppState,
    plane::{webhooks::PlaneWebhookPayload, PlaneError},
    workers::run_executor,
};

pub async fn process_webhook_event(state: AppState, event_id: Uuid) -> AppResult<()> {
    let event = WebhookEventsRepo::get_webhook_event(&state.pool, event_id)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("webhook event {event_id} not found")))?;
    let payload: PlaneWebhookPayload = serde_json::from_value(event.payload.clone())
        .map_err(|error| AppError::BadRequest(error.to_string()))?;

    let Some(project_id) = event.plane_project_id else {
        WebhookEventsRepo::mark_webhook_event_ignored(
            &state.pool,
            event_id,
            Some("missing required Plane identifiers"),
        )
        .await?;
        return Ok(());
    };
    let Some(work_item_id) = event.plane_work_item_id else {
        WebhookEventsRepo::mark_webhook_event_ignored(
            &state.pool,
            event_id,
            Some("missing required Plane identifiers"),
        )
        .await?;
        return Ok(());
    };

    let comment_html = payload
        .data
        .get("comment_html")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let comment_text = strip_html_for_mentions(comment_html);
    let normalized_comment = comment_text.trim().to_lowercase();

    if handle_approval_command(&state, &event, &normalized_comment).await? {
        WebhookEventsRepo::mark_webhook_event_processed(&state.pool, event_id).await?;
        return Ok(());
    }

    if payload.event != "issue_comment" || !matches!(payload.action.as_str(), "create" | "update") {
        WebhookEventsRepo::mark_webhook_event_ignored(&state.pool, event_id, None).await?;
        return Ok(());
    }

    if !contains_agent_mention(comment_html, &state.config.agent_mentions) {
        WebhookEventsRepo::mark_webhook_event_ignored(&state.pool, event_id, None).await?;
        return Ok(());
    }

    if event.plane_actor_id == Some(state.config.agent_user_id) {
        WebhookEventsRepo::mark_webhook_event_ignored(&state.pool, event_id, None).await?;
        return Ok(());
    }

    if !state.config.allowed_project_ids.contains(&project_id) {
        WebhookEventsRepo::mark_webhook_event_ignored(&state.pool, event_id, None).await?;
        return Ok(());
    }

    let work_item = match state
        .plane
        .get_work_item(&payload.workspace_slug, project_id, work_item_id)
        .await
    {
        Ok(work_item) => work_item,
        Err(PlaneError::HttpStatus { status, .. }) if status.as_u16() == 404 => {
            WebhookEventsRepo::mark_webhook_event_ignored(&state.pool, event_id, None).await?;
            return Ok(());
        }
        Err(error) => {
            WebhookEventsRepo::mark_webhook_event_failed(
                &state.pool,
                event_id,
                Some(&error.to_string()),
            )
            .await?;
            return Err(AppError::Internal(error.to_string()));
        }
    };

    if work_item.archived_at.is_some() {
        WebhookEventsRepo::mark_webhook_event_ignored(&state.pool, event_id, None).await?;
        return Ok(());
    }

    let Some(project_control) =
        ProjectControlsRepo::get_project_control(&state.pool, project_id).await?
    else {
        comment_project_control_blocker(
            &state,
            &payload.workspace_slug,
            project_id,
            work_item_id,
            event.plane_comment_id,
        )
        .await?;
        WebhookEventsRepo::mark_webhook_event_ignored(
            &state.pool,
            event_id,
            Some("approved project control required"),
        )
        .await?;
        return Ok(());
    };

    if project_control.brief_status != "approved" {
        comment_project_control_blocker(
            &state,
            &payload.workspace_slug,
            project_id,
            work_item_id,
            event.plane_comment_id,
        )
        .await?;
        WebhookEventsRepo::mark_webhook_event_ignored(
            &state.pool,
            event_id,
            Some("approved project control required"),
        )
        .await?;
        return Ok(());
    }

    if !has_acceptance_criteria(&work_item.description_html, &project_control.approved_scope) {
        state
            .plane
            .create_comment(
                &payload.workspace_slug,
                project_id,
                work_item_id,
                "<p>Agent needs acceptance criteria before starting this work item.</p>",
                event
                    .plane_comment_id
                    .map(|comment_id| format!("needs-acceptance-criteria-{comment_id}"))
                    .as_deref(),
            )
            .await
            .map_err(|error| AppError::Internal(error.to_string()))?;
        run_executor::add_label_to_work_item(
            &state,
            &payload.workspace_slug,
            project_id,
            work_item_id,
            None,
            &state.config.agent_blocked_label_name,
        )
        .await?;
        WebhookEventsRepo::mark_webhook_event_ignored(
            &state.pool,
            event_id,
            Some("missing acceptance criteria"),
        )
        .await?;
        return Ok(());
    }

    if let Some(active_run) = RunsRepo::get_active_run(&state.pool, work_item_id).await? {
        let html = format!(
            "<p>Agent is already running this task. Existing run: {}</p>",
            active_run.id
        );
        state
            .plane
            .create_comment(
                &payload.workspace_slug,
                project_id,
                work_item_id,
                &html,
                event
                    .plane_comment_id
                    .map(|comment_id| format!("duplicate-run-{comment_id}"))
                    .as_deref(),
            )
            .await
            .map_err(|error| AppError::Internal(error.to_string()))?;
        WebhookEventsRepo::mark_webhook_event_ignored(
            &state.pool,
            event_id,
            Some("active run already exists"),
        )
        .await?;
        return Ok(());
    }

    let run = match RunsRepo::create_run(
        &state.pool,
        &NewAgentRun {
            plane_workspace_slug: payload.workspace_slug.clone(),
            plane_project_id: project_id,
            plane_work_item_id: work_item_id,
            trigger_comment_id: event.plane_comment_id,
            trigger_user_id: event.plane_actor_id,
            status: "queued".to_string(),
            runner_mode: state.config.runner_mode.as_str().to_string(),
            runner_command: Some(state.config.agent_command.clone()),
            prompt: None,
        },
    )
    .await
    {
        Ok(run) => run,
        Err(SqlxError::Database(error)) if error.code().as_deref() == Some("23505") => {
            if let Some(active_run) = RunsRepo::get_active_run(&state.pool, work_item_id).await? {
                let html = format!(
                    "<p>Agent is already running this task. Existing run: {}</p>",
                    active_run.id
                );
                state
                    .plane
                    .create_comment(
                        &payload.workspace_slug,
                        project_id,
                        work_item_id,
                        &html,
                        event
                            .plane_comment_id
                            .map(|comment_id| format!("duplicate-run-{comment_id}"))
                            .as_deref(),
                    )
                    .await
                    .map_err(|plane_error| AppError::Internal(plane_error.to_string()))?;
                WebhookEventsRepo::mark_webhook_event_ignored(
                    &state.pool,
                    event_id,
                    Some("active run already exists"),
                )
                .await?;
                return Ok(());
            }
            return Err(AppError::Conflict("failed to claim work item".to_string()));
        }
        Err(error) => return Err(AppError::Internal(error.to_string())),
    };

    state
        .plane
        .create_comment(
            &payload.workspace_slug,
            project_id,
            work_item_id,
            &format!("<p>Agent accepted this task.</p><p>Run: {}</p>", run.id),
            Some(&format!("accepted-{}", run.id)),
        )
        .await
        .map_err(|error| AppError::Internal(error.to_string()))?;
    run_executor::add_label(&state, &run, &state.config.agent_running_label_name).await?;
    run_executor::move_to_state(&state, &run, &state.config.in_progress_state_name).await?;
    run_executor::execute_run(
        state.clone(),
        run.id,
        comment_text,
        event.plane_comment_id,
        project_control,
    )
    .await?;
    WebhookEventsRepo::mark_webhook_event_processed(&state.pool, event_id).await?;
    Ok(())
}

async fn handle_approval_command(
    state: &AppState,
    event: &crate::db::webhook_events::PlaneWebhookEvent,
    normalized_comment: &str,
) -> AppResult<bool> {
    let Some(work_item_id) = event.plane_work_item_id else {
        return Ok(false);
    };
    let Some(project_id) = event.plane_project_id else {
        return Ok(false);
    };

    let resolution = if normalized_comment == "@agent approve"
        || normalized_comment.starts_with("@agent approve with note:")
    {
        Some("approved")
    } else if normalized_comment == "@agent reject" {
        Some("rejected")
    } else {
        None
    };
    let Some(resolution_status) = resolution else {
        return Ok(false);
    };

    let Some(approval) = RunsRepo::get_latest_requested_approval(&state.pool, work_item_id).await?
    else {
        return Ok(false);
    };
    RunsRepo::resolve_approval(
        &state.pool,
        approval.id,
        &ResolveAgentApproval {
            status: resolution_status.to_string(),
            resolved_by: event.plane_actor_id,
        },
    )
    .await?;

    let run = RunsRepo::get_run(&state.pool, approval.run_id)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("run {} not found", approval.run_id)))?;

    if approval.approval_type == "scope_change_approval" && resolution_status == "approved" {
        RunsRepo::update_run(
            &state.pool,
            approval.run_id,
            &crate::db::runs::UpdateAgentRun {
                status: "queued".to_string(),
                error_message: Some("scope change approved".to_string()),
                ..crate::db::runs::UpdateAgentRun::default()
            },
        )
        .await?;
        state
            .plane
            .create_comment(
                &event.plane_workspace_slug,
                project_id,
                work_item_id,
                &format!(
                    "<p>Approval recorded. Scope change approved; resuming run {}.</p>",
                    approval.run_id
                ),
                Some(&format!("approval-approved-{}", approval.id)),
            )
            .await
            .map_err(|error| AppError::Internal(error.to_string()))?;
        let project_control = ProjectControlsRepo::get_project_control(&state.pool, project_id)
            .await?
            .ok_or_else(|| {
                AppError::NotFound(format!("project control missing for {project_id}"))
            })?;
        run_executor::execute_run(
            state.clone(),
            approval.run_id,
            approval
                .requested_payload
                .get("new_request")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            event.plane_comment_id,
            project_control,
        )
        .await?;
        return Ok(true);
    }

    if approval.approval_type == "scope_change_approval" && resolution_status == "rejected" {
        RunsRepo::update_run(
            &state.pool,
            approval.run_id,
            &crate::db::runs::UpdateAgentRun {
                status: "cancelled".to_string(),
                error_message: Some("scope change rejected by human".to_string()),
                ..crate::db::runs::UpdateAgentRun::default()
            },
        )
        .await?;
        state
            .plane
            .create_comment(
                &event.plane_workspace_slug,
                project_id,
                work_item_id,
                &format!(
                    "<p>Approval recorded. Run {} was cancelled by human decision.</p>",
                    run.id
                ),
                Some(&format!("approval-rejected-{}", approval.id)),
            )
            .await
            .map_err(|error| AppError::Internal(error.to_string()))?;
        return Ok(true);
    }

    Ok(false)
}

async fn comment_project_control_blocker(
    state: &AppState,
    workspace_slug: &str,
    project_id: Uuid,
    work_item_id: Uuid,
    comment_id: Option<Uuid>,
) -> AppResult<()> {
    state
        .plane
        .create_comment(
            workspace_slug,
            project_id,
            work_item_id,
            "<p>Agent needs approved project control before starting. Missing: approved TOR / Project Control for this project.</p>",
            comment_id
                .map(|value| format!("needs-project-control-{value}"))
                .as_deref(),
        )
        .await
        .map_err(|error| AppError::Internal(error.to_string()))?;
    Ok(())
}

fn has_acceptance_criteria(description_html: &Option<String>, approved_scope: &Value) -> bool {
    let description_has_phrase = description_html
        .as_deref()
        .map(strip_html_for_mentions)
        .map(|text| text.to_lowercase().contains("acceptance criteria"))
        .unwrap_or(false);
    let approved_scope_has_criteria = approved_scope
        .get("acceptance_criteria")
        .and_then(Value::as_array)
        .map(|items| !items.is_empty())
        .unwrap_or(false);
    description_has_phrase || approved_scope_has_criteria
}

#[cfg(test)]
mod tests {
    use super::has_acceptance_criteria;
    use serde_json::json;

    #[test]
    fn acceptance_criteria_detected_from_description_or_scope() {
        assert!(has_acceptance_criteria(
            &Some("<p>Acceptance criteria</p>".to_string()),
            &json!({})
        ));
        assert!(has_acceptance_criteria(
            &None,
            &json!({ "acceptance_criteria": ["first"] })
        ));
        assert!(!has_acceptance_criteria(
            &Some("<p>No checklist</p>".to_string()),
            &json!({})
        ));
    }
}
