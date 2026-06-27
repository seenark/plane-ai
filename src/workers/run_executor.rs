use chrono::Utc;
use serde_json::json;
use uuid::Uuid;

use crate::{
    agent::{
        prompts::{build_prompt, PromptContext, PromptProjectControl, PromptWorkItem},
        runner::{RunRequest, RunnerExecutionStatus, RunnerOutput},
        scope::{classify_scope, ScopeClassification, ScopeContext},
        trigger::strip_html_for_mentions,
    },
    db::{
        project_controls::ProjectControl,
        runs::{
            AgentRun, NewAgentApproval, NewAgentRunArtifact, NewAgentRunEvent, NewScopeChangeLog,
            RunsRepo, ScopeChangeLog, UpdateAgentRun, UpsertAgentCost,
        },
    },
    error::{AppError, AppResult},
    http::AppState,
    plane::{Priority, WorkItemPatch},
};

pub async fn execute_run(
    state: AppState,
    run_id: Uuid,
    trigger_comment_text: String,
    trigger_comment_id: Option<Uuid>,
    project_control: ProjectControl,
) -> AppResult<()> {
    let run = RunsRepo::get_run(&state.pool, run_id)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("run {run_id} not found")))?;
    let work_item = state
        .plane
        .get_work_item(
            &run.plane_workspace_slug,
            run.plane_project_id,
            run.plane_work_item_id,
        )
        .await
        .map_err(|error| AppError::Internal(error.to_string()))?;
    let comments = state
        .plane
        .list_comments(
            &run.plane_workspace_slug,
            run.plane_project_id,
            run.plane_work_item_id,
        )
        .await
        .map_err(|error| AppError::Internal(error.to_string()))?;
    let activities = state
        .plane
        .list_activities(
            &run.plane_workspace_slug,
            run.plane_project_id,
            run.plane_work_item_id,
        )
        .await
        .map_err(|error| AppError::Internal(error.to_string()))?;

    let description_text =
        strip_html_for_mentions(work_item.description_html.as_deref().unwrap_or_default());
    let latest_comments = comments
        .iter()
        .filter_map(|comment| comment.comment_html.as_deref())
        .map(strip_html_for_mentions)
        .collect::<Vec<_>>();
    let activities_summary = activities
        .iter()
        .map(|activity| {
            format_activity(
                activity.field.as_deref(),
                activity.verb.as_deref(),
                activity.created_at.as_deref(),
            )
        })
        .collect::<Vec<_>>();

    let pre_scope = classify_scope(&ScopeContext {
        approved_scope: &project_control.approved_scope,
        work_item_description: &description_text,
        trigger_comment_text: &trigger_comment_text,
        changed_files: &[],
        budget_man_days: project_control.budget_man_days,
        internal_cost_per_agent_hour: state.config.internal_cost_per_agent_hour,
        cost_estimate: None,
    });

    if matches!(
        pre_scope.classification,
        ScopeClassification::MajorScopeChange | ScopeClassification::OutOfScope
    ) {
        handle_scope_change_request(
            &state,
            &run,
            &project_control,
            scope_classification_text(pre_scope.classification),
            &pre_scope.original_scope,
            &pre_scope.new_request,
            &pre_scope.estimated_impact,
            trigger_comment_id,
        )
        .await?;
        return Ok(());
    }

    let prompt = build_prompt(&PromptContext {
        work_item: PromptWorkItem {
            id: work_item.id,
            title: work_item.name.clone(),
            priority: work_item.priority.as_ref().map(priority_to_string),
            current_state_id: work_item.current_state_id(),
            description_text: description_text.clone(),
            trigger_comment_text: trigger_comment_text.clone(),
            latest_comments,
            activities_summary,
        },
        project_control: PromptProjectControl {
            tor_markdown: project_control.tor_markdown.clone(),
            approved_scope: project_control.approved_scope.clone(),
            budget_man_days: project_control.budget_man_days,
            billing_rate_per_day: project_control.billing_rate_per_day,
            internal_cost_rate_per_day: project_control.internal_cost_rate_per_day,
            human_reviewer_id: project_control.human_reviewer_id,
        },
    });

    RunsRepo::update_run(
        &state.pool,
        run.id,
        &UpdateAgentRun {
            status: "running".to_string(),
            prompt: Some(prompt.clone()),
            started_at: Some(Utc::now()),
            ..UpdateAgentRun::default()
        },
    )
    .await?;
    RunsRepo::insert_run_event(
        &state.pool,
        &NewAgentRunEvent {
            run_id: run.id,
            event_type: "run_started".to_string(),
            payload: json!({ "prompt_length": prompt.len() }),
        },
    )
    .await?;
    RunsRepo::insert_run_artifact(
        &state.pool,
        &NewAgentRunArtifact {
            run_id: run.id,
            artifact_type: "prompt".to_string(),
            name: "runner-prompt.txt".to_string(),
            content: prompt.clone(),
        },
    )
    .await?;

    let result = state
        .runner
        .execute(&RunRequest {
            run_id: run.id,
            prompt: prompt.clone(),
        })
        .await;

    RunsRepo::insert_run_artifact(
        &state.pool,
        &NewAgentRunArtifact {
            run_id: run.id,
            artifact_type: "stdout".to_string(),
            name: "stdout.log".to_string(),
            content: result.stdout.clone(),
        },
    )
    .await?;
    RunsRepo::insert_run_artifact(
        &state.pool,
        &NewAgentRunArtifact {
            run_id: run.id,
            artifact_type: "stderr".to_string(),
            name: "stderr.log".to_string(),
            content: result.stderr.clone(),
        },
    )
    .await?;

    let (changed_files, cost_estimate, final_response) = match &result.output {
        Some(RunnerOutput::Succeeded {
            changed_files,
            cost_estimate,
            ..
        }) => (
            changed_files.clone(),
            *cost_estimate,
            Some(serde_json::to_string(&result.output).unwrap_or_default()),
        ),
        Some(RunnerOutput::Blocked { .. }) | Some(RunnerOutput::Failed { .. }) => (
            vec![],
            None,
            Some(serde_json::to_string(&result.output).unwrap_or_default()),
        ),
        None => (vec![], None, None),
    };

    let post_scope = classify_scope(&ScopeContext {
        approved_scope: &project_control.approved_scope,
        work_item_description: &description_text,
        trigger_comment_text: &trigger_comment_text,
        changed_files: &changed_files,
        budget_man_days: project_control.budget_man_days,
        internal_cost_per_agent_hour: state.config.internal_cost_per_agent_hour,
        cost_estimate,
    });
    let _scope_log = RunsRepo::insert_scope_change_log(
        &state.pool,
        &NewScopeChangeLog {
            run_id: run.id,
            classification: Some(scope_classification_text(post_scope.classification).to_string()),
            original_scope: Some(post_scope.original_scope.clone()),
            new_request: Some(post_scope.new_request.clone()),
            estimated_impact: Some(post_scope.estimated_impact.clone()),
            decision: Some("recorded".to_string()),
        },
    )
    .await?;

    let serialized_output = final_response.clone();
    if let Some(content) = final_response.as_ref() {
        RunsRepo::insert_run_artifact(
            &state.pool,
            &NewAgentRunArtifact {
                run_id: run.id,
                artifact_type: "final_response".to_string(),
                name: "final-response.json".to_string(),
                content: content.clone(),
            },
        )
        .await?;
    }
    RunsRepo::update_run(
        &state.pool,
        run.id,
        &UpdateAgentRun {
            status: status_text(result.status).to_string(),
            prompt: Some(prompt),
            final_response: serialized_output,
            started_at: Some(result.started_at),
            finished_at: Some(result.finished_at),
            exit_code: result.exit_code,
            error_message: result.error_message.clone(),
            commit_sha: output_commit_sha(&result.output),
            pr_url: output_pr_url(&result.output),
            runner_command: Some(state.config.agent_command.clone()),
            stdout: Some(result.stdout.clone()),
            stderr: Some(result.stderr.clone()),
            llm_cost: Some(cost_estimate.unwrap_or(0.0)),
            agent_runtime_seconds: Some(result.runtime_seconds as i32),
            ..UpdateAgentRun::default()
        },
    )
    .await?;
    RunsRepo::insert_run_event(
        &state.pool,
        &NewAgentRunEvent {
            run_id: run.id,
            event_type: "run_finished".to_string(),
            payload: json!({
                "status": status_text(result.status),
                "exit_code": result.exit_code,
                "runtime_seconds": result.runtime_seconds,
            }),
        },
    )
    .await?;

    match &result.output {
        Some(RunnerOutput::Succeeded {
            summary,
            changed_files,
            verification,
            artifacts,
            ..
        }) => {
            remove_label(&state, &run, &state.config.agent_running_label_name).await?;
            add_label(&state, &run, &state.config.agent_review_ready_label_name).await?;
            move_to_state(&state, &run, &state.config.human_review_state_name).await?;
            let html = success_comment_html(
                run.id,
                summary,
                changed_files,
                verification,
                artifacts.pr_url.as_deref(),
                artifacts.commit_sha.as_deref(),
            );
            state
                .plane
                .create_comment(
                    &run.plane_workspace_slug,
                    run.plane_project_id,
                    run.plane_work_item_id,
                    &html,
                    Some(&format!("completed-{}", run.id)),
                )
                .await
                .map_err(|error| AppError::Internal(error.to_string()))?;
        }
        Some(RunnerOutput::Blocked {
            question,
            options,
            recommended_option,
            impact,
        }) => {
            add_label(&state, &run, &state.config.agent_blocked_label_name).await?;
            move_to_state(&state, &run, &state.config.blocked_state_name).await?;
            let html = format!(
                "<p>Agent needs input.</p><p>{question}</p><p>Options: {options}</p><p>Recommended: {recommended_option}</p><p>Impact: {impact}</p>",
                options = options.join("; "),
            );
            state
                .plane
                .create_comment(
                    &run.plane_workspace_slug,
                    run.plane_project_id,
                    run.plane_work_item_id,
                    &html,
                    Some(&format!("blocked-{}", run.id)),
                )
                .await
                .map_err(|error| AppError::Internal(error.to_string()))?;
        }
        Some(RunnerOutput::Failed { error, .. }) => {
            remove_label(&state, &run, &state.config.agent_running_label_name).await?;
            add_label(&state, &run, &state.config.agent_blocked_label_name).await?;
            move_to_state(&state, &run, &state.config.blocked_state_name).await?;
            let stderr_tail = tail_text(&result.stderr, 2000);
            let html = format!(
                "<p>Agent failed.</p><p>Run: {run_id}</p><p>Exit code: {exit_code}</p><p>Error: {error}</p><pre>{stderr_tail}</pre>",
                run_id = run.id,
                exit_code = result.exit_code.map(|value| value.to_string()).unwrap_or_else(|| "not provided".to_string()),
            );
            state
                .plane
                .create_comment(
                    &run.plane_workspace_slug,
                    run.plane_project_id,
                    run.plane_work_item_id,
                    &html,
                    Some(&format!("failed-{}", run.id)),
                )
                .await
                .map_err(|error| AppError::Internal(error.to_string()))?;
        }
        None => {
            remove_label(&state, &run, &state.config.agent_running_label_name).await?;
            add_label(&state, &run, &state.config.agent_blocked_label_name).await?;
            move_to_state(&state, &run, &state.config.blocked_state_name).await?;
            let stderr_tail = tail_text(&result.stderr, 2000);
            let html = format!(
                "<p>Agent failed.</p><p>Run: {run_id}</p><p>Exit code: {exit_code}</p><p>Error: {error}</p><pre>{stderr_tail}</pre>",
                run_id = run.id,
                exit_code = result.exit_code.map(|value| value.to_string()).unwrap_or_else(|| "not provided".to_string()),
                error = result.error_message.as_deref().unwrap_or("failed without structured output"),
            );
            state
                .plane
                .create_comment(
                    &run.plane_workspace_slug,
                    run.plane_project_id,
                    run.plane_work_item_id,
                    &html,
                    Some(&format!("failed-{}", run.id)),
                )
                .await
                .map_err(|error| AppError::Internal(error.to_string()))?;
        }
    }

    upsert_cost(
        &state,
        &project_control,
        &run,
        result.runtime_seconds as i32,
        cost_estimate.unwrap_or(0.0),
    )
    .await?;
    Ok(())
}

