use std::{env, net::SocketAddr, str::FromStr};

use thiserror::Error;
use url::Url;
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq)]
pub struct Config {
    pub database_url: String,
    pub plane_base_url: Url,
    pub plane_api_key: String,
    pub plane_webhook_secret: Option<String>,
    pub plane_workspace_slug: String,
    pub allowed_project_ids: Vec<Uuid>,
    pub agent_user_id: Uuid,
    pub agent_mentions: Vec<String>,
    pub runner_mode: RunnerMode,
    pub agent_command: String,
    pub agent_command_args: Vec<String>,
    pub in_progress_state_name: String,
    pub human_review_state_name: String,
    pub blocked_state_name: String,
    pub agent_running_label_name: String,
    pub agent_blocked_label_name: String,
    pub agent_review_ready_label_name: String,
    pub internal_cost_per_agent_hour: f64,
    pub billing_rate_per_day: f64,
    pub app_bind_addr: SocketAddr,
    pub agent_timeout_seconds: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunnerMode {
    Fake,
    Subprocess,
}

impl RunnerMode {
    fn parse(value: Option<String>) -> Result<Self, ConfigError> {
        match value.as_deref().unwrap_or("subprocess") {
            "fake" => Ok(Self::Fake),
            "subprocess" => Ok(Self::Subprocess),
            other => Err(ConfigError::InvalidEnum {
                key: "RUNNER_MODE",
                value: other.to_string(),
            }),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            RunnerMode::Fake => "fake",
            RunnerMode::Subprocess => "subprocess",
        }
    }
}

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("missing required environment variable {0}")]
    Missing(&'static str),
    #[error("invalid URL for {key}: {value}")]
    InvalidUrl { key: &'static str, value: String },
    #[error("invalid UUID for {key}: {value}")]
    InvalidUuid { key: &'static str, value: String },
    #[error("invalid comma-separated UUID list for {key}: {value}")]
    InvalidUuidList { key: &'static str, value: String },
    #[error("invalid decimal for {key}: {value}")]
    InvalidDecimal { key: &'static str, value: String },
    #[error("invalid socket address for {key}: {value}")]
    InvalidSocketAddr { key: &'static str, value: String },
    #[error("invalid integer for {key}: {value}")]
    InvalidInteger { key: &'static str, value: String },
    #[error("invalid enum value for {key}: {value}")]
    InvalidEnum { key: &'static str, value: String },
}

impl Config {
    pub fn from_env() -> Result<Self, ConfigError> {
        Self::from_iter(env::vars())
    }

    pub fn from_iter<I>(vars: I) -> Result<Self, ConfigError>
    where
        I: IntoIterator<Item = (String, String)>,
    {
        let env = vars
            .into_iter()
            .collect::<std::collections::HashMap<_, _>>();
        let get_required = |key: &'static str| {
            env.get(key)
                .cloned()
                .filter(|value| !value.trim().is_empty())
                .ok_or(ConfigError::Missing(key))
        };
        let get_optional = |key: &'static str| env.get(key).cloned();

        let plane_base_raw = get_required("PLANE_BASE_URL")?;
        let plane_base_url = Url::parse(&plane_base_raw).map_err(|_| ConfigError::InvalidUrl {
            key: "PLANE_BASE_URL",
            value: plane_base_raw.clone(),
        })?;

        let allowed_raw = get_required("ALLOWED_PROJECT_IDS")?;
        let allowed_project_ids = parse_uuid_list("ALLOWED_PROJECT_IDS", &allowed_raw)?;

        let agent_user_raw = get_required("AGENT_USER_ID")?;
        let agent_user_id =
            Uuid::parse_str(&agent_user_raw).map_err(|_| ConfigError::InvalidUuid {
                key: "AGENT_USER_ID",
                value: agent_user_raw.clone(),
            })?;

        let agent_mentions = split_csv(get_optional("AGENT_MENTIONS").as_deref())
            .filter(|entries| !entries.is_empty())
            .unwrap_or_else(|| vec!["@agent".to_string(), "@ai".to_string()]);

        let app_bind_addr = match get_optional("APP_BIND_ADDR") {
            Some(value) => {
                SocketAddr::from_str(&value).map_err(|_| ConfigError::InvalidSocketAddr {
                    key: "APP_BIND_ADDR",
                    value,
                })?
            }
            None => SocketAddr::from_str("0.0.0.0:3000").expect("default bind addr is valid"),
        };

        let agent_timeout_seconds = match get_optional("AGENT_TIMEOUT_SECONDS") {
            Some(value) => value
                .parse::<u64>()
                .map_err(|_| ConfigError::InvalidInteger {
                    key: "AGENT_TIMEOUT_SECONDS",
                    value,
                })?,
            None => 7200,
        };

        Ok(Self {
            database_url: get_required("DATABASE_URL")?,
            plane_base_url,
            plane_api_key: get_required("PLANE_API_KEY")?,
            plane_webhook_secret: get_optional("PLANE_WEBHOOK_SECRET")
                .filter(|value| !value.trim().is_empty()),
            plane_workspace_slug: get_required("PLANE_WORKSPACE_SLUG")?,
            allowed_project_ids,
            agent_user_id,
            agent_mentions,
            runner_mode: RunnerMode::parse(get_optional("RUNNER_MODE"))?,
            agent_command: get_optional("AGENT_COMMAND")
                .filter(|value| !value.trim().is_empty())
                .unwrap_or_else(|| "omp".to_string()),
            agent_command_args: split_csv(get_optional("AGENT_COMMAND_ARGS").as_deref())
                .unwrap_or_default(),
            in_progress_state_name: get_optional("IN_PROGRESS_STATE_NAME")
                .unwrap_or_else(|| "In Progress".to_string()),
            human_review_state_name: get_optional("HUMAN_REVIEW_STATE_NAME")
                .unwrap_or_else(|| "Human Review".to_string()),
            blocked_state_name: get_optional("BLOCKED_STATE_NAME")
                .unwrap_or_else(|| "Blocked".to_string()),
            agent_running_label_name: get_optional("AGENT_RUNNING_LABEL_NAME")
                .unwrap_or_else(|| "agent-running".to_string()),
            agent_blocked_label_name: get_optional("AGENT_BLOCKED_LABEL_NAME")
                .unwrap_or_else(|| "agent-blocked".to_string()),
            agent_review_ready_label_name: get_optional("AGENT_REVIEW_READY_LABEL_NAME")
                .unwrap_or_else(|| "agent-review-ready".to_string()),
            internal_cost_per_agent_hour: parse_decimal(
                "INTERNAL_COST_PER_AGENT_HOUR",
                get_optional("INTERNAL_COST_PER_AGENT_HOUR")
                    .as_deref()
                    .unwrap_or("0"),
            )?,
            billing_rate_per_day: parse_decimal(
                "BILLING_RATE_PER_DAY",
                get_optional("BILLING_RATE_PER_DAY")
                    .as_deref()
                    .unwrap_or("0"),
            )?,
            app_bind_addr,
            agent_timeout_seconds,
        })
    }
}

fn parse_decimal(key: &'static str, value: &str) -> Result<f64, ConfigError> {
    value
        .parse::<f64>()
        .map_err(|_| ConfigError::InvalidDecimal {
            key,
            value: value.to_string(),
        })
}

fn parse_uuid_list(key: &'static str, value: &str) -> Result<Vec<Uuid>, ConfigError> {
    split_csv(Some(value))
        .unwrap_or_default()
        .into_iter()
        .map(|entry| {
            Uuid::parse_str(&entry).map_err(|_| ConfigError::InvalidUuidList {
                key,
                value: value.to_string(),
            })
        })
        .collect()
}

fn split_csv(value: Option<&str>) -> Option<Vec<String>> {
    value.map(|raw| {
        raw.split(',')
            .map(str::trim)
            .filter(|entry| !entry.is_empty())
            .map(ToOwned::to_owned)
            .collect::<Vec<_>>()
    })
}

#[cfg(test)]
mod tests {
    use super::{Config, ConfigError, RunnerMode};

    fn base_env() -> Vec<(String, String)> {
        vec![
            (
                "DATABASE_URL".into(),
                "postgres://postgres:postgres@localhost/plane_ai".into(),
            ),
            ("PLANE_BASE_URL".into(), "https://plane.example.com".into()),
            ("PLANE_API_KEY".into(), "plane_api_123".into()),
            ("PLANE_WORKSPACE_SLUG".into(), "demo".into()),
            (
                "ALLOWED_PROJECT_IDS".into(),
                "11111111-1111-1111-1111-111111111111,22222222-2222-2222-2222-222222222222".into(),
            ),
            (
                "AGENT_USER_ID".into(),
                "33333333-3333-3333-3333-333333333333".into(),
            ),
        ]
    }

    #[test]
    fn config_parses_required_and_default_values() {
        let config = Config::from_iter(base_env()).expect("config should parse");
        assert_eq!(config.runner_mode, RunnerMode::Subprocess);
        assert_eq!(config.agent_mentions, vec!["@agent", "@ai"]);
        assert_eq!(config.agent_command, "omp");
        assert_eq!(config.agent_command_args, Vec::<String>::new());
        assert_eq!(config.in_progress_state_name, "In Progress");
        assert_eq!(config.human_review_state_name, "Human Review");
        assert_eq!(config.blocked_state_name, "Blocked");
        assert_eq!(config.agent_running_label_name, "agent-running");
        assert_eq!(config.agent_blocked_label_name, "agent-blocked");
        assert_eq!(config.agent_review_ready_label_name, "agent-review-ready");
        assert_eq!(config.internal_cost_per_agent_hour, 0.0);
        assert_eq!(config.billing_rate_per_day, 0.0);
        assert_eq!(config.agent_timeout_seconds, 7200);
    }

    #[test]
    fn config_rejects_missing_allowed_projects() {
        let env = base_env()
            .into_iter()
            .filter(|(key, _)| key != "ALLOWED_PROJECT_IDS")
            .collect::<Vec<_>>();
        let error = Config::from_iter(env).expect_err("config should fail");
        assert!(matches!(error, ConfigError::Missing("ALLOWED_PROJECT_IDS")));
    }

    #[test]
    fn config_parses_optional_csv_values() {
        let mut env = base_env();
        env.push(("AGENT_MENTIONS".into(), "@bot,@helper".into()));
        env.push(("AGENT_COMMAND_ARGS".into(), "--model,sonnet".into()));
        env.push(("RUNNER_MODE".into(), "fake".into()));

        let config = Config::from_iter(env).expect("config should parse");
        assert_eq!(config.runner_mode, RunnerMode::Fake);
        assert_eq!(config.agent_mentions, vec!["@bot", "@helper"]);
        assert_eq!(config.agent_command_args, vec!["--model", "sonnet"]);
    }
}
