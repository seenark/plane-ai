pub mod project_controls;
pub mod runs;
pub mod webhook_events;

pub type DbResult<T> = Result<T, sqlx::Error>;
