use uuid::Uuid;

use crate::plane::{
    Activity, Comment, Label, PlaneClient, PlaneError, ProjectMember, State, WorkItem,
    WorkItemPatch,
};

pub struct PlaneToolset {
    client: PlaneClient,
}

impl PlaneToolset {
    pub const GET_WORK_ITEM: &'static str = "plane.get_work_item";
    pub const UPDATE_WORK_ITEM: &'static str = "plane.update_work_item";
    pub const CREATE_COMMENT: &'static str = "plane.create_comment";
    pub const LIST_COMMENTS: &'static str = "plane.list_comments";
    pub const LIST_ACTIVITIES: &'static str = "plane.list_activities";
    pub const LIST_STATES: &'static str = "plane.list_states";
    pub const LIST_LABELS: &'static str = "plane.list_labels";
    pub const LIST_PROJECT_MEMBERS: &'static str = "plane.list_project_members";

    pub fn new(client: PlaneClient) -> Self {
        Self { client }
    }

    pub fn client(&self) -> &PlaneClient {
        &self.client
    }

    pub async fn get_work_item(
        &self,
        workspace: &str,
        project_id: Uuid,
        item_id: Uuid,
    ) -> Result<WorkItem, PlaneError> {
        self.client
            .get_work_item(workspace, project_id, item_id)
            .await
    }

    pub async fn update_work_item(
        &self,
        workspace: &str,
        project_id: Uuid,
        item_id: Uuid,
        patch: WorkItemPatch,
    ) -> Result<WorkItem, PlaneError> {
        self.client
            .patch_work_item(workspace, project_id, item_id, patch)
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
        self.client
            .create_comment(workspace, project_id, item_id, html, external_id)
            .await
    }

    pub async fn list_comments(
        &self,
        workspace: &str,
        project_id: Uuid,
        item_id: Uuid,
    ) -> Result<Vec<Comment>, PlaneError> {
        self.client
            .list_comments(workspace, project_id, item_id)
            .await
    }

    pub async fn list_activities(
        &self,
        workspace: &str,
        project_id: Uuid,
        item_id: Uuid,
    ) -> Result<Vec<Activity>, PlaneError> {
        self.client
            .list_activities(workspace, project_id, item_id)
            .await
    }

    pub async fn list_states(
        &self,
        workspace: &str,
        project_id: Uuid,
    ) -> Result<Vec<State>, PlaneError> {
        self.client.list_states(workspace, project_id).await
    }

    pub async fn list_labels(
        &self,
        workspace: &str,
        project_id: Uuid,
    ) -> Result<Vec<Label>, PlaneError> {
        self.client.list_labels(workspace, project_id).await
    }

    pub async fn list_project_members(
        &self,
        workspace: &str,
        project_id: Uuid,
    ) -> Result<Vec<ProjectMember>, PlaneError> {
        self.client
            .list_project_members(workspace, project_id)
            .await
    }
}
