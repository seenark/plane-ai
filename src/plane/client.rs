use reqwest::{Client, Method, Response, StatusCode};
use serde::de::DeserializeOwned;
use thiserror::Error;
use url::Url;
use uuid::Uuid;

use super::types::{
    Activity, Comment, IdResponse, Label, Project, ProjectMember, State, WorkItem, WorkItemPatch,
};

#[derive(Debug, Clone)]
pub struct PlaneClient {
    pub base_url: Url,
    pub api_key: String,
    pub http: Client,
}

#[derive(Debug, Error)]
pub enum PlaneError {
    #[error("invalid Plane API path: {0}")]
    InvalidPath(String),
    #[error("Plane API request failed: {0}")]
    Request(#[from] reqwest::Error),
    #[error("Plane API returned {status}: {body}")]
    HttpStatus { status: StatusCode, body: String },
    #[error("Plane API returned conflict without reusable comment id")]
    ConflictWithoutId,
}

impl PlaneClient {
    pub fn new(base_url: Url, api_key: impl Into<String>) -> Self {
        Self {
            base_url,
            api_key: api_key.into(),
            http: Client::new(),
        }
    }

    pub fn with_http(base_url: Url, api_key: impl Into<String>, http: Client) -> Self {
        Self {
            base_url,
            api_key: api_key.into(),
            http,
        }
    }

    pub async fn list_projects(&self, workspace: &str) -> Result<Vec<Project>, PlaneError> {
        self.get_json(&format!("/api/v1/workspaces/{workspace}/projects/"))
            .await
    }

    pub async fn list_states(
        &self,
        workspace: &str,
        project_id: Uuid,
    ) -> Result<Vec<State>, PlaneError> {
        self.get_json(&format!(
            "/api/v1/workspaces/{workspace}/projects/{project_id}/states/"
        ))
        .await
    }

    pub async fn list_labels(
        &self,
        workspace: &str,
        project_id: Uuid,
    ) -> Result<Vec<Label>, PlaneError> {
        self.get_json(&format!(
            "/api/v1/workspaces/{workspace}/projects/{project_id}/labels/"
        ))
        .await
    }

    pub async fn list_project_members(
        &self,
        workspace: &str,
        project_id: Uuid,
    ) -> Result<Vec<ProjectMember>, PlaneError> {
        self.get_json(&format!(
            "/api/v1/workspaces/{workspace}/projects/{project_id}/members/"
        ))
        .await
    }

    pub async fn get_work_item(
        &self,
        workspace: &str,
        project_id: Uuid,
        item_id: Uuid,
    ) -> Result<WorkItem, PlaneError> {
        self.get_json(&format!(
            "/api/v1/workspaces/{workspace}/projects/{project_id}/work-items/{item_id}/"
        ))
        .await
    }

    pub async fn patch_work_item(
        &self,
        workspace: &str,
        project_id: Uuid,
        item_id: Uuid,
        patch: WorkItemPatch,
    ) -> Result<WorkItem, PlaneError> {
        self.send_json(
            Method::PATCH,
            &format!("/api/v1/workspaces/{workspace}/projects/{project_id}/work-items/{item_id}/"),
            Some(&patch),
        )
        .await
    }

    pub async fn list_comments(
        &self,
        workspace: &str,
        project_id: Uuid,
        item_id: Uuid,
    ) -> Result<Vec<Comment>, PlaneError> {
        self.get_json(&format!(
            "/api/v1/workspaces/{workspace}/projects/{project_id}/work-items/{item_id}/comments/"
        ))
        .await
    }

    pub async fn create_comment(
        &self,
        workspace: &str,
        project_id: Uuid,
        item_id: Uuid,
        html: &str,
        external_id: Option<&str>,
    ) -> Result<Comment, PlaneError> {
        let payload = super::types::CreateCommentRequest::new(html, external_id);
        let response = self
            .request(
                Method::POST,
                &format!(
                    "/api/v1/workspaces/{workspace}/projects/{project_id}/work-items/{item_id}/comments/"
                ),
            )?
            .json(&payload)
            .send()
            .await?;

        if response.status() == StatusCode::CONFLICT {
            let body = response.text().await?;
            let id = serde_json::from_str::<IdResponse>(&body)
                .ok()
                .and_then(|parsed| parsed.id);
            return id
                .map(Comment::from_conflict_id)
                .ok_or(PlaneError::ConflictWithoutId);
        }

        Self::parse_json_response(response).await
    }

    pub async fn list_activities(
        &self,
        workspace: &str,
        project_id: Uuid,
        item_id: Uuid,
    ) -> Result<Vec<Activity>, PlaneError> {
        self.get_json(&format!(
            "/api/v1/workspaces/{workspace}/projects/{project_id}/work-items/{item_id}/activities/"
        ))
        .await
    }

    async fn get_json<T>(&self, path: &str) -> Result<T, PlaneError>
    where
        T: DeserializeOwned,
    {
        self.send_json::<(), T>(Method::GET, path, None).await
    }

    async fn send_json<B, T>(
        &self,
        method: Method,
        path: &str,
        body: Option<&B>,
    ) -> Result<T, PlaneError>
    where
        B: serde::Serialize + ?Sized,
        T: DeserializeOwned,
    {
        let mut request = self.request(method, path)?;
        if let Some(body) = body {
            request = request.json(body);
        }
        let response = request.send().await?;
        Self::parse_json_response(response).await
    }

    fn request(&self, method: Method, path: &str) -> Result<reqwest::RequestBuilder, PlaneError> {
        let url = self
            .base_url
            .join(path.trim_start_matches('/'))
            .map_err(|_| PlaneError::InvalidPath(path.to_string()))?;
        Ok(self
            .http
            .request(method, url)
            .header("X-Api-Key", &self.api_key))
    }

    async fn parse_json_response<T>(response: Response) -> Result<T, PlaneError>
    where
        T: DeserializeOwned,
    {
        if response.status().is_success() {
            return Ok(response.json::<T>().await?);
        }

        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        Err(PlaneError::HttpStatus { status, body })
    }
}
