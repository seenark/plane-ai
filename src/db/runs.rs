use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{Executor, FromRow, Postgres};
use uuid::Uuid;

use crate::db::DbResult;

const RUN_COLUMNS: &str = r#"
    id,
    plane_workspace_slug,
    plane_project_id,
    plane_work_item_id,
    trigger_comment_id,
    trigger_user_id,
    status,
    prompt,
    final_response,
    started_at,
    finished_at,
    exit_code,
    error_message,
    repo_url,
    branch_name,
    commit_sha,
    pr_url,
    runner_mode,
    runner_command,
    stdout,
    stderr,
    llm_cost::double precision AS llm_cost,
    agent_runtime_seconds,
    created_at
"#;

const COST_COLUMNS: &str = r#"
    id,
    run_id,
    planned_man_days::double precision AS planned_man_days,
    actual_agent_runtime_seconds,
    llm_cost::double precision AS llm_cost,
    internal_cost::double precision AS internal_cost,
    billable_amount::double precision AS billable_amount,
    gross_margin::double precision AS gross_margin,
    gross_margin_percent::double precision AS gross_margin_percent,
    created_at,
    updated_at
"#;

pub struct RunsRepo;

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct AgentRun {
    pub id: Uuid,
    pub plane_workspace_slug: String,
    pub plane_project_id: Uuid,
    pub plane_work_item_id: Uuid,
    pub trigger_comment_id: Option<Uuid>,
    pub trigger_user_id: Option<Uuid>,
    pub status: String,
    pub prompt: Option<String>,
    pub final_response: Option<String>,
    pub started_at: Option<DateTime<Utc>>,
    pub finished_at: Option<DateTime<Utc>>,
    pub exit_code: Option<i32>,
    pub error_message: Option<String>,
    pub repo_url: Option<String>,
    pub branch_name: Option<String>,
    pub commit_sha: Option<String>,
    pub pr_url: Option<String>,
    pub runner_mode: String,
    pub runner_command: Option<String>,
    pub stdout: Option<String>,
    pub stderr: Option<String>,
    pub llm_cost: f64,
    pub agent_runtime_seconds: i32,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct NewAgentRun {
    pub plane_workspace_slug: String,
    pub plane_project_id: Uuid,
    pub plane_work_item_id: Uuid,
    pub trigger_comment_id: Option<Uuid>,
    pub trigger_user_id: Option<Uuid>,
    pub status: String,
    pub runner_mode: String,
    pub runner_command: Option<String>,
    pub prompt: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct UpdateAgentRun {
    pub status: String,
    pub prompt: Option<String>,
    pub final_response: Option<String>,
    pub started_at: Option<DateTime<Utc>>,
    pub finished_at: Option<DateTime<Utc>>,
    pub exit_code: Option<i32>,
    pub error_message: Option<String>,
    pub repo_url: Option<String>,
    pub branch_name: Option<String>,
    pub commit_sha: Option<String>,
    pub pr_url: Option<String>,
    pub runner_command: Option<String>,
    pub stdout: Option<String>,
    pub stderr: Option<String>,
    pub llm_cost: Option<f64>,
    pub agent_runtime_seconds: Option<i32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct AgentRunEvent {
    pub id: Uuid,
    pub run_id: Uuid,
    pub event_type: String,
    pub payload: serde_json::Value,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct NewAgentRunEvent {
    pub run_id: Uuid,
    pub event_type: String,
    pub payload: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct AgentRunArtifact {
    pub id: Uuid,
    pub run_id: Uuid,
    pub artifact_type: String,
    pub name: String,
    pub content: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct NewAgentRunArtifact {
    pub run_id: Uuid,
    pub artifact_type: String,
    pub name: String,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct AgentApproval {
    pub id: Uuid,
    pub run_id: Uuid,
    pub approval_type: String,
    pub requested_payload: serde_json::Value,
    pub plane_work_item_id: Uuid,
    pub plane_comment_id: Option<Uuid>,
    pub status: String,
    pub requested_at: DateTime<Utc>,
    pub resolved_at: Option<DateTime<Utc>>,
    pub resolved_by: Option<Uuid>,
}

#[derive(Debug, Clone)]
pub struct NewAgentApproval {
    pub run_id: Uuid,
    pub approval_type: String,
    pub requested_payload: serde_json::Value,
    pub plane_work_item_id: Uuid,
    pub plane_comment_id: Option<Uuid>,
    pub status: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ResolveAgentApproval {
    pub status: String,
    pub resolved_by: Option<Uuid>,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct AgentCost {
    pub id: Uuid,
    pub run_id: Uuid,
    pub planned_man_days: Option<f64>,
    pub actual_agent_runtime_seconds: i32,
    pub llm_cost: f64,
    pub internal_cost: f64,
    pub billable_amount: f64,
    pub gross_margin: f64,
    pub gross_margin_percent: f64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct UpsertAgentCost {
    pub run_id: Uuid,
    pub planned_man_days: Option<f64>,
    pub actual_agent_runtime_seconds: i32,
    pub llm_cost: f64,
    pub internal_cost: f64,
    pub billable_amount: f64,
    pub gross_margin: f64,
    pub gross_margin_percent: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct ScopeChangeLog {
    pub id: Uuid,
    pub run_id: Uuid,
    pub classification: Option<String>,
    pub original_scope: Option<String>,
    pub new_request: Option<String>,
    pub estimated_impact: Option<String>,
    pub decision: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct NewScopeChangeLog {
    pub run_id: Uuid,
    pub classification: Option<String>,
    pub original_scope: Option<String>,
    pub new_request: Option<String>,
    pub estimated_impact: Option<String>,
    pub decision: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentRunDetail {
    pub run: AgentRun,
    pub events: Vec<AgentRunEvent>,
    pub artifacts: Vec<AgentRunArtifact>,
    pub approvals: Vec<AgentApproval>,
    pub cost: Option<AgentCost>,
}

impl RunsRepo {
    pub async fn create_run<'e, E>(executor: E, new_run: &NewAgentRun) -> DbResult<AgentRun>
    where
        E: Executor<'e, Database = Postgres>,
    {
        let sql = format!(
            r#"
            INSERT INTO agent_runs (
                plane_workspace_slug,
                plane_project_id,
                plane_work_item_id,
                trigger_comment_id,
                trigger_user_id,
                status,
                prompt,
                runner_mode,
                runner_command
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
            RETURNING {RUN_COLUMNS}
            "#,
        );

        sqlx::query_as::<_, AgentRun>(&sql)
            .bind(&new_run.plane_workspace_slug)
            .bind(new_run.plane_project_id)
            .bind(new_run.plane_work_item_id)
            .bind(new_run.trigger_comment_id)
            .bind(new_run.trigger_user_id)
            .bind(&new_run.status)
            .bind(new_run.prompt.as_deref())
            .bind(&new_run.runner_mode)
            .bind(new_run.runner_command.as_deref())
            .fetch_one(executor)
            .await
    }

    pub async fn get_run<'e, E>(executor: E, run_id: Uuid) -> DbResult<Option<AgentRun>>
    where
        E: Executor<'e, Database = Postgres>,
    {
        let sql = format!("SELECT {RUN_COLUMNS} FROM agent_runs WHERE id = $1");

        sqlx::query_as::<_, AgentRun>(&sql)
            .bind(run_id)
            .fetch_optional(executor)
            .await
    }

    pub async fn get_active_run<'e, E>(
        executor: E,
        plane_work_item_id: Uuid,
    ) -> DbResult<Option<AgentRun>>
    where
        E: Executor<'e, Database = Postgres>,
    {
        let sql = format!(
            r#"
            SELECT {RUN_COLUMNS}
            FROM agent_runs
            WHERE plane_work_item_id = $1
              AND status IN ('queued', 'running', 'waiting_for_user')
            ORDER BY created_at DESC
            LIMIT 1
            "#,
        );

        sqlx::query_as::<_, AgentRun>(&sql)
            .bind(plane_work_item_id)
            .fetch_optional(executor)
            .await
    }

    pub async fn update_run<'e, E>(
        executor: E,
        run_id: Uuid,
        update: &UpdateAgentRun,
    ) -> DbResult<AgentRun>
    where
        E: Executor<'e, Database = Postgres>,
    {
        let sql = format!(
            r#"
            UPDATE agent_runs
            SET status = $2,
                prompt = COALESCE($3, prompt),
                final_response = COALESCE($4, final_response),
                started_at = COALESCE($5, started_at),
                finished_at = COALESCE($6, finished_at),
                exit_code = COALESCE($7, exit_code),
                error_message = COALESCE($8, error_message),
                repo_url = COALESCE($9, repo_url),
                branch_name = COALESCE($10, branch_name),
                commit_sha = COALESCE($11, commit_sha),
                pr_url = COALESCE($12, pr_url),
                runner_command = COALESCE($13, runner_command),
                stdout = COALESCE($14, stdout),
                stderr = COALESCE($15, stderr),
                llm_cost = COALESCE($16, llm_cost),
                agent_runtime_seconds = COALESCE($17, agent_runtime_seconds)
            WHERE id = $1
            RETURNING {RUN_COLUMNS}
            "#,
        );

        sqlx::query_as::<_, AgentRun>(&sql)
            .bind(run_id)
            .bind(&update.status)
            .bind(update.prompt.as_deref())
            .bind(update.final_response.as_deref())
            .bind(update.started_at)
            .bind(update.finished_at)
            .bind(update.exit_code)
            .bind(update.error_message.as_deref())
            .bind(update.repo_url.as_deref())
            .bind(update.branch_name.as_deref())
            .bind(update.commit_sha.as_deref())
            .bind(update.pr_url.as_deref())
            .bind(update.runner_command.as_deref())
            .bind(update.stdout.as_deref())
            .bind(update.stderr.as_deref())
            .bind(update.llm_cost)
            .bind(update.agent_runtime_seconds)
            .fetch_one(executor)
            .await
    }

    pub async fn list_runs<'e, E>(executor: E, limit: i64) -> DbResult<Vec<AgentRun>>
    where
        E: Executor<'e, Database = Postgres>,
    {
        let sql = format!(
            r#"
            SELECT {RUN_COLUMNS}
            FROM agent_runs
            ORDER BY created_at DESC
            LIMIT $1
            "#,
        );

        sqlx::query_as::<_, AgentRun>(&sql)
            .bind(limit)
            .fetch_all(executor)
            .await
    }

    pub async fn insert_run_event<'e, E>(
        executor: E,
        new_event: &NewAgentRunEvent,
    ) -> DbResult<AgentRunEvent>
    where
        E: Executor<'e, Database = Postgres>,
    {
        sqlx::query_as::<_, AgentRunEvent>(
            r#"
            INSERT INTO agent_run_events (run_id, event_type, payload)
            VALUES ($1, $2, $3)
            RETURNING id, run_id, event_type, payload, created_at
            "#,
        )
        .bind(new_event.run_id)
        .bind(&new_event.event_type)
        .bind(&new_event.payload)
        .fetch_one(executor)
        .await
    }

    pub async fn list_run_events<'e, E>(executor: E, run_id: Uuid) -> DbResult<Vec<AgentRunEvent>>
    where
        E: Executor<'e, Database = Postgres>,
    {
        sqlx::query_as::<_, AgentRunEvent>(
            r#"
            SELECT id, run_id, event_type, payload, created_at
            FROM agent_run_events
            WHERE run_id = $1
            ORDER BY created_at ASC
            "#,
        )
        .bind(run_id)
        .fetch_all(executor)
        .await
    }

    pub async fn insert_run_artifact<'e, E>(
        executor: E,
        new_artifact: &NewAgentRunArtifact,
    ) -> DbResult<AgentRunArtifact>
    where
        E: Executor<'e, Database = Postgres>,
    {
        sqlx::query_as::<_, AgentRunArtifact>(
            r#"
            INSERT INTO agent_run_artifacts (run_id, artifact_type, name, content)
            VALUES ($1, $2, $3, $4)
            RETURNING id, run_id, artifact_type, name, content, created_at
            "#,
        )
        .bind(new_artifact.run_id)
        .bind(&new_artifact.artifact_type)
        .bind(&new_artifact.name)
        .bind(&new_artifact.content)
        .fetch_one(executor)
        .await
    }

    pub async fn list_run_artifacts<'e, E>(
        executor: E,
        run_id: Uuid,
    ) -> DbResult<Vec<AgentRunArtifact>>
    where
        E: Executor<'e, Database = Postgres>,
    {
        sqlx::query_as::<_, AgentRunArtifact>(
            r#"
            SELECT id, run_id, artifact_type, name, content, created_at
            FROM agent_run_artifacts
            WHERE run_id = $1
            ORDER BY created_at ASC
            "#,
        )
        .bind(run_id)
        .fetch_all(executor)
        .await
    }

    pub async fn create_approval<'e, E>(
        executor: E,
        new_approval: &NewAgentApproval,
    ) -> DbResult<AgentApproval>
    where
        E: Executor<'e, Database = Postgres>,
    {
        sqlx::query_as::<_, AgentApproval>(
            r#"
            INSERT INTO agent_approvals (
                run_id,
                approval_type,
                requested_payload,
                plane_work_item_id,
                plane_comment_id,
                status
            )
            VALUES ($1, $2, $3, $4, $5, COALESCE($6, 'requested'))
            RETURNING id, run_id, approval_type, requested_payload, plane_work_item_id, plane_comment_id, status, requested_at, resolved_at, resolved_by
            "#,
        )
        .bind(new_approval.run_id)
        .bind(&new_approval.approval_type)
        .bind(&new_approval.requested_payload)
        .bind(new_approval.plane_work_item_id)
        .bind(new_approval.plane_comment_id)
        .bind(new_approval.status.as_deref())
        .fetch_one(executor)
        .await
    }

    pub async fn get_latest_requested_approval<'e, E>(
        executor: E,
        plane_work_item_id: Uuid,
    ) -> DbResult<Option<AgentApproval>>
    where
        E: Executor<'e, Database = Postgres>,
    {
        sqlx::query_as::<_, AgentApproval>(
            r#"
            SELECT id, run_id, approval_type, requested_payload, plane_work_item_id, plane_comment_id, status, requested_at, resolved_at, resolved_by
            FROM agent_approvals
            WHERE plane_work_item_id = $1
              AND status = 'requested'
            ORDER BY requested_at DESC
            LIMIT 1
            "#,
        )
        .bind(plane_work_item_id)
        .fetch_optional(executor)
        .await
    }

    pub async fn resolve_approval<'e, E>(
        executor: E,
        approval_id: Uuid,
        resolution: &ResolveAgentApproval,
    ) -> DbResult<AgentApproval>
    where
        E: Executor<'e, Database = Postgres>,
    {
        sqlx::query_as::<_, AgentApproval>(
            r#"
            UPDATE agent_approvals
            SET status = $2,
                resolved_at = now(),
                resolved_by = $3
            WHERE id = $1
            RETURNING id, run_id, approval_type, requested_payload, plane_work_item_id, plane_comment_id, status, requested_at, resolved_at, resolved_by
            "#,
        )
        .bind(approval_id)
        .bind(&resolution.status)
        .bind(resolution.resolved_by)
        .fetch_one(executor)
        .await
    }

    pub async fn list_run_approvals<'e, E>(
        executor: E,
        run_id: Uuid,
    ) -> DbResult<Vec<AgentApproval>>
    where
        E: Executor<'e, Database = Postgres>,
    {
        sqlx::query_as::<_, AgentApproval>(
            r#"
            SELECT id, run_id, approval_type, requested_payload, plane_work_item_id, plane_comment_id, status, requested_at, resolved_at, resolved_by
            FROM agent_approvals
            WHERE run_id = $1
            ORDER BY requested_at ASC
            "#,
        )
        .bind(run_id)
        .fetch_all(executor)
        .await
    }

    pub async fn insert_scope_change_log<'e, E>(
        executor: E,
        new_log: &NewScopeChangeLog,
    ) -> DbResult<ScopeChangeLog>
    where
        E: Executor<'e, Database = Postgres>,
    {
        sqlx::query_as::<_, ScopeChangeLog>(
            r#"
            INSERT INTO scope_change_logs (
                run_id,
                classification,
                original_scope,
                new_request,
                estimated_impact,
                decision
            )
            VALUES ($1, $2, $3, $4, $5, $6)
            RETURNING id, run_id, classification, original_scope, new_request, estimated_impact, decision, created_at, updated_at
            "#,
        )
        .bind(new_log.run_id)
        .bind(new_log.classification.as_deref())
        .bind(new_log.original_scope.as_deref())
        .bind(new_log.new_request.as_deref())
        .bind(new_log.estimated_impact.as_deref())
        .bind(new_log.decision.as_deref())
        .fetch_one(executor)
        .await
    }

    pub async fn list_scope_change_logs<'e, E>(
        executor: E,
        run_id: Uuid,
    ) -> DbResult<Vec<ScopeChangeLog>>
    where
        E: Executor<'e, Database = Postgres>,
    {
        sqlx::query_as::<_, ScopeChangeLog>(
            r#"
            SELECT id, run_id, classification, original_scope, new_request, estimated_impact, decision, created_at, updated_at
            FROM scope_change_logs
            WHERE run_id = $1
            ORDER BY created_at ASC
            "#,
        )
        .bind(run_id)
        .fetch_all(executor)
        .await
    }

    pub async fn upsert_cost<'e, E>(executor: E, new_cost: &UpsertAgentCost) -> DbResult<AgentCost>
    where
        E: Executor<'e, Database = Postgres>,
    {
        let sql = format!(
            r#"
            INSERT INTO agent_costs (
                run_id,
                planned_man_days,
                actual_agent_runtime_seconds,
                llm_cost,
                internal_cost,
                billable_amount,
                gross_margin,
                gross_margin_percent,
                updated_at
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, now())
            ON CONFLICT (run_id) DO UPDATE
            SET planned_man_days = EXCLUDED.planned_man_days,
                actual_agent_runtime_seconds = EXCLUDED.actual_agent_runtime_seconds,
                llm_cost = EXCLUDED.llm_cost,
                internal_cost = EXCLUDED.internal_cost,
                billable_amount = EXCLUDED.billable_amount,
                gross_margin = EXCLUDED.gross_margin,
                gross_margin_percent = EXCLUDED.gross_margin_percent,
                updated_at = now()
            RETURNING {COST_COLUMNS}
            "#,
        );

        sqlx::query_as::<_, AgentCost>(&sql)
            .bind(new_cost.run_id)
            .bind(new_cost.planned_man_days)
            .bind(new_cost.actual_agent_runtime_seconds)
            .bind(new_cost.llm_cost)
            .bind(new_cost.internal_cost)
            .bind(new_cost.billable_amount)
            .bind(new_cost.gross_margin)
            .bind(new_cost.gross_margin_percent)
            .fetch_one(executor)
            .await
    }

    pub async fn get_run_cost<'e, E>(executor: E, run_id: Uuid) -> DbResult<Option<AgentCost>>
    where
        E: Executor<'e, Database = Postgres>,
    {
        let sql = format!("SELECT {COST_COLUMNS} FROM agent_costs WHERE run_id = $1");

        sqlx::query_as::<_, AgentCost>(&sql)
            .bind(run_id)
            .fetch_optional(executor)
            .await
    }

    pub async fn get_run_detail<'e, E>(
        executor: E,
        run_id: Uuid,
    ) -> DbResult<Option<AgentRunDetail>>
    where
        E: Executor<'e, Database = Postgres> + Copy,
    {
        let Some(run) = Self::get_run(executor, run_id).await? else {
            return Ok(None);
        };

        let events = Self::list_run_events(executor, run_id).await?;
        let artifacts = Self::list_run_artifacts(executor, run_id).await?;
        let approvals = Self::list_run_approvals(executor, run_id).await?;
        let cost = Self::get_run_cost(executor, run_id).await?;

        Ok(Some(AgentRunDetail {
            run,
            events,
            artifacts,
            approvals,
            cost,
        }))
    }
}