async fn handle_scope_change_request(
    state: &AppState,
    run: &AgentRun,
    project_control: &ProjectControl,
    classification: &str,
    original_scope: &str,
    new_request: &str,
    estimated_impact: &str,
    trigger_comment_id: Option<Uuid>,
) -> AppResult<ScopeChangeLog> {
    let log = RunsRepo::insert_scope_change_log(
        &state.pool,
        &NewScopeChangeLog {
            run_id: run.id,
            classification: Some(classification.to_string()),
            original_scope: Some(original_scope.to_string()),
            new_request: Some(new_request.to_string()),
            estimated_impact: Some(estimated_impact.to_string()),
            decision: Some("approval_requested".to_string()),
        },
    )
    .await?;

    RunsRepo::update_run(
        &state.pool,
        run.id,
        &UpdateAgentRun {
            status: "waiting_for_user".to_string(),
            error_message: Some("scope change approval required".to_string()),
            ..UpdateAgentRun::default()
        },
    )
    .await?;
    RunsRepo::create_approval(
        &state.pool,
        &NewAgentApproval {
            run_id: run.id,
            approval_type: "scope_change_approval".to_string(),
            requested_payload: json!({
                "classification": classification,
                "original_scope": original_scope,
                "new_request": new_request,
                "estimated_impact": estimated_impact,
            }),
            plane_work_item_id: run.plane_work_item_id,
            plane_comment_id: trigger_comment_id,
            status: None,
        },
    )
    .await?;
    add_optional_named_label(state, run, "scope-creep-detected").await?;
    let html = format!(
        "<p>Scope creep detected.</p><p>Original approved scope:</p><ul><li>{}</li></ul><p>New requested scope:</p><ul><li>{}</li></ul><p>Estimated impact:</p><ul><li>{}</li></ul><p>Recommendation:</p><ul><li>Create a separate Scope Change card and approve budget impact before implementation.</li></ul>",
        original_scope,
        new_request,
        estimated_impact,
    );
    state
        .plane
        .create_comment(
            &run.plane_workspace_slug,
            run.plane_project_id,
            run.plane_work_item_id,
            &html,
            Some(&format!("scope-change-{}", run.id)),
        )
        .await
        .map_err(|error| AppError::Internal(error.to_string()))?;
    upsert_cost(state, project_control, run, 0, 0.0).await?;
    Ok(log)
}

