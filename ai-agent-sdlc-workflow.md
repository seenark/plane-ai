# AI Automation SDLC Workflow for Plane + Coding Agents

## Decisions

- Do not use Plane MCP for the production automation path. It is useful for humans in editors, but the product should use first-party Plane API tools owned by the AI Automation app.
- Do ACP later or never. Start with subprocess execution of `claude -p` / `omp -p`, capture outputs, and store sessions in the AI Automation database.
- Build custom Plane API tools and expose them to the AI Coding Agent through the automation app.
- Human stays in the loop at requirement approval, scope changes, project-info changes, and final review.
- Plane remains the project/work tracking UI. The automation app owns agent runs, audit, costs, and policy.

## System Roles

### Human Stakeholder

Owns business intent, budget, timeline, and acceptance.

Responsibilities:

- Create or approve TOR / project brief.
- Answer agent questions.
- Approve scope changes.
- Review final work.
- Approve project budget/timeline changes.

### AI Automation App

Owns orchestration and safety.

Responsibilities:

- Receive Plane webhooks.
- Read Plane project/work-item/page context.
- Store agent sessions and audit logs.
- Enforce permissions and approval gates.
- Run AI Coding Agent subprocesses.
- Call Plane API tools.
- Detect scope creep, budget risk, and timeline risk.

### AI Coding Agent

Owns code execution tasks under policy.

Responsibilities:

- Read task context given by the automation app.
- Inspect/edit code.
- Run local checks where allowed.
- Return structured result.
- Ask questions when blocked.

The coding agent should not directly mutate Plane financial/project-control records. It proposes updates; the automation app applies allowed updates after policy checks.

### Human Reviewer

Owns final acceptance.

Responsibilities:

- Review PR/code changes.
- Confirm acceptance criteria.
- Approve move to Done.

## Plane Artifacts

### Project Page: TOR / Project Control Document

Create one project-scoped Plane Page per project.

Recommended title:

```text
TOR / Project Control
```

Sections:

```md
# TOR / Project Control

## 1. Project Brief
- Problem
- Goal
- Business value
- Stakeholders

## 2. Scope
- In scope
- Out of scope
- Assumptions
- Constraints

## 3. Deliverables
- Deliverable
- Acceptance criteria
- Owner

## 4. Technical Context
- Git repository
- Main branch
- Deployment target
- Environments
- Architecture notes
- Important links

## 5. Budget and Man-days
- Approved budget
- Approved man-days
- Billing rate
- Internal cost rate
- Target margin

## 6. Timeline
- Start date
- Target date
- Milestones
- Release dates

## 7. Risk Register
- Risk
- Probability
- Impact
- Mitigation

## 8. Scope Creep Log
- Date
- Trigger work item
- Original scope
- New request
- Estimated impact
- Decision

## 9. Agent Proposed Changes
- Pending agent suggestions requiring human review

## 10. Decision Log
- Date
- Decision
- Decider
- Reason
```

Rules:

- Approved sections are human-owned.
- Agent may append proposed changes under `Agent Proposed Changes`.
- Agent may append scope-creep entries.
- Agent must not silently rewrite budget, timeline, or approved scope.
- Human can remove proposed blocks after accepting or rejecting them.

### Plane Work Items

Use work items for executable units.

Recommended work item types:

- Epic
- Feature
- Task
- Bug
- Spike
- Review
- Scope Change
- Agent Question

Recommended labels:

- `agent-ready`
- `agent-running`
- `agent-blocked`
- `agent-needs-input`
- `agent-review-ready`
- `scope-creep-detected`
- `budget-risk`
- `timeline-risk`
- `human-approval-required`

Recommended states:

```text
Backlog
Ready
Ready for Agent
In Progress
Blocked
Human Review
QA
Done
Cancelled
```

## End-to-End Workflow

```text
TOR / Project Brief
  ↓
Human approval gate
  ↓
AI decomposes into epics/tasks
  ↓
Human reviews work breakdown
  ↓
Cards enter Ready for Agent
  ↓
Agent runs on explicit trigger
  ↓
Agent asks questions or implements
  ↓
Scope/budget/timeline check
  ↓
PR / result produced
  ↓
Human final review
  ↓
Done + metrics stored
```

## Phase 1: TOR / Project Brief Intake

Trigger:

- Human creates project brief in Plane Page, or
- Human creates a work item and mentions `@agent`, or
- Human uses automation app UI to start a project.

Automation app actions:

1. Read TOR Page.
2. Check required sections.
3. If missing critical info, create questions as comments or `Agent Question` cards.
4. Produce project understanding summary.
5. Ask human to approve the brief before decomposition.

Required fields before proceeding:

