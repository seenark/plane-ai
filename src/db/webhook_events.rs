use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{Executor, FromRow, Postgres};
use uuid::Uuid;

use crate::db::DbResult;

const WEBHOOK_EVENT_COLUMNS: &str = r#"
    id,
    plane_delivery_id,
    plane_event,
    plane_action,
    plane_webhook_id,
    plane_workspace_id,
    plane_workspace_slug,
    plane_project_id,
    plane_work_item_id,
    plane_comment_id,
    plane_actor_id,
    payload,
    processing_status,
    received_at,
    processed_at,
    error_message
"#;

pub struct WebhookEventsRepo;

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct PlaneWebhookEvent {
    pub id: Uuid,
    pub plane_delivery_id: Option<String>,
    pub plane_event: String,
    pub plane_action: String,
    pub plane_webhook_id: Option<Uuid>,
    pub plane_workspace_id: Uuid,
    pub plane_workspace_slug: String,
    pub plane_project_id: Option<Uuid>,
    pub plane_work_item_id: Option<Uuid>,
    pub plane_comment_id: Option<Uuid>,
    pub plane_actor_id: Option<Uuid>,
    pub payload: serde_json::Value,
    pub processing_status: String,
    pub received_at: DateTime<Utc>,
    pub processed_at: Option<DateTime<Utc>>,
    pub error_message: Option<String>,
}

#[derive(Debug, Clone)]
pub struct NewWebhookEvent {
    pub plane_delivery_id: Option<String>,
    pub plane_event: String,
    pub plane_action: String,
    pub plane_webhook_id: Option<Uuid>,
    pub plane_workspace_id: Uuid,
    pub plane_workspace_slug: String,
    pub plane_project_id: Option<Uuid>,
    pub plane_work_item_id: Option<Uuid>,
    pub plane_comment_id: Option<Uuid>,
    pub plane_actor_id: Option<Uuid>,
    pub payload: serde_json::Value,
    pub processing_status: Option<String>,
    pub error_message: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InsertWebhookEventResult {
    Inserted(Uuid),
    Duplicate,
}

impl WebhookEventsRepo {
    pub async fn insert_webhook_event<'e, E>(
        executor: E,
        event: &NewWebhookEvent,
    ) -> DbResult<InsertWebhookEventResult>
    where
        E: Executor<'e, Database = Postgres>,
    {
        let sql = format!(
            r#"
            INSERT INTO plane_webhook_events (
                plane_delivery_id,
                plane_event,
                plane_action,
                plane_webhook_id,
                plane_workspace_id,
                plane_workspace_slug,
                plane_project_id,
                plane_work_item_id,
                plane_comment_id,
                plane_actor_id,
                payload,
                processing_status,
                error_message
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, COALESCE($12, 'received'), $13)
            ON CONFLICT DO NOTHING
            RETURNING {WEBHOOK_EVENT_COLUMNS}
            "#,
        );

        let inserted = sqlx::query_as::<_, PlaneWebhookEvent>(&sql)
            .bind(event.plane_delivery_id.as_deref())
            .bind(&event.plane_event)
            .bind(&event.plane_action)
            .bind(event.plane_webhook_id)
            .bind(event.plane_workspace_id)
            .bind(&event.plane_workspace_slug)
            .bind(event.plane_project_id)
            .bind(event.plane_work_item_id)
            .bind(event.plane_comment_id)
            .bind(event.plane_actor_id)
            .bind(&event.payload)
            .bind(event.processing_status.as_deref())
            .bind(event.error_message.as_deref())
            .fetch_optional(executor)
            .await?;

        Ok(match inserted {
            Some(event) => InsertWebhookEventResult::Inserted(event.id),
            None => InsertWebhookEventResult::Duplicate,
        })
    }

    pub async fn get_webhook_event<'e, E>(
        executor: E,
        event_id: Uuid,
    ) -> DbResult<Option<PlaneWebhookEvent>>
    where
        E: Executor<'e, Database = Postgres>,
    {
        let sql = format!("SELECT {WEBHOOK_EVENT_COLUMNS} FROM plane_webhook_events WHERE id = $1");

        sqlx::query_as::<_, PlaneWebhookEvent>(&sql)
            .bind(event_id)
            .fetch_optional(executor)
            .await
    }

    pub async fn mark_webhook_event_status<'e, E>(
        executor: E,
        event_id: Uuid,
        processing_status: &str,
        error_message: Option<&str>,
    ) -> DbResult<PlaneWebhookEvent>
    where
        E: Executor<'e, Database = Postgres>,
    {
        let sql = format!(
            r#"
            UPDATE plane_webhook_events
            SET processing_status = $2,
                processed_at = CASE
                    WHEN $2 IN ('ignored', 'processed', 'failed') THEN now()
                    ELSE processed_at
                END,
                error_message = $3
            WHERE id = $1
            RETURNING {WEBHOOK_EVENT_COLUMNS}
            "#,
        );

        sqlx::query_as::<_, PlaneWebhookEvent>(&sql)
            .bind(event_id)
            .bind(processing_status)
            .bind(error_message)
            .fetch_one(executor)
            .await
    }

    pub async fn mark_webhook_event_queued<'e, E>(
        executor: E,
        event_id: Uuid,
    ) -> DbResult<PlaneWebhookEvent>
    where
        E: Executor<'e, Database = Postgres>,
    {
        Self::mark_webhook_event_status(executor, event_id, "queued", None).await
    }

    pub async fn mark_webhook_event_processed<'e, E>(
        executor: E,
        event_id: Uuid,
    ) -> DbResult<PlaneWebhookEvent>
    where
        E: Executor<'e, Database = Postgres>,
    {
        Self::mark_webhook_event_status(executor, event_id, "processed", None).await
    }

    pub async fn mark_webhook_event_ignored<'e, E>(
        executor: E,
        event_id: Uuid,
        error_message: Option<&str>,
    ) -> DbResult<PlaneWebhookEvent>
    where
        E: Executor<'e, Database = Postgres>,
    {
        Self::mark_webhook_event_status(executor, event_id, "ignored", error_message).await
    }

    pub async fn mark_webhook_event_failed<'e, E>(
        executor: E,
        event_id: Uuid,
        error_message: Option<&str>,
    ) -> DbResult<PlaneWebhookEvent>
    where
        E: Executor<'e, Database = Postgres>,
    {
        Self::mark_webhook_event_status(executor, event_id, "failed", error_message).await
    }
}
