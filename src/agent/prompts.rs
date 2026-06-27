use serde_json::Value;
use uuid::Uuid;

use crate::agent::policy::render_policy_section;

#[derive(Debug, Clone, PartialEq)]
pub struct PromptContext {
    pub work_item: PromptWorkItem,
    pub project_control: PromptProjectControl,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PromptWorkItem {
    pub id: Uuid,
    pub title: String,
    pub priority: Option<String>,
    pub current_state_id: Option<Uuid>,
    pub description_text: String,
    pub trigger_comment_text: String,
    pub latest_comments: Vec<String>,
    pub activities_summary: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PromptProjectControl {
    pub tor_markdown: String,
    pub approved_scope: Value,
    pub budget_man_days: Option<f64>,
    pub billing_rate_per_day: Option<f64>,
    pub internal_cost_rate_per_day: Option<f64>,
    pub human_reviewer_id: Option<Uuid>,
}

pub fn build_prompt(context: &PromptContext) -> String {
    let work_item = &context.work_item;
    let project_control = &context.project_control;

    format!(
        "# Plane Work Item\n\
Work item ID: {work_item_id}\n\
Title: {title}\n\
Priority: {priority}\n\
Current state ID: {current_state_id}\n\
Description text:\n{description_text}\n\
Trigger comment text:\n{trigger_comment_text}\n\
Latest comments:\n{latest_comments}\n\
Activities summary:\n{activities_summary}\n\n\
# Project Control\n\
TOR markdown:\n{tor_markdown}\n\
Approved scope JSON:\n{approved_scope}\n\
Budget man-days: {budget_man_days}\n\
Billing rate per day: {billing_rate_per_day}\n\
Internal cost rate per day: {internal_cost_rate_per_day}\n\
Human reviewer ID: {human_reviewer_id}\n\n\
# Policy\n\
{policy}\n\n\
# Required Output\n\
Return a single JSON object and nothing else.\n\
Success: {{ \"status\": \"succeeded\", \"summary\": string, \"changed_files\": string[], \"verification\": string[], \"artifacts\": {{ \"pr_url\"?: string, \"commit_sha\"?: string }}, \"cost_estimate\"?: number }}\n\
Blocked: {{ \"status\": \"blocked\", \"question\": string, \"options\": string[], \"recommended_option\": string, \"impact\": string }}\n\
Failed: {{ \"status\": \"failed\", \"error\": string, \"summary\"?: string }}\n",
        work_item_id = work_item.id,
        title = work_item.title,
        priority = work_item.priority.as_deref().unwrap_or("not set"),
        current_state_id = work_item
            .current_state_id
            .map(|value| value.to_string())
            .unwrap_or_else(|| "not set".to_string()),
        description_text = work_item.description_text,
        trigger_comment_text = work_item.trigger_comment_text,
        latest_comments = render_list(&work_item.latest_comments),
        activities_summary = render_list(&work_item.activities_summary),
        tor_markdown = project_control.tor_markdown,
        approved_scope = serde_json::to_string(&project_control.approved_scope)
            .expect("prompt approved scope should serialize"),
        budget_man_days = render_optional_f64(project_control.budget_man_days),
        billing_rate_per_day = render_optional_f64(project_control.billing_rate_per_day),
        internal_cost_rate_per_day = render_optional_f64(project_control.internal_cost_rate_per_day),
        human_reviewer_id = project_control
            .human_reviewer_id
            .map(|value| value.to_string())
            .unwrap_or_else(|| "not set".to_string()),
        policy = render_policy_section(),
    )
}

fn render_list(items: &[String]) -> String {
    if items.is_empty() {
        "- none".to_string()
    } else {
        items
            .iter()
            .map(|item| format!("- {item}"))
            .collect::<Vec<_>>()
            .join("\n")
    }
}

fn render_optional_f64(value: Option<f64>) -> String {
    value
        .map(|number| {
            let mut rendered = number.to_string();
            if !rendered.contains('.') {
                rendered.push_str(".0");
            }
            rendered
        })
        .unwrap_or_else(|| "not set".to_string())
}

#[cfg(test)]
mod tests {
    use super::{build_prompt, PromptContext, PromptProjectControl, PromptWorkItem};
    use serde_json::json;
    use uuid::Uuid;

    #[test]
    fn prompt_contains_required_sections_in_order() {
        let context = PromptContext {
            work_item: PromptWorkItem {
                id: Uuid::parse_str("11111111-1111-1111-1111-111111111111").unwrap(),
                title: "Implement agent runner".to_string(),
                priority: Some("high".to_string()),
                current_state_id: Some(
                    Uuid::parse_str("22222222-2222-2222-2222-222222222222").unwrap(),
                ),
                description_text: "Acceptance criteria\n- parses JSON".to_string(),
                trigger_comment_text: "@agent please implement it".to_string(),
                latest_comments: vec!["comment one".to_string(), "comment two".to_string()],
                activities_summary: vec!["state changed".to_string()],
            },
            project_control: PromptProjectControl {
                tor_markdown: "# TOR".to_string(),
                approved_scope: json!({"acceptance_criteria": ["it works"]}),
                budget_man_days: Some(3.0),
                billing_rate_per_day: Some(900.0),
                internal_cost_rate_per_day: Some(500.0),
                human_reviewer_id: Some(
                    Uuid::parse_str("33333333-3333-3333-3333-333333333333").unwrap(),
                ),
            },
        };

        let prompt = build_prompt(&context);

        let work_idx = prompt.find("# Plane Work Item").expect("work section");
        let control_idx = prompt
            .find("# Project Control")
            .expect("project control section");
        let policy_idx = prompt.find("# Policy").expect("policy section");
        let output_idx = prompt
            .find("# Required Output")
            .expect("required output section");

        assert!(work_idx < control_idx && control_idx < policy_idx && policy_idx < output_idx);
        assert!(prompt.contains("Work item ID: 11111111-1111-1111-1111-111111111111"));
        assert!(prompt.contains("Title: Implement agent runner"));
        assert!(prompt.contains("Priority: high"));
        assert!(prompt.contains("Current state ID: 22222222-2222-2222-2222-222222222222"));
        assert!(prompt.contains("Trigger comment text:\n@agent please implement it"));
        assert!(prompt.contains("Latest comments:\n- comment one\n- comment two"));
        assert!(prompt.contains("Activities summary:\n- state changed"));
        assert!(prompt.contains("Approved scope JSON:\n{\"acceptance_criteria\":[\"it works\"]}"));
        assert!(prompt.contains("Human reviewer ID: 33333333-3333-3333-3333-333333333333"));
        assert!(prompt.contains("{ \"status\": \"succeeded\", \"summary\": string, \"changed_files\": string[], \"verification\": string[], \"artifacts\": { \"pr_url\"?: string, \"commit_sha\"?: string }, \"cost_estimate\"?: number }"));
        assert!(prompt.contains("{ \"status\": \"blocked\", \"question\": string, \"options\": string[], \"recommended_option\": string, \"impact\": string }"));
        assert!(
            prompt.contains("{ \"status\": \"failed\", \"error\": string, \"summary\"?: string }")
        );
    }

    #[test]
    fn prompt_embeds_policy_text() {
        let context = PromptContext {
            work_item: PromptWorkItem {
                id: Uuid::nil(),
                title: "Title".to_string(),
                priority: None,
                current_state_id: None,
                description_text: "Description".to_string(),
                trigger_comment_text: "Comment".to_string(),
                latest_comments: vec![],
                activities_summary: vec![],
            },
            project_control: PromptProjectControl {
                tor_markdown: "TOR".to_string(),
                approved_scope: json!({}),
                budget_man_days: None,
                billing_rate_per_day: None,
                internal_cost_rate_per_day: None,
                human_reviewer_id: None,
            },
        };

        let prompt = build_prompt(&context);

        assert!(prompt.contains(&crate::agent::policy::render_policy_section()));
    }
}
