pub const ALLOWED_ACTIONS: &[&str] = &[
    "inspect code",
    "edit code",
    "run targeted checks",
    "ask blockers",
    "summarize results",
];

pub const FORBIDDEN_ACTIONS: &[&str] = &[
    "mutate Plane budget",
    "mutate Plane timeline",
    "mark Done",
    "delete cards",
    "archive cards",
    "create many cards",
    "approve its own work",
    "change financial assumptions",
];

pub fn render_policy_section() -> String {
    let mut lines = vec![
        "Allowed actions:".to_string(),
        "- inspect/edit code needed for the assigned task".to_string(),
        "- run targeted checks that validate the touched changes".to_string(),
        "- ask blockers when required information is missing".to_string(),
        "- summarize results, changed files, and verification clearly".to_string(),
        String::new(),
        "Forbidden actions:".to_string(),
        "- do not mutate Plane budget or timeline".to_string(),
        "- do not mark the card Done".to_string(),
        "- do not delete or archive Plane cards".to_string(),
        "- do not create many cards in bulk".to_string(),
        "- do not approve your own work".to_string(),
        "- do not change financial assumptions".to_string(),
    ];

    for action in ALLOWED_ACTIONS {
        if !lines.iter().any(|line| line.contains(action)) {
            lines.push(format!("- {action}"));
        }
    }

    for action in FORBIDDEN_ACTIONS {
        if !lines.iter().any(|line| line.contains(action)) {
            lines.push(format!("- {action}"));
        }
    }

    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::{render_policy_section, ALLOWED_ACTIONS, FORBIDDEN_ACTIONS};

    #[test]
    fn policy_lists_allowed_and_forbidden_actions() {
        let policy = render_policy_section();

        assert!(policy.contains("Allowed actions:"));
        assert!(policy.contains("Forbidden actions:"));

        for allowed in ALLOWED_ACTIONS {
            assert!(policy.contains(allowed), "missing allowed action {allowed}");
        }

        for forbidden in FORBIDDEN_ACTIONS {
            assert!(
                policy.contains(forbidden),
                "missing forbidden action {forbidden}"
            );
        }
    }
}