pub async fn add_label(state: &AppState, run: &AgentRun, label_name: &str) -> AppResult<()> {
    set_label_presence(
        state,
        &run.plane_workspace_slug,
        run.plane_project_id,
        run.plane_work_item_id,
        Some(run.id),
        label_name,
        true,
    )
    .await
}

pub async fn add_label_to_work_item(
    state: &AppState,
    workspace_slug: &str,
    project_id: Uuid,
    work_item_id: Uuid,
    run_id: Option<Uuid>,
    label_name: &str,
) -> AppResult<()> {
    set_label_presence(
        state,
        workspace_slug,
        project_id,
        work_item_id,
        run_id,
        label_name,
        true,
    )
    .await
}

pub async fn remove_label(state: &AppState, run: &AgentRun, label_name: &str) -> AppResult<()> {
    set_label_presence(
        state,
        &run.plane_workspace_slug,
        run.plane_project_id,
        run.plane_work_item_id,
        Some(run.id),
        label_name,
        false,
    )
    .await
}

async fn add_optional_named_label(
    state: &AppState,
    run: &AgentRun,
    label_name: &str,
) -> AppResult<()> {
    match add_label(state, run, label_name).await {
        Ok(()) => Ok(()),
        Err(AppError::Internal(_)) => Ok(()),
        Err(error) => Err(error),
    }
}