- Goal
- In-scope
- Out-of-scope
- Acceptance criteria
- Git repository
- Target environment
- Budget/man-day baseline or explicit `unknown`
- Human reviewer

Gate: `Brief Approved`

Human must approve before AI creates full work breakdown.

## Phase 2: Planning and Work Breakdown

Automation app asks AI to generate:

- Epics
- Features/tasks
- Dependencies
- Acceptance criteria per task
- Risk list
- Initial estimate/man-days
- Suggested milestones

Automation app creates proposed Plane work items, but only after approval unless project policy allows auto-create.

Recommended flow:

1. Agent writes proposed breakdown to TOR Page under `Agent Proposed Changes`.
2. Agent comments summary on project planning card.
3. Human approves.
4. Automation app creates work items.
5. Automation app links created work items back to the planning card/page.

Gate: `Work Breakdown Approved`

## Phase 3: Ready for Agent

A work item becomes eligible only if one of these explicit triggers exists:

- user comments `@agent do this`
- label `agent-ready`
- state `Ready for Agent`
- assignment to agent user
- manual run from automation app UI

Default: mention trigger only.

Eligibility checks:

- Work item is not archived/deleted.
- Project is allowlisted.
- No active run exists for this work item.
- Acceptance criteria are present.
- Required repo/context is known.
- Scope is within approved TOR.

If checks fail:

- Add `agent-needs-input`.
- Comment with exact missing info.
- Do not start coding.

## Phase 4: Claim and Run

Claim protocol:

1. Create `agent_runs` row with status `queued`.
2. Check unique active run for work item.
3. Comment: `Agent accepted this task.`
4. Add label `agent-running`.
5. Move card to `In Progress` if policy allows.
6. Start coding agent subprocess.

Run command examples:

```bash
claude -p "$PROMPT"
omp -p "$PROMPT"
```

The automation app must capture:

- prompt
- stdout
- stderr
- exit code
- start/finish time
- repo path
- branch
- commit sha
- PR URL if created
- final response
- cost estimate if available

## Phase 5: Agent Execution Policy

The AI Coding Agent receives a prompt containing:

- work item title/description
- acceptance criteria
- relevant comments
- TOR Page summary
- scope constraints
- repo URL/path
- allowed commands/tools
- required output format

Agent can:

- inspect code
- edit code
- run targeted checks
- propose card updates
- ask questions
- create implementation summary

Agent cannot directly:

- update TOR budget/timeline
- mark project Done
- approve its own work
- delete cards
- archive projects
- create many cards without approval
- change project financial assumptions

## Phase 6: Human Questions / Blockers

If blocked, agent returns a structured blocker:

```json
{
  "status": "blocked",
  "question": "Which billing rule should apply for overtime?",
  "options": ["Billable", "Non-billable", "Needs separate approval"],
  "recommended_option": "Needs separate approval",
  "impact": "Cannot finalize margin prediction without this rule."
}
```

Automation app actions:

1. Store blocker in DB.
2. Comment question on Plane card.
3. Add label `agent-needs-input`.
4. Move card to `Blocked` if policy allows.
5. Set run status `waiting_for_user`.

When human replies with `@agent`, resume the same run or create a continuation run linked to the original.

## Phase 7: Scope Creep Detection

Run this before and after execution.

Inputs:

- Approved TOR scope.
- Work item original description.
- Trigger comment.
- New comments since run started.
- Files/areas changed by agent.
- Estimated effort delta.

Classifications:

```text
none
clarification
minor_scope_change
major_scope_change
out_of_scope
budget_risk
timeline_risk
```

If scope creep is detected:

1. Stop if major or out-of-scope.
2. Comment explanation.
3. Add `scope-creep-detected`.
4. Append entry under TOR Page `Scope Creep Log` or `Agent Proposed Changes`.
5. Ask human approval.
6. Optionally create a `Scope Change` work item.

Example comment:

```text
Scope creep detected.

Original approved scope:
- Mention-triggered coding agent runs.

New requested scope:
- Add margin prediction and budget analytics.

Estimated impact:
- +2.5 man-days
- Timeline risk: medium
- Margin impact: -8%

Recommendation:
Create a separate Scope Change card and approve budget impact before implementation.
```

## Phase 8: Result and Review

When coding succeeds:

1. Store final session data.
2. Comment result summary on Plane.
3. Link PR/commit/artifacts.
4. Remove `agent-running`.
5. Add `agent-review-ready`.
6. Move card to `Human Review`.
7. Log work duration if policy allows.

Result comment template:

```md
Agent completed implementation.

Summary:
- ...

Changed files:
- ...

Verification:
- ...

Artifacts:
- PR: ...
- Run: ...

Needs human review:
- ...
```

Gate: `Human Review Approved`

Only human reviewer moves to Done by default.

