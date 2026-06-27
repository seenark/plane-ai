use std::{process::Stdio, time::Duration};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tokio::{io::AsyncReadExt, process::Command};
use uuid::Uuid;

use crate::config::{Config, RunnerMode};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum RunnerOutput {
    Succeeded {
        summary: String,
        changed_files: Vec<String>,
        verification: Vec<String>,
        artifacts: RunnerArtifacts,
        #[serde(skip_serializing_if = "Option::is_none")]
        cost_estimate: Option<f64>,
    },
    Blocked {
        question: String,
        options: Vec<String>,
        recommended_option: String,
        impact: String,
    },
    Failed {
        error: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        summary: Option<String>,
    },
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunnerArtifacts {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pr_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub commit_sha: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunRequest {
    pub run_id: Uuid,
    pub prompt: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RunResult {
    pub status: RunnerExecutionStatus,
    pub output: Option<RunnerOutput>,
    pub started_at: DateTime<Utc>,
    pub finished_at: DateTime<Utc>,
    pub runtime_seconds: i64,
    pub exit_code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    pub error_message: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunnerExecutionStatus {
    Succeeded,
    WaitingForUser,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Runner {
    mode: RunnerMode,
    command: String,
    command_args: Vec<String>,
    timeout: Duration,
}

impl Runner {
    pub fn from_config(config: &Config) -> Self {
        Self::new(
            config.runner_mode,
            config.agent_command.clone(),
            config.agent_command_args.clone(),
            Duration::from_secs(config.agent_timeout_seconds),
        )
    }

    pub fn new(
        mode: RunnerMode,
        command: String,
        command_args: Vec<String>,
        timeout: Duration,
    ) -> Self {
        Self {
            mode,
            command,
            command_args,
            timeout,
        }
    }

    pub async fn execute(&self, request: &RunRequest) -> RunResult {
        match self.mode {
            RunnerMode::Fake => self.execute_fake(request),
            RunnerMode::Subprocess => self.execute_subprocess(request).await,
        }
    }

    fn execute_fake(&self, request: &RunRequest) -> RunResult {
        let started_at = Utc::now();
        let output = RunnerOutput::Succeeded {
            summary: format!("Dry-run response for run {}.", request.run_id),
            changed_files: vec![],
            verification: vec!["fake runner".to_string()],
            artifacts: RunnerArtifacts::default(),
            cost_estimate: Some(0.0),
        };
        let stdout = serde_json::to_string(&output).expect("fake runner output should serialize");
        let finished_at = Utc::now();

        RunResult {
            status: RunnerExecutionStatus::Succeeded,
            output: Some(output),
            started_at,
            finished_at,
            runtime_seconds: runtime_seconds(started_at, finished_at),
            exit_code: Some(0),
            stdout: format!("{stdout}\n"),
            stderr: String::new(),
            error_message: None,
        }
    }

    async fn execute_subprocess(&self, request: &RunRequest) -> RunResult {
        let started_at = Utc::now();
        let mut command = Command::new(&self.command);
        command
            .args(&self.command_args)
            .arg("-p")
            .arg(&request.prompt)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        let mut child = match command.spawn() {
            Ok(child) => child,
            Err(_) => {
                let finished_at = Utc::now();
                return failure_result(
                    started_at,
                    finished_at,
                    None,
                    String::new(),
                    String::new(),
                    "failed to spawn agent subprocess",
                );
            }
        };

        let stdout_handle = child.stdout.take().map(|mut stdout| {
            tokio::spawn(async move {
                let mut buffer = Vec::new();
                let _ = stdout.read_to_end(&mut buffer).await;
                buffer
            })
        });
        let stderr_handle = child.stderr.take().map(|mut stderr| {
            tokio::spawn(async move {
                let mut buffer = Vec::new();
                let _ = stderr.read_to_end(&mut buffer).await;
                buffer
            })
        });

        let exit_status = match tokio::time::timeout(self.timeout, child.wait()).await {
            Ok(Ok(status)) => Some(status),
            Ok(Err(_)) => None,
            Err(_) => {
                let _ = child.kill().await;
                let _ = child.wait().await;
                let finished_at = Utc::now();
                let stdout = join_output(stdout_handle).await;
                let stderr = join_output(stderr_handle).await;
                return failure_result(
                    started_at,
                    finished_at,
                    None,
                    stdout,
                    stderr,
                    "agent subprocess timed out",
                );
            }
        };

        let finished_at = Utc::now();
        let stdout = join_output(stdout_handle).await;
        let stderr = join_output(stderr_handle).await;
        let exit_code = exit_status.and_then(|status| status.code());

        match parse_last_non_empty_stdout_line(&stdout) {
            Ok(output) => RunResult {
                status: execution_status_for_output(&output),
                output: Some(output),
                started_at,
                finished_at,
                runtime_seconds: runtime_seconds(started_at, finished_at),
                exit_code,
                stdout,
                stderr,
                error_message: None,
            },
            Err(error) => failure_result(
                started_at,
                finished_at,
                exit_code,
                stdout,
                stderr,
                &format!("failed to parse agent output: {error}"),
            ),
        }
    }
}

pub fn parse_last_non_empty_stdout_line(stdout: &str) -> Result<RunnerOutput, serde_json::Error> {
    let line = stdout
        .lines()
        .rev()
        .find(|line| !line.trim().is_empty())
        .unwrap_or_default();

    serde_json::from_str(line.trim())
}

fn execution_status_for_output(output: &RunnerOutput) -> RunnerExecutionStatus {
    match output {
        RunnerOutput::Succeeded { .. } => RunnerExecutionStatus::Succeeded,
        RunnerOutput::Blocked { .. } => RunnerExecutionStatus::WaitingForUser,
        RunnerOutput::Failed { .. } => RunnerExecutionStatus::Failed,
    }
}

async fn join_output(handle: Option<tokio::task::JoinHandle<Vec<u8>>>) -> String {
    match handle {
        Some(handle) => match handle.await {
            Ok(bytes) => String::from_utf8_lossy(&bytes).into_owned(),
            Err(_) => String::new(),
        },
        None => String::new(),
    }
}

fn failure_result(
    started_at: DateTime<Utc>,
    finished_at: DateTime<Utc>,
    exit_code: Option<i32>,
    stdout: String,
    stderr: String,
    error_message: &str,
) -> RunResult {
    RunResult {
        status: RunnerExecutionStatus::Failed,
        output: Some(RunnerOutput::Failed {
            error: error_message.to_string(),
            summary: None,
        }),
        started_at,
        finished_at,
        runtime_seconds: runtime_seconds(started_at, finished_at),
        exit_code,
        stdout,
        stderr,
        error_message: Some(error_message.to_string()),
    }
}

fn runtime_seconds(started_at: DateTime<Utc>, finished_at: DateTime<Utc>) -> i64 {
    (finished_at - started_at).num_seconds().max(0)
}

#[cfg(test)]
mod tests {
    use super::{
        parse_last_non_empty_stdout_line, RunRequest, Runner, RunnerArtifacts,
        RunnerExecutionStatus, RunnerOutput,
    };
    use crate::config::RunnerMode;
    use std::time::Duration;
    use uuid::Uuid;

    #[test]
    fn parses_last_non_empty_stdout_line() {
        let stdout = "starting\n\n{\"status\":\"failed\",\"error\":\"ignore me\"}\n  \n{\"status\":\"succeeded\",\"summary\":\"done\",\"changed_files\":[\"src/main.rs\"],\"verification\":[\"cargo test\"],\"artifacts\":{},\"cost_estimate\":1.5}\n";
        let parsed = parse_last_non_empty_stdout_line(stdout).expect("json line should parse");

        assert_eq!(
            parsed,
            RunnerOutput::Succeeded {
                summary: "done".to_string(),
                changed_files: vec!["src/main.rs".to_string()],
                verification: vec!["cargo test".to_string()],
                artifacts: Default::default(),
                cost_estimate: Some(1.5),
            }
        );
    }

    #[tokio::test]
    async fn fake_runner_returns_deterministic_success_output() {
        let runner = Runner::new(
            RunnerMode::Fake,
            "omp".to_string(),
            vec!["--model".to_string()],
            Duration::from_secs(5),
        );
        let request = RunRequest {
            run_id: Uuid::parse_str("11111111-1111-1111-1111-111111111111").unwrap(),
            prompt: "prompt body".to_string(),
        };

        let result = runner.execute(&request).await;

        assert_eq!(result.status, RunnerExecutionStatus::Succeeded);
        assert_eq!(result.exit_code, Some(0));
        assert!(result.stderr.is_empty());
        assert_eq!(
            result.output,
            Some(RunnerOutput::Succeeded {
                summary: "Dry-run response for run 11111111-1111-1111-1111-111111111111."
                    .to_string(),
                changed_files: vec![],
                verification: vec!["fake runner".to_string()],
                artifacts: Default::default(),
                cost_estimate: Some(0.0),
            })
        );
    }

    #[tokio::test]
    async fn subprocess_runner_uses_last_non_empty_json_line() {
        let runner = Runner::new(
            RunnerMode::Subprocess,
            "python3".to_string(),
            vec![
                "-c".to_string(),
                "import sys; print('before'); print('{\\\"status\\\":\\\"succeeded\\\",\\\"summary\\\":\\\"from subprocess\\\",\\\"changed_files\\\":[\\\"src/agent/runner.rs\\\"],\\\"verification\\\":[\\\"targeted test\\\"],\\\"artifacts\\\":{\\\"commit_sha\\\":\\\"abc123\\\"}}'); print('')".to_string(),
            ],
            Duration::from_secs(5),
        );
        let request = RunRequest {
            run_id: Uuid::new_v4(),
            prompt: "ignored".to_string(),
        };

        let result = runner.execute(&request).await;

        assert_eq!(result.status, RunnerExecutionStatus::Succeeded);
        assert_eq!(result.exit_code, Some(0));
        assert!(result.stdout.contains("before"));
        assert_eq!(
            result.output,
            Some(RunnerOutput::Succeeded {
                summary: "from subprocess".to_string(),
                changed_files: vec!["src/agent/runner.rs".to_string()],
                verification: vec!["targeted test".to_string()],
                artifacts: RunnerArtifacts {
                    pr_url: None,
                    commit_sha: Some("abc123".to_string()),
                },
                cost_estimate: None,
            })
        );
    }

    #[tokio::test]
    async fn subprocess_runner_times_out_with_contract_error() {
        let runner = Runner::new(
            RunnerMode::Subprocess,
            "python3".to_string(),
            vec!["-c".to_string(), "import time; time.sleep(0.2)".to_string()],
            Duration::from_millis(10),
        );
        let request = RunRequest {
            run_id: Uuid::new_v4(),
            prompt: "ignored".to_string(),
        };

        let result = runner.execute(&request).await;

        assert_eq!(result.status, RunnerExecutionStatus::Failed);
        assert_eq!(
            result.error_message.as_deref(),
            Some("agent subprocess timed out")
        );
        assert_eq!(
            result.output,
            Some(RunnerOutput::Failed {
                error: "agent subprocess timed out".to_string(),
                summary: None,
            })
        );
    }
}
