# Plane AI manual E2E testing

## What this app does
- Receives Plane `issue_comment` webhooks.
- Looks for an explicit mention such as `@agent`.
- Validates project allowlist, project control approval, acceptance criteria, and active-run rules.
- Creates an agent run in its own Postgres DB.
- Runs either:
  - fake mode for safe testing, or
  - `omp -p <prompt>` in subprocess mode.

## Important MVP constraint
This MVP does **not** read Plane Pages automatically.

If you create a Plane Page with TOR / project details, the app will **not** consume it by itself.
You must also create the project control row through this app's API:

- `PUT /api/projects/{project_id}/control`

## Recommended first test path
Start with `RUNNER_MODE=fake`.
Once that works end to end, switch to `RUNNER_MODE=subprocess` for real `omp` execution.

---

## 1. Prerequisites
You already have:
- Plane self-hosted in Docker
- Plane reachable at `http://localhost`
- A Plane account
- A workspace
- A project named `Todo`

You also need:
- local Postgres for this Rust app
- a Plane API key
- the Plane workspace slug
- the Plane project UUID
- the Plane agent user UUID

---

## 2. Create a Postgres database for this app
Example:

```bash
createdb plane_ai_dev
```

Then use:

```bash
export DATABASE_URL=postgres://YOURUSER@localhost/plane_ai_dev
```

---

## 3. Get the Plane values you need

### 3.1 Plane API key
Create or copy an API key from the Plane account you want the app to act as.

This app sends all Plane API requests with:

```http
X-Api-Key: <token>
```

### 3.2 Workspace slug
Use your Plane workspace slug, for example:

```bash
export WORKSPACE_SLUG=your-workspace-slug
```

### 3.3 Project UUID
List projects:

```bash
export PLANE_API_KEY=YOUR_PLANE_API_KEY
curl -s "http://localhost/api/v1/workspaces/$WORKSPACE_SLUG/projects/" \
  -H "X-Api-Key: $PLANE_API_KEY"
```

Find the `Todo` project and copy its `id`.

```bash
export PROJECT_UUID=YOUR_PROJECT_UUID
```

### 3.4 Agent user UUID
List project members:

```bash
curl -s "http://localhost/api/v1/workspaces/$WORKSPACE_SLUG/projects/$PROJECT_UUID/members/" \
  -H "X-Api-Key: $PLANE_API_KEY"
```

Choose the user account that should author app comments and act as the bot.

```bash
export AGENT_USER_ID=YOUR_AGENT_USER_UUID
```

---

## 4. Start the Rust app in fake mode
Plane is already on port 80, so run this app on port 3000:

```bash
export DATABASE_URL=postgres://YOURUSER@localhost/plane_ai_dev
export PLANE_BASE_URL=http://localhost
export PLANE_API_KEY=YOUR_PLANE_API_KEY
export PLANE_WEBHOOK_SECRET=test-secret
export PLANE_WORKSPACE_SLUG=$WORKSPACE_SLUG
export ALLOWED_PROJECT_IDS=$PROJECT_UUID
export AGENT_USER_ID=$AGENT_USER_ID
export RUNNER_MODE=fake
export APP_BIND_ADDR=0.0.0.0:3000

cargo run
```

Health check:

```bash
curl -i http://localhost:3000/health
```

Expected:
- `204 No Content`

---

## 5. Create project control in this app
This step is required before `@agent` can start work.

```bash
curl -X PUT "http://localhost:3000/api/projects/$PROJECT_UUID/control" \
  -H "content-type: application/json" \
  -d '{
    "plane_workspace_slug": "'"$WORKSPACE_SLUG"'",
    "tor_markdown": "# TOR / Project Control\n\n## Scope\n- Manual E2E test for Plane AI",
    "approved_scope": {
      "acceptance_criteria": [
        "Agent accepts @agent trigger",
        "Agent posts result comment",
        "Card moves to Human Review"
      ]
    },
    "budget_man_days": 1,
    "billing_rate_per_day": 1000,
    "internal_cost_rate_per_day": 500,
    "human_reviewer_id": null,
    "brief_status": "approved"
  }'
```

Verify:

```bash
curl "http://localhost:3000/api/projects/$PROJECT_UUID/control"
```

---