async fn set_label_presence(
    state: &AppState,
    workspace_slug: &str,
    project_id: Uuid,
    work_item_id: Uuid,
    run_id: Option<Uuid>,
    label_name: &str,
    present: bool,
) -> AppResult<()> {
    let labels = state
        .plane
        .list_labels(workspace_slug, project_id)
        .await
        .map_err(|error| AppError::Internal(error.to_string()))?;
    let Some(label_id) = labels
        .iter()
        .find(|label| label.name == label_name)
        .map(|label| label.id)
    else {
        if let Some(run_id) = run_id {
            RunsRepo::insert_run_event(
                &state.pool,
                &NewAgentRunEvent {
                    run_id,
                    event_type: "label_missing".to_string(),
                    payload: json!({ "label": label_name }),
                },
            )
            .await?;
        }
        return Ok(());
    };

    let work_item = state
        .plane
        .get_work_item(workspace_slug, project_id, work_item_id)
        .await
        .map_err(|error| AppError::Internal(error.to_string()))?;
    let merged = merge_label_ids(&work_item.label_ids(), label_id, present);

    state
        .plane
        .patch_work_item(
            workspace_slug,
            project_id,
            work_item_id,
            WorkItemPatch {
                labels: Some(merged),
                ..WorkItemPatch::default()
            },
        )
        .await
        .map_err(|error| AppError::Internal(error.to_string()))?;
    Ok(())
}

