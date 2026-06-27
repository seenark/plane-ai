use std::process::ExitCode;

use sqlx::postgres::PgPoolOptions;
use tracing::{error, warn};
use tracing_subscriber::{fmt, EnvFilter};

use plane_ai::{
    agent::runner::Runner,
    config::Config,
    http::{routes::router, AppState},
    plane::PlaneClient,
};

#[tokio::main]
async fn main() -> ExitCode {
    init_tracing();

    let config = match Config::from_env() {
        Ok(config) => config,
        Err(error) => {
            error!(error = %error, "failed to load config");
            return ExitCode::FAILURE;
        }
    };

    if config.plane_webhook_secret.is_none() {
        warn!("PLANE_WEBHOOK_SECRET is not configured; accepting unsigned webhooks for local development only");
    }

    let pool = match PgPoolOptions::new()
        .max_connections(5)
        .connect(&config.database_url)
        .await
    {
        Ok(pool) => pool,
        Err(error) => {
            error!(error = %error, "failed to connect to database");
            return ExitCode::FAILURE;
        }
    };

    if let Err(error) = sqlx::migrate!().run(&pool).await {
        error!(error = %error, "failed to run database migrations");
        return ExitCode::FAILURE;
    }

    let state = AppState {
        plane: PlaneClient::new(config.plane_base_url.clone(), config.plane_api_key.clone()),
        runner: Runner::from_config(&config),
        pool,
        config,
    };
    let app = router(state.clone());
    let listener = match tokio::net::TcpListener::bind(state.config.app_bind_addr).await {
        Ok(listener) => listener,
        Err(error) => {
            error!(error = %error, "failed to bind application listener");
            return ExitCode::FAILURE;
        }
    };

    if let Err(error) = axum::serve(listener, app).await {
        error!(error = %error, "application server exited with error");
        return ExitCode::FAILURE;
    }

    ExitCode::SUCCESS
}

fn init_tracing() {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    fmt().with_env_filter(filter).json().init();
}