## 6. Register the Plane webhook
Because Plane runs inside Docker and this app runs on the host, use:

```text
http://host.docker.internal:3000/webhooks/plane
```

Do **not** use `http://localhost:3000/webhooks/plane` inside the Plane webhook UI unless your container networking is confirmed to support that path.

Webhook settings:
- URL: `http://host.docker.internal:3000/webhooks/plane`
- Secret: `test-secret`
- Event: `issue_comment`

---

## 7. Prepare a Plane work item
Create or pick a work item in project `Todo`.

To satisfy the app's policy, provide acceptance criteria in one of two ways:

### Option A: in the Plane work item description
Example:

```text
Acceptance criteria
- agent accepts the task
- agent posts a completion comment
- card moves to Human Review
```

### Option B: in `approved_scope.acceptance_criteria`
Already done in step 5.

---

## 8. Trigger the agent
Add a comment on the Plane work item:

```text
@agent summarize the task and propose next steps
```

What the app checks before running:
- event is `issue_comment`
- action is `create` or `update`
- comment contains a configured mention like `@agent`
- comment is not authored by `AGENT_USER_ID`
- project is in `ALLOWED_PROJECT_IDS`
- work item is not archived
- project control exists and has `brief_status = "approved"`
- acceptance criteria exist
- no active run exists for that work item

---

## 9. Expected result in fake mode
If all checks pass, Plane should show:

1. acceptance comment:

```html
<p>Agent accepted this task.</p><p>Run: ...</p>
```

2. completion comment:

```html
<p>Agent completed implementation.</p>
...
<p>Needs human review before Done.</p>
```

3. state updates:
- moves to `In Progress` if that state exists
- then moves to `Human Review` if that state exists

4. label updates:
- adds `agent-running` if it exists
- removes `agent-running` after completion
- adds `agent-review-ready` if it exists

Missing labels or states do **not** fail the run.
The app records missing labels/states as run events instead.

---

## 10. Inspect the run
JSON endpoints:

```bash
curl http://localhost:3000/runs
curl http://localhost:3000/runs/RUN_UUID
```

HTML UI:
- `http://localhost:3000/ui/runs`
- `http://localhost:3000/ui/runs/RUN_UUID`

---

## 11. Expected blocker behavior

### Missing project control
Plane should get:

```html
<p>Agent needs approved project control before starting. Missing: approved TOR / Project Control for this project.</p>
```

### Missing acceptance criteria
Plane should get:

```html
<p>Agent needs acceptance criteria before starting this work item.</p>
```

### Duplicate active run
Plane should get:

```html
<p>Agent is already running this task. Existing run: RUN_UUID</p>
```

---

## 12. Switch to real OMP execution
After fake mode works, stop the app and restart with:

```bash
export RUNNER_MODE=subprocess
export AGENT_COMMAND=omp
cargo run
```

This app will execute:

```text
omp -p <generated prompt>
```

Requirements:
- `omp` installed
- `omp` available on `PATH`

Optional args before `-p`:

```bash
export AGENT_COMMAND_ARGS=--some-arg,--another-arg
```

---

## 13. Troubleshooting

### Nothing happens after comment
Check:
- webhook URL reachable from Plane container
- webhook event includes `issue_comment`
- comment contains `@agent`
- project UUID is in `ALLOWED_PROJECT_IDS`
- comment author is not `AGENT_USER_ID`

### Plane cannot reach the Rust app
Try:
- `http://host.docker.internal:3000/webhooks/plane`

### App comments project control blocker
You forgot step 5 or `brief_status` is not `approved`.

### App comments acceptance criteria blocker
Add `Acceptance criteria` text to the work item or put a non-empty `approved_scope.acceptance_criteria` array in project control.

### Real OMP mode fails immediately
Check:
- `which omp`
- shell `PATH`
- local `omp -p "test"` works outside the app

---

## 14. Recommended workflow for playing around
1. Start in `RUNNER_MODE=fake`
2. Create project control with `brief_status = approved`
3. Register webhook to `host.docker.internal:3000/webhooks/plane`
4. Trigger with `@agent ...`
5. Inspect `/ui/runs`
6. Try blocker cases
7. Switch to `RUNNER_MODE=subprocess`
8. Trigger again with real OMP