fn merge_label_ids(existing: &[Uuid], label_id: Uuid, present: bool) -> Vec<Uuid> {
    let mut merged = existing.to_vec();
    if present {
        if !merged.contains(&label_id) {
            merged.push(label_id);
        }
    } else {
        merged.retain(|existing| *existing != label_id);
    }
    merged
}

pub async fn move_to_state(state: &AppState, run: &AgentRun, state_name: &str) -> AppResult<()> {
    let states = state
        .plane
        .list_states(&run.plane_workspace_slug, run.plane_project_id)
        .await
        .map_err(|error| AppError::Internal(error.to_string()))?;
    let Some(state_id) = states
        .iter()
        .find(|entry| entry.name == state_name)
        .map(|entry| entry.id)
    else {
        RunsRepo::insert_run_event(
            &state.pool,
            &NewAgentRunEvent {
                run_id: run.id,
                event_type: "state_missing".to_string(),
                payload: json!({ "state": state_name }),
            },
        )
        .await?;
        return Ok(());
    };

    state
        .plane
        .patch_work_item(
            &run.plane_workspace_slug,
            run.plane_project_id,
            run.plane_work_item_id,
            WorkItemPatch {
                state: Some(state_id),
                ..WorkItemPatch::default()
            },
        )
        .await
        .map_err(|error| AppError::Internal(error.to_string()))?;
    Ok(())
}

async fn upsert_cost(
    state: &AppState,
    project_control: &ProjectControl,
    run: &AgentRun,
    runtime_seconds: i32,
    llm_cost: f64,
) -> AppResult<()> {
    let internal_cost =
        (runtime_seconds as f64 / 3600.0) * state.config.internal_cost_per_agent_hour;
    let billable_amount = match (
        project_control.budget_man_days,
        project_control.billing_rate_per_day,
    ) {
        (Some(days), Some(rate)) => days * rate,
        _ => 0.0,
    };
    let gross_margin = billable_amount - internal_cost - llm_cost;
    let gross_margin_percent = if billable_amount > 0.0 {
        gross_margin / billable_amount * 100.0
    } else {
        0.0
    };
    RunsRepo::upsert_cost(
        &state.pool,
        &UpsertAgentCost {
            run_id: run.id,
            planned_man_days: project_control.budget_man_days,
            actual_agent_runtime_seconds: runtime_seconds,
            llm_cost,
            internal_cost,
            billable_amount,
            gross_margin,
            gross_margin_percent,
        },
    )
    .await?;
    Ok(())
}

fn success_comment_html(
    run_id: Uuid,
    summary: &str,
    changed_files: &[String],
    verification: &[String],
    pr_url: Option<&str>,
    commit_sha: Option<&str>,
) -> String {
    format!(
        "<p>Agent completed implementation.</p><p>Run: {run_id}</p><p>Summary:</p><ul>{summary}</ul><p>Changed files:</p><ul>{changed_files}</ul><p>Verification:</p><ul>{verification}</ul><p>Artifacts:</p><ul><li>PR: {pr_url}</li><li>Commit: {commit_sha}</li></ul><p>Needs human review before Done.</p>",
        summary = to_html_list(&split_summary(summary)),
        changed_files = to_html_list(changed_files),
        verification = to_html_list(verification),
        pr_url = pr_url.unwrap_or("not provided"),
        commit_sha = commit_sha.unwrap_or("not provided"),
    )
}

fn split_summary(summary: &str) -> Vec<String> {
    summary
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(|line| line.trim_start_matches("- ").to_string())
        .collect::<Vec<_>>()
        .into_iter()
        .collect::<Vec<_>>()
        .tap_if_empty(summary)
}

fn to_html_list(items: &[String]) -> String {
    if items.is_empty() {
        "<li>none</li>".to_string()
    } else {
        items
            .iter()
            .map(|item| format!("<li>{item}</li>"))
            .collect::<Vec<_>>()
            .join("")
    }
}

fn tail_text(text: &str, max_chars: usize) -> String {
    let chars = text.chars().collect::<Vec<_>>();
    if chars.len() <= max_chars {
        text.to_string()
    } else {
        chars[chars.len() - max_chars..].iter().collect()
    }
}

