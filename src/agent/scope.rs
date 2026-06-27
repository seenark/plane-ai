use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScopeClassification {
    None,
    Clarification,
    MinorScopeChange,
    MajorScopeChange,
    OutOfScope,
    BudgetRisk,
    TimelineRisk,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ScopeAssessment {
    pub classification: ScopeClassification,
    pub original_scope: String,
    pub new_request: String,
    pub estimated_impact: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ScopeContext<'a> {
    pub approved_scope: &'a Value,
    pub work_item_description: &'a str,
    pub trigger_comment_text: &'a str,
    pub changed_files: &'a [String],
    pub budget_man_days: Option<f64>,
    pub internal_cost_per_agent_hour: f64,
    pub cost_estimate: Option<f64>,
}

pub fn classify_scope(context: &ScopeContext<'_>) -> ScopeAssessment {
    let trigger_lower = context.trigger_comment_text.to_lowercase();
    let description_lower = context.work_item_description.to_lowercase();

    let classification = if trigger_lower.contains("out of scope") {
        ScopeClassification::OutOfScope
    } else if contains_major_scope_phrase(&trigger_lower, &description_lower) {
        ScopeClassification::MajorScopeChange
    } else if exceeds_budget(context) {
        ScopeClassification::BudgetRisk
    } else if trigger_lower.contains("urgent deadline") || trigger_lower.contains("blocked release")
    {
        ScopeClassification::TimelineRisk
    } else {
        ScopeClassification::None
    };

    ScopeAssessment {
        classification,
        original_scope: original_scope_text(context.approved_scope, context.work_item_description),
        new_request: context.trigger_comment_text.to_string(),
        estimated_impact: estimated_impact(
            classification,
            context.changed_files,
            context.cost_estimate,
        ),
    }
}

fn contains_major_scope_phrase(trigger_lower: &str, description_lower: &str) -> bool {
    ["new feature", "also build", "while you're at it"]
        .into_iter()
        .any(|phrase| trigger_lower.contains(phrase) && !description_lower.contains(phrase))
}

fn exceeds_budget(context: &ScopeContext<'_>) -> bool {
    match (context.budget_man_days, context.cost_estimate) {
        (Some(budget_man_days), Some(cost_estimate)) => {
            cost_estimate > budget_man_days * 8.0 * context.internal_cost_per_agent_hour
        }
        _ => false,
    }
}

fn original_scope_text(approved_scope: &Value, work_item_description: &str) -> String {
    if !work_item_description.trim().is_empty() {
        work_item_description.to_string()
    } else {
        serde_json::to_string(approved_scope).unwrap_or_else(|_| "{}".to_string())
    }
}

fn estimated_impact(
    classification: ScopeClassification,
    changed_files: &[String],
    cost_estimate: Option<f64>,
) -> String {
    match classification {
        ScopeClassification::OutOfScope => {
            "Requested work is explicitly outside the approved scope.".to_string()
        }
        ScopeClassification::MajorScopeChange => {
            "Requested work expands delivery beyond the current description and needs approval."
                .to_string()
        }
        ScopeClassification::BudgetRisk => format!(
            "Estimated cost {} exceeds the configured budget threshold.",
            cost_estimate.unwrap_or_default()
        ),
        ScopeClassification::TimelineRisk => {
            "Requested work references schedule pressure and may impact delivery timing."
                .to_string()
        }
        ScopeClassification::Clarification => {
            "Clarification is required before execution can continue.".to_string()
        }
        ScopeClassification::MinorScopeChange => {
            "Requested change is slightly outside the current scope.".to_string()
        }
        ScopeClassification::None => {
            if changed_files.is_empty() {
                "No scope expansion detected.".to_string()
            } else {
                format!(
                    "No scope expansion detected across {} changed file(s).",
                    changed_files.len()
                )
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{classify_scope, ScopeClassification, ScopeContext};
    use serde_json::json;

    #[test]
    fn classifies_out_of_scope_from_trigger_text() {
        let assessment = classify_scope(&ScopeContext {
            approved_scope: &json!({"scope": "fix bugs"}),
            work_item_description: "Implement small fix",
            trigger_comment_text: "This is out of scope, but @agent do it",
            changed_files: &[],
            budget_man_days: None,
            internal_cost_per_agent_hour: 0.0,
            cost_estimate: None,
        });

        assert_eq!(assessment.classification, ScopeClassification::OutOfScope);
    }

    #[test]
    fn classifies_major_scope_change_when_phrase_missing_from_description() {
        let assessment = classify_scope(&ScopeContext {
            approved_scope: &json!({}),
            work_item_description: "Implement the bug fix",
            trigger_comment_text: "@agent also build a new feature while you're at it",
            changed_files: &[],
            budget_man_days: None,
            internal_cost_per_agent_hour: 0.0,
            cost_estimate: None,
        });

        assert_eq!(
            assessment.classification,
            ScopeClassification::MajorScopeChange
        );
    }

    #[test]
    fn does_not_classify_major_scope_change_when_description_already_mentions_phrase() {
        let assessment = classify_scope(&ScopeContext {
            approved_scope: &json!({}),
            work_item_description: "Please also build the reporting panel.",
            trigger_comment_text: "@agent also build the reporting panel",
            changed_files: &[],
            budget_man_days: None,
            internal_cost_per_agent_hour: 0.0,
            cost_estimate: None,
        });

        assert_eq!(assessment.classification, ScopeClassification::None);
    }

    #[test]
    fn classifies_budget_risk_from_cost_estimate() {
        let assessment = classify_scope(&ScopeContext {
            approved_scope: &json!({}),
            work_item_description: "Implement change",
            trigger_comment_text: "@agent do it",
            changed_files: &["src/main.rs".to_string()],
            budget_man_days: Some(2.0),
            internal_cost_per_agent_hour: 100.0,
            cost_estimate: Some(1700.0),
        });

        assert_eq!(assessment.classification, ScopeClassification::BudgetRisk);
    }

    #[test]
    fn classifies_timeline_risk_from_trigger_text() {
        let assessment = classify_scope(&ScopeContext {
            approved_scope: &json!({}),
            work_item_description: "Implement change",
            trigger_comment_text: "@agent urgent deadline because this blocked release",
            changed_files: &[],
            budget_man_days: None,
            internal_cost_per_agent_hour: 0.0,
            cost_estimate: None,
        });

        assert_eq!(assessment.classification, ScopeClassification::TimelineRisk);
    }
}
