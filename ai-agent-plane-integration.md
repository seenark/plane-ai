# AI Agent App Integration with Plane Boards

## Goal

Build a completely separate AI Agent app that integrates with Plane boards/work items. The agent should act only when explicitly triggered by a human, especially when a user mentions the agent on a card/comment. It should not autonomously pick every available card unless that behavior is later enabled by configuration.

## Recommended Architecture

Use Plane webhooks as the trigger layer and Plane `/api/v1/` as the action layer.

```text
Plane
  ├─ Webhooks: notify the agent app about comments / issue updates
  └─ /api/v1/: agent reads and mutates work items using X-Api-Key

AI Agent App
  ├─ webhook receiver
  ├─ mention detector
  ├─ Plane API client
  ├─ run/claim database
  ├─ agent worker
  └─ audit/log storage
```

Why this architecture:

- Webhooks give near-realtime triggers.
- `/api/v1/` gives stable external API access for reading/updating cards.
- The agent stays outside Plane, so Plane does not need to be forked.
- Human intent is explicit: the agent only acts on mentions, labels, assignments, or configured triggers.

## Confirmed Plane Surfaces

From the Plane repo:

- Root routing: `apps/api/plane/urls.py`
  - `/api/` routes to the session-authenticated app API.
  - `/api/v1/` routes to the external API-key API.
- API-key authentication: `apps/api/plane/api/middleware/api_authentication.py`
  - Reads `X-Api-Key`.
  - Loads `APIToken`.
  - Authenticates requests as `APIToken.user`.
- External API base: `apps/api/plane/api/views/base.py`
  - Uses `APIKeyAuthentication`.
  - Requires authenticated user.
  - Applies API-key throttling.
- Work item routes: `apps/api/plane/api/urls/work_item.py`
  - Work items/cards.
  - Comments.
  - Activities.
  - Attachments.
  - Links.
  - Relations.
- State routes: `apps/api/plane/api/urls/state.py`
  - Board columns/workflow states.
- Webhook routes: `apps/api/plane/app/urls/webhook.py`
  - Session-authenticated workspace webhook management.
- Webhook dispatch: `apps/api/plane/bgtasks/webhook_task.py`
  - Emits events for issue, issue_comment, project, cycle, module, etc.

## Authentication Model

External API calls use:

```http
X-Api-Key: plane_api_xxx
```

Important constraints:

- The API key acts as a specific Plane user.
- There is no separate OAuth/app-install flow in the inspected code.
- There are no fine-grained token scopes in the inspected code.
- Normal workspace/project permissions still apply.
- API tokens are created through the session API, not `/api/v1/`:

```http
POST /api/users/api-tokens/
```

Practical recommendation:

- Create a dedicated Plane user for the agent, e.g. `ai-agent@company.com`.
- Add that user to the needed workspaces/projects as Member or Admin.
- Generate an API token for that user.
- Store the API token only in the AI Agent app secret store.

## Core Domain Model

Plane boards are project-scoped. Cards are work items/issues. Columns are states.

```text
Workspace
  Project
    State = board column
    Issue = card / work item
      Assignees
      Labels
      Comments
      Activities
      Cycle
      Module
```

Minimum objects the AI app should understand:

```ts
type Workspace = {
  id: string;
  slug: string;
};

type Project = {
  id: string;
  identifier: string;
  name: string;
  default_state?: string | null;
  guest_view_all_features?: boolean;
  archived_at?: string | null;
};

type State = {
  id: string;
  name: string;
  color: string;
  group: "backlog" | "unstarted" | "started" | "completed" | "cancelled";
  default: boolean;
  sequence: number;
};

type WorkItem = {
  id: string;
  sequence_id: number;
  project_id: string;
  name: string;
  description_html?: string;
  state_id: string | null;
  sort_order: number;
  priority: "urgent" | "high" | "medium" | "low" | "none" | null;
  assignee_ids: string[];
  label_ids: string[];
  cycle_id: string | null;
  module_ids: string[] | null;
  parent_id: string | null;
  start_date: string | null;
  target_date: string | null;
  completed_at: string | null;
  archived_at: string | null;
  created_by: string;
  updated_at: string;
};
```

## API Operations Needed

Use canonical `/api/v1/.../work-items/` paths.

### List projects

```http
GET /api/v1/workspaces/:workspace_slug/projects/
X-Api-Key: plane_api_xxx
```

### List states / board columns

```http
GET /api/v1/workspaces/:workspace_slug/projects/:project_id/states/
X-Api-Key: plane_api_xxx
```

### List cards / work items

```http
GET /api/v1/workspaces/:workspace_slug/projects/:project_id/work-items/
X-Api-Key: plane_api_xxx
```

Useful query concepts:

- pagination/cursor params
- `fields`
- `expand`
- ordering/filtering params as needed

### Retrieve one card

```http
GET /api/v1/workspaces/:workspace_slug/projects/:project_id/work-items/:work_item_id/
X-Api-Key: plane_api_xxx
```