fn output_commit_sha(output: &Option<RunnerOutput>) -> Option<String> {
    match output {
        Some(RunnerOutput::Succeeded { artifacts, .. }) => artifacts.commit_sha.clone(),
        _ => None,
    }
}

fn output_pr_url(output: &Option<RunnerOutput>) -> Option<String> {
    match output {
        Some(RunnerOutput::Succeeded { artifacts, .. }) => artifacts.pr_url.clone(),
        _ => None,
    }
}

fn status_text(status: RunnerExecutionStatus) -> &'static str {
    match status {
        RunnerExecutionStatus::Succeeded => "succeeded",
        RunnerExecutionStatus::WaitingForUser => "waiting_for_user",
        RunnerExecutionStatus::Failed => "failed",
    }
}

fn scope_classification_text(classification: ScopeClassification) -> &'static str {
    match classification {
        ScopeClassification::None => "none",
        ScopeClassification::Clarification => "clarification",
        ScopeClassification::MinorScopeChange => "minor_scope_change",
        ScopeClassification::MajorScopeChange => "major_scope_change",
        ScopeClassification::OutOfScope => "out_of_scope",
        ScopeClassification::BudgetRisk => "budget_risk",
        ScopeClassification::TimelineRisk => "timeline_risk",
    }
}

fn priority_to_string(priority: &Priority) -> String {
    match priority {
        Priority::None => "none",
        Priority::Low => "low",
        Priority::Medium => "medium",
        Priority::High => "high",
        Priority::Urgent => "urgent",
    }
    .to_string()
}

fn format_activity(field: Option<&str>, verb: Option<&str>, created_at: Option<&str>) -> String {
    format!(
        "field={} verb={} created_at={}",
        field.unwrap_or("unknown"),
        verb.unwrap_or("unknown"),
        created_at.unwrap_or("unknown"),
    )
}

trait TapIfEmpty {
    fn tap_if_empty(self, fallback: &str) -> Vec<String>;
}

impl TapIfEmpty for Vec<String> {
    fn tap_if_empty(self, fallback: &str) -> Vec<String> {
        if self.is_empty() {
            vec![fallback.to_string()]
        } else {
            self
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        merge_label_ids, scope_classification_text, split_summary, success_comment_html, tail_text,
    };
    use crate::agent::scope::ScopeClassification;
    use uuid::Uuid;

    #[test]
    fn summary_comment_renders_lists() {
        let html = success_comment_html(
            Uuid::parse_str("11111111-1111-1111-1111-111111111111").unwrap(),
            "did one thing\ndid two things",
            &["src/main.rs".to_string()],
            &["cargo test".to_string()],
            Some("https://example.com/pr/1"),
            Some("abc123"),
        );
        assert!(html.contains("<li>did one thing</li>"));
        assert!(html.contains("<li>src/main.rs</li>"));
        assert!(html.contains("Needs human review before Done."));
    }

    #[test]
    fn tail_text_limits_output() {
        assert_eq!(tail_text("abcdef", 3), "def");
        assert_eq!(tail_text("abc", 10), "abc");
    }

    #[test]
    fn scope_classification_strings_match_plan_values() {
        assert_eq!(
            scope_classification_text(ScopeClassification::MajorScopeChange),
            "major_scope_change"
        );
        assert_eq!(
            scope_classification_text(ScopeClassification::OutOfScope),
            "out_of_scope"
        );
        assert_eq!(
            scope_classification_text(ScopeClassification::TimelineRisk),
            "timeline_risk"
        );
    }

    #[test]
    fn split_summary_falls_back_to_original_text() {
        assert_eq!(
            split_summary("single summary"),
            vec!["single summary".to_string()]
        );
    }

    #[test]
    fn merge_label_ids_preserves_existing_labels() {
        let existing = vec![
            Uuid::parse_str("11111111-1111-1111-1111-111111111111").unwrap(),
            Uuid::parse_str("22222222-2222-2222-2222-222222222222").unwrap(),
        ];
        let target = Uuid::parse_str("33333333-3333-3333-3333-333333333333").unwrap();

        let added = merge_label_ids(&existing, target, true);
        assert_eq!(added, vec![existing[0], existing[1], target]);

        let removed = merge_label_ids(&added, target, false);
        assert_eq!(removed, existing);
    }
}