## Phase 9: Cost, Man-days, and Margin Tracking

Plane can store work logs and project worklog summaries through MCP docs, and Plane has estimates/time tracking concepts. Your automation app should own financial analytics.

Automation app should track:

- planned man-days
- actual human man-days
- actual agent runtime
- LLM cost
- internal cost
- billable amount
- gross margin
- margin risk

Recommended formulas:

```text
estimated_cost = estimated_man_days * internal_cost_per_day
actual_cost = human_cost + agent_cost + overhead_cost
revenue = billable_man_days * billing_rate_per_day
margin = revenue - actual_cost
margin_percent = margin / revenue * 100
```

Plane Page stores approved assumptions. Automation DB stores calculated actuals.

## Custom Plane API Tools

Do not use MCP in the production path. Create tools around Plane REST API.

Minimum tools:

```text
plane.get_project
plane.update_project
plane.get_project_page
plane.create_project_page
plane.append_project_page_proposal
plane.list_states
plane.list_work_items
plane.get_work_item
plane.create_work_item
plane.update_work_item
plane.create_comment
plane.list_comments
plane.list_activities
plane.create_work_log
plane.get_project_worklog_summary
plane.create_link
plane.add_label
plane.remove_label
```

Important: `append_project_page_proposal` may require direct Plane API support beyond MCP docs. If Page update API is limited, store proposal in your DB and add a Plane comment linking to it.

Tool policy:

- Tools validate project allowlist.
- Tools enforce approval gates.
- Tools prevent destructive actions by default.
- Tools add audit records for every write.
- Tools distinguish proposed vs approved project changes.

## Database Model

Minimum tables:

```sql
agent_runs
agent_run_events
agent_run_artifacts
agent_approvals
project_controls
scope_change_logs
agent_costs
plane_webhook_events
```

### agent_runs

```sql
id UUID PRIMARY KEY,
plane_workspace_slug TEXT NOT NULL,
plane_project_id UUID NOT NULL,
plane_work_item_id UUID,
trigger_comment_id UUID,
status TEXT NOT NULL,
prompt TEXT,
final_response TEXT,
started_at TIMESTAMPTZ,
finished_at TIMESTAMPTZ,
exit_code INT,
error_message TEXT,
repo_url TEXT,
branch_name TEXT,
commit_sha TEXT,
pr_url TEXT
```

### agent_approvals

Approvals are stored in the automation app database for auditability, but the human-facing approval action should happen in Plane, not in a separate workflow UI.

```sql
id UUID PRIMARY KEY,
run_id UUID,
approval_type TEXT NOT NULL,
requested_payload JSONB NOT NULL,
plane_work_item_id UUID,
plane_comment_id UUID,
status TEXT NOT NULL,
requested_at TIMESTAMPTZ NOT NULL,
resolved_at TIMESTAMPTZ,
resolved_by UUID
```

Approval types:

- `brief_approval`
- `work_breakdown_approval`
- `scope_change_approval`
- `project_info_update`
- `budget_update`
- `mark_done`
- `bulk_card_create`

Human approval UX:

- Agent asks for approval in the relevant Plane card comment.
- Human replies in Plane, e.g. `@agent approve`, `@agent reject`, or `@agent approve with note: ...`.
- Automation app records the approval result in `agent_approvals`.
- Automation app applies the approved Plane/API change.

## Approval Matrix

| Action | Default policy |
|---|---|
| Read project/card/page | Allowed |
| Comment on triggered card | Allowed |
| Move card to In Progress | Allowed if triggered |
| Move card to Human Review | Allowed after result |
| Move card to Done | Human approval required |
| Create one follow-up card | Approval recommended |
| Create many cards | Human approval required |
| Update project description | Human approval required |
| Update TOR approved sections | Human approval required |
| Append agent proposal to Page | Allowed |
| Change budget/man-days | Human approval required |
| Delete card/project/page | Forbidden by default |

## MVP Build Order

1. Plane API client.
2. Webhook receiver.
3. Agent run DB.
4. Mention trigger.
5. Work item fetch/comment/update tools.
6. CLI runner for `claude -p` / `omp -p`.
7. Session capture and run viewer.
8. Human question/blocker flow.
9. TOR Page reader.
10. Scope creep detector.
11. Page proposal writer or DB-backed proposal with Plane link.
12. Cost/man-day tracking.
13. Plane-native approval commands via comments.
14. Observability UI for agent runs, streaming, subagents, logs, and artifacts.
15. Optional ACP adapter later.

## Operating Principle

The AI should do the job, but humans approve business-impacting decisions.

Default automation level:

```text
AI executes work-item implementation.
AI asks when blocked.
AI proposes scope/budget/timeline changes.
Human approves final result and project-control changes.
```
