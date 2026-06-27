use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{Executor, FromRow, Postgres};
use uuid::Uuid;

use crate::db::DbResult;

const PROJECT_CONTROL_COLUMNS: &str = r#"
    id,
    plane_workspace_slug,
    plane_project_id,
    source,
    tor_markdown,
    approved_scope,
    budget_man_days::double precision AS budget_man_days,
    billing_rate_per_day::double precision AS billing_rate_per_day,
    internal_cost_rate_per_day::double precision AS internal_cost_rate_per_day,
    human_reviewer_id,
    brief_status,
    created_at,
    updated_at
"#;

pub struct ProjectControlsRepo;

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct ProjectControl {
    pub id: Uuid,
    pub plane_workspace_slug: String,
    pub plane_project_id: Uuid,
    pub source: String,
    pub tor_markdown: String,
    pub approved_scope: serde_json::Value,
    pub budget_man_days: Option<f64>,
    pub billing_rate_per_day: Option<f64>,
    pub internal_cost_rate_per_day: Option<f64>,
    pub human_reviewer_id: Option<Uuid>,
    pub brief_status: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct UpsertProjectControl {
    pub plane_workspace_slug: String,
    pub plane_project_id: Uuid,
    pub source: String,
    pub tor_markdown: String,
    pub approved_scope: serde_json::Value,
    pub budget_man_days: Option<f64>,
    pub billing_rate_per_day: Option<f64>,
    pub internal_cost_rate_per_day: Option<f64>,
    pub human_reviewer_id: Option<Uuid>,
    pub brief_status: String,
}

impl ProjectControlsRepo {
    pub async fn upsert_project_control<'e, E>(
        executor: E,
        control: &UpsertProjectControl,
    ) -> DbResult<ProjectControl>
    where
        E: Executor<'e, Database = Postgres>,
    {
        let sql = format!(
            r#"
            INSERT INTO project_controls (
                plane_workspace_slug,
                plane_project_id,
                source,
                tor_markdown,
                approved_scope,
                budget_man_days,
                billing_rate_per_day,
                internal_cost_rate_per_day,
                human_reviewer_id,
                brief_status,
                updated_at
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, now())
            ON CONFLICT (plane_project_id) DO UPDATE
            SET plane_workspace_slug = EXCLUDED.plane_workspace_slug,
                source = EXCLUDED.source,
                tor_markdown = EXCLUDED.tor_markdown,
                approved_scope = EXCLUDED.approved_scope,
                budget_man_days = EXCLUDED.budget_man_days,
                billing_rate_per_day = EXCLUDED.billing_rate_per_day,
                internal_cost_rate_per_day = EXCLUDED.internal_cost_rate_per_day,
                human_reviewer_id = EXCLUDED.human_reviewer_id,
                brief_status = EXCLUDED.brief_status,
                updated_at = now()
            RETURNING {PROJECT_CONTROL_COLUMNS}
            "#,
        );

        sqlx::query_as::<_, ProjectControl>(&sql)
            .bind(&control.plane_workspace_slug)
            .bind(control.plane_project_id)
            .bind(&control.source)
            .bind(&control.tor_markdown)
            .bind(&control.approved_scope)
            .bind(control.budget_man_days)
            .bind(control.billing_rate_per_day)
            .bind(control.internal_cost_rate_per_day)
            .bind(control.human_reviewer_id)
            .bind(&control.brief_status)
            .fetch_one(executor)
            .await
    }

    pub async fn get_project_control<'e, E>(
        executor: E,
        plane_project_id: Uuid,
    ) -> DbResult<Option<ProjectControl>>
    where
        E: Executor<'e, Database = Postgres>,
    {
        let sql = format!(
            "SELECT {PROJECT_CONTROL_COLUMNS} FROM project_controls WHERE plane_project_id = $1"
        );

        sqlx::query_as::<_, ProjectControl>(&sql)
            .bind(plane_project_id)
            .fetch_optional(executor)
            .await
    }
}
