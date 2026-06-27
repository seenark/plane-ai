pub mod routes;

use sqlx::PgPool;

use crate::{agent::runner::Runner, config::Config, plane::PlaneClient};

#[derive(Clone)]
pub struct AppState {
    pub config: Config,
    pub pool: PgPool,
    pub plane: PlaneClient,
    pub runner: Runner,
}