### Move a card between columns

Plane has no dedicated move-card endpoint. Move by patching the work item's state.

```http
PATCH /api/v1/workspaces/:workspace_slug/projects/:project_id/work-items/:work_item_id/
X-Api-Key: plane_api_xxx
Content-Type: application/json

{
  "state": "target-state-uuid"
}
```

Depending on API serializer behavior and route, `state` is the external API field. In app/web code, UI uses `state_id`; test both against the target Plane version and standardize in the client.

### Update card properties

```http
PATCH /api/v1/workspaces/:workspace_slug/projects/:project_id/work-items/:work_item_id/
X-Api-Key: plane_api_xxx
Content-Type: application/json

{
  "priority": "high",
  "assignees": ["user-uuid"],
  "labels": ["label-uuid"],
  "target_date": "2026-07-01"
}
```

Important: assignee and label updates are replace-all writes in the inspected serializers. Always read current values, merge intentionally, then patch the full desired set.

### Add a comment

```http
POST /api/v1/workspaces/:workspace_slug/projects/:project_id/work-items/:work_item_id/comments/
X-Api-Key: plane_api_xxx
Content-Type: application/json

{
  "comment_html": "<p>Agent accepted this task.</p>"
}
```

Field names may differ by serializer/version. Confirm against the running API schema or a small integration test.

### Read activity

```http
GET /api/v1/workspaces/:workspace_slug/projects/:project_id/work-items/:work_item_id/activities/
X-Api-Key: plane_api_xxx
```

### Assign cycle/module membership

Cycle membership:

```http
POST /api/v1/workspaces/:workspace_slug/projects/:project_id/cycles/:cycle_id/cycle-issues/
X-Api-Key: plane_api_xxx
Content-Type: application/json

{
  "issues": ["work-item-uuid"]
}
```

Module membership:

```http
POST /api/v1/workspaces/:workspace_slug/projects/:project_id/modules/:module_id/module-issues/
X-Api-Key: plane_api_xxx
Content-Type: application/json

{
  "issues": ["work-item-uuid"]
}
```

## Trigger Model

The agent should not process every card automatically. Use explicit human triggers.

Recommended trigger priority:

1. Mention trigger: user comments `@agent ...`.
2. Assignment trigger: user assigns card to the agent user.
3. Label trigger: user adds `agent-ready`.
4. State trigger: user moves card to `Ready for Agent`.
5. Manual trigger: user clicks a button in the separate app.

Start with mention trigger only.

## Mention Trigger Flow

```text
1. Human comments on Plane work item:
   "@agent please investigate this"

2. Plane sends issue_comment webhook to AI Agent app.

3. AI Agent app validates webhook signature if available.

4. AI Agent app checks:
   - event is issue_comment created/updated
   - comment contains @agent mention
   - comment is not authored by the agent itself
   - work item/project is allowed by config
   - no active run exists for this work_item_id

5. AI Agent app fetches latest work item from /api/v1/.

6. AI Agent app creates a run row in its own DB.

7. AI Agent app claims card:
   - comment: "Agent accepted this task. Run: ..."
   - optionally assign agent user
   - optionally add agent-working label
   - optionally move state to In Progress

8. Agent executes work.

9. Agent comments result.

10. Agent moves card to Review/Done/Blocked depending on result.
```

## Claim Protocol

Plane does not expose a first-class lock/claim endpoint. Implement claims in the AI app database.

Suggested table:

```sql
CREATE TABLE agent_runs (
  id UUID PRIMARY KEY,
  plane_workspace_slug TEXT NOT NULL,
  plane_project_id UUID NOT NULL,
  plane_work_item_id UUID NOT NULL,
  trigger_comment_id UUID,
  trigger_user_id UUID,
  status TEXT NOT NULL,
  started_at TIMESTAMPTZ NOT NULL,
  finished_at TIMESTAMPTZ,
  UNIQUE (plane_work_item_id) WHERE status IN ('queued', 'running')
);
```

Statuses:

- `queued`
- `running`
- `succeeded`
- `failed`
- `cancelled`
- `ignored`

Behavior:

- If an active run already exists, ignore duplicate mention or reply that the task is already running.
- Make webhook handling idempotent by storing processed webhook IDs or `(event_type, comment_id)`.
- Never rely only on Plane labels/states as locks; they are useful UI signals, not atomic locks.

## Race and Concurrency Notes

Observed from repo inspection:

- `Issue.save()` serializes sequence ID creation per project with a Postgres advisory lock.
- Normal issue updates are last-write-wins.
- Assignee updates replace the full assignee set.
- Label updates replace the full label set.
- Cycle/module membership changes can be last-write-wins.
- No optimistic locking/version check was found for issue/state/module mutations.

Agent client rules:

1. Read latest work item before patching.
2. Patch only fields the agent owns when possible.
3. For set fields like assignees/labels, merge current values intentionally.
4. Store every run/action in the AI app DB.
5. Make all webhook handling idempotent.
6. Avoid running two agents on the same work item simultaneously.

