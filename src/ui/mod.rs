use axum::{
    extract::{Path, State},
    response::Html,
    routing::get,
    Router,
};
use uuid::Uuid;

use crate::{
    db::runs::RunsRepo,
    error::{AppError, AppResult},
    http::AppState,
};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/ui/runs", get(list_runs_page))
        .route("/ui/runs/{run_id}", get(run_detail_page))
}

async fn list_runs_page(State(state): State<AppState>) -> AppResult<Html<String>> {
    let runs = RunsRepo::list_runs(&state.pool, 100).await?;
    let mut rows = String::new();
    for run in runs {
        rows.push_str(&format!(
            "<tr><td><a href=\"/ui/runs/{id}\">{id}</a></td><td>{work_item}</td><td>{status}</td><td>{started}</td><td>{finished}</td><td>{exit_code}</td><td>{runtime}</td><td>{pr_url}</td><td>{cost}</td></tr>",
            id = run.id,
            work_item = run.plane_work_item_id,
            status = run.status,
            started = optional_text(run.started_at.map(|value| value.to_rfc3339())),
            finished = optional_text(run.finished_at.map(|value| value.to_rfc3339())),
            exit_code = optional_text(run.exit_code.map(|value| value.to_string())),
            runtime = run.agent_runtime_seconds,
            pr_url = optional_text(run.pr_url),
            cost = format!("{:.4}", run.llm_cost),
        ));
    }

    Ok(Html(format!(
        "<!doctype html><html><head><title>Plane AI Runs</title></head><body><h1>Runs</h1><table border=\"1\" cellspacing=\"0\" cellpadding=\"6\"><thead><tr><th>Run ID</th><th>Work Item ID</th><th>Status</th><th>Started</th><th>Finished</th><th>Exit Code</th><th>Runtime Seconds</th><th>PR URL</th><th>Cost</th></tr></thead><tbody>{rows}</tbody></table></body></html>"
    )))
}

async fn run_detail_page(
    State(state): State<AppState>,
    Path(run_id): Path<Uuid>,
) -> AppResult<Html<String>> {
    let detail = RunsRepo::get_run_detail(&state.pool, run_id)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("run {run_id} not found")))?;

    let event_rows = detail
        .events
        .iter()
        .map(|event| {
            format!(
                "<li><strong>{}</strong><pre>{}</pre></li>",
                event.event_type, event.payload
            )
        })
        .collect::<Vec<_>>()
        .join("");
    let artifact_rows = detail
        .artifacts
        .iter()
        .map(|artifact| {
            format!(
                "<li><strong>{}:{}</strong><pre>{}</pre></li>",
                artifact.artifact_type, artifact.name, artifact.content
            )
        })
        .collect::<Vec<_>>()
        .join("");
    let approval_rows = detail
        .approvals
        .iter()
        .map(|approval| {
            format!(
                "<li><strong>{}</strong> — {}<pre>{}</pre></li>",
                approval.approval_type, approval.status, approval.requested_payload
            )
        })
        .collect::<Vec<_>>()
        .join("");
    let cost_html = match detail.cost {
        Some(cost) => format!(
            "<ul><li>planned_man_days: {}</li><li>actual_agent_runtime_seconds: {}</li><li>llm_cost: {:.4}</li><li>internal_cost: {:.4}</li><li>billable_amount: {:.4}</li><li>gross_margin: {:.4}</li><li>gross_margin_percent: {:.4}</li></ul>",
            optional_text(cost.planned_man_days.map(|value| value.to_string())),
            cost.actual_agent_runtime_seconds,
            cost.llm_cost,
            cost.internal_cost,
            cost.billable_amount,
            cost.gross_margin,
            cost.gross_margin_percent,
        ),
        None => "<p>none</p>".to_string(),
    };

    Ok(Html(format!(
        "<!doctype html><html><head><title>Run {id}</title></head><body><h1>Run {id}</h1><p>Status: {status}</p><p>Work item: {work_item}</p><h2>Prompt</h2><pre>{prompt}</pre><h2>Final response</h2><pre>{response}</pre><h2>Stdout</h2><pre>{stdout}</pre><h2>Stderr</h2><pre>{stderr}</pre><h2>Events</h2><ul>{event_rows}</ul><h2>Artifacts</h2><ul>{artifact_rows}</ul><h2>Approvals</h2><ul>{approval_rows}</ul><h2>Cost</h2>{cost_html}</body></html>",
        id = detail.run.id,
        status = detail.run.status,
        work_item = detail.run.plane_work_item_id,
        prompt = detail.run.prompt.unwrap_or_default(),
        response = detail.run.final_response.unwrap_or_default(),
        stdout = detail.run.stdout.unwrap_or_default(),
        stderr = detail.run.stderr.unwrap_or_default(),
    )))
}

fn optional_text(value: Option<String>) -> String {
    value.unwrap_or_else(|| "not provided".to_string())
}