## Webhook Receiver Requirements

The AI app should expose:

```http
POST /webhooks/plane
```

Receiver should:

- verify Plane webhook signature if `X-Plane-Signature` and secret are configured
- parse event type/action
- quickly persist event
- return 2xx fast
- process asynchronously in worker

Pseudo-code:

```rust
async fn plane_webhook_handler(headers, body) -> Result<StatusCode> {
    verify_signature(headers, body)?;
    let event = parse_event(body)?;
    save_webhook_event(event).await?;
    enqueue_processing(event.id).await?;
    Ok(StatusCode::NO_CONTENT)
}
```

## Rust Implementation Recommendation

Use Rust for the separate AI Agent app.

Suggested stack:

- HTTP server: `axum`
- HTTP client: `reqwest`
- async runtime: `tokio`
- JSON/types: `serde`
- database: `sqlx` + Postgres
- queue: Postgres-backed jobs, Redis, or a dedicated queue
- logs/tracing: `tracing`
- secrets: environment/secret manager

Suggested modules:

```text
src/
  main.rs
  config.rs
  plane/
    client.rs
    types.rs
    webhooks.rs
  agent/
    trigger.rs
    runner.rs
    tools.rs
    prompts.rs
  db/
    runs.rs
    webhook_events.rs
  http/
    routes.rs
  workers/
    webhook_processor.rs
    run_executor.rs
```

## Plane Client Shape

```rust
pub struct PlaneClient {
    base_url: Url,
    api_key: String,
    http: reqwest::Client,
}

impl PlaneClient {
    pub async fn list_states(&self, workspace: &str, project_id: Uuid) -> Result<Vec<State>>;
    pub async fn get_work_item(&self, workspace: &str, project_id: Uuid, item_id: Uuid) -> Result<WorkItem>;
    pub async fn patch_work_item(&self, workspace: &str, project_id: Uuid, item_id: Uuid, patch: WorkItemPatch) -> Result<WorkItem>;
    pub async fn create_comment(&self, workspace: &str, project_id: Uuid, item_id: Uuid, html: &str) -> Result<Comment>;
}
```

All requests should include:

```http
X-Api-Key: plane_api_xxx
Content-Type: application/json
```

## Configuration

Example config:

```toml
[plane]
base_url = "https://plane.example.com"
workspace_slug = "my-workspace"
api_key_env = "PLANE_API_KEY"
agent_user_id = "..."
agent_mentions = ["@agent", "@ai"]

[triggers]
mentions = true
labels = false
assignment = false
state = false

[workflow]
in_progress_state_name = "In Progress"
review_state_name = "In Review"
done_state_name = "Done"
working_label_name = "agent-working"
ready_label_name = "agent-ready"
```

## Safety Rules

Default safety behavior:

- Only act on explicit mention.
- Ignore comments authored by the agent user.
- Ignore archived work items.
- Ignore deleted work items.
- Ignore projects not allowlisted.
- Do not auto-close cards unless configured.
- Always comment before and after work.
- Keep an audit trail in the AI app DB.
- Prefer moving to Review instead of Done for code-writing tasks.

## Suggested MVP

Build in this order:

1. Plane API client.
2. Webhook receiver.
3. Webhook persistence/idempotency.
4. Mention parser.
5. Fetch latest work item.
6. Agent run claim table.
7. Comment: accepted/started.
8. Simple fake worker that comments a dry-run response.
9. Real AI worker.
10. Optional state transition to In Progress/Review.
11. Optional label/assignment triggers.
12. Admin UI or config file for allowed projects and trigger settings.

## Manual Test Scenario

1. Create a Plane project.
2. Add the dedicated agent user as Member.
3. Generate API token for agent user.
4. Register webhook URL in Plane workspace settings.
5. Create a card.
6. Add a comment:

```text
@agent summarize the task and propose next steps
```

7. Verify AI app receives webhook.
8. Verify AI app creates one `agent_runs` row.
9. Verify AI app comments “accepted”.
10. Verify duplicate webhook/comment does not create duplicate run.
11. Verify agent result comment appears on the same card.
12. Verify card state changes only if configured.

## Open Questions for Next Session

Decide before implementation:

1. Will the agent use one shared Plane bot user or per-human delegated tokens?
2. Should `@agent` immediately execute, or ask for confirmation first?
3. Should successful work move cards to Review or Done?
4. Should the agent be allowed to assign itself?
5. Which projects/workspaces are allowlisted?
6. What LLM/tool runtime will the agent use?
7. Where should agent run logs/artifacts be stored?

## Final Recommendation

Implement the new app as a separate Rust service. Use Plane webhooks to detect explicit human intent, especially `@agent` mentions. Use Plane `/api/v1/` with `X-Api-Key` to read cards, comment, assign/update fields, and move cards. Store claims/runs in the AI app database to prevent duplicate or unwanted work.
