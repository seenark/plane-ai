CREATE EXTENSION IF NOT EXISTS pgcrypto;

CREATE TABLE plane_webhook_events (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    plane_delivery_id TEXT UNIQUE,
    plane_event TEXT NOT NULL,
    plane_action TEXT NOT NULL,
    plane_webhook_id UUID,
    plane_workspace_id UUID NOT NULL,
    plane_workspace_slug TEXT NOT NULL,
    plane_project_id UUID,
    plane_work_item_id UUID,
    plane_comment_id UUID,
    plane_actor_id UUID,
    payload JSONB NOT NULL,
    processing_status TEXT NOT NULL DEFAULT 'received' CHECK (processing_status IN ('received', 'ignored', 'queued', 'processed', 'failed')),
    received_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    processed_at TIMESTAMPTZ,
    error_message TEXT
);

CREATE UNIQUE INDEX plane_webhook_events_comment_event_unique
    ON plane_webhook_events (plane_event, plane_action, plane_comment_id)
    WHERE plane_comment_id IS NOT NULL;

CREATE TABLE agent_runs (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    plane_workspace_slug TEXT NOT NULL,
    plane_project_id UUID NOT NULL,
    plane_work_item_id UUID NOT NULL,
    trigger_comment_id UUID,
    trigger_user_id UUID,
    status TEXT NOT NULL CHECK (status IN ('queued', 'running', 'succeeded', 'failed', 'cancelled', 'ignored', 'waiting_for_user')),
    prompt TEXT,
    final_response TEXT,
    started_at TIMESTAMPTZ,
    finished_at TIMESTAMPTZ,
    exit_code INT,
    error_message TEXT,
    repo_url TEXT,
    branch_name TEXT,
    commit_sha TEXT,
    pr_url TEXT,
    runner_mode TEXT NOT NULL,
    runner_command TEXT,
    stdout TEXT,
    stderr TEXT,
    llm_cost NUMERIC(12,4) NOT NULL DEFAULT 0,
    agent_runtime_seconds INT NOT NULL DEFAULT 0,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE UNIQUE INDEX agent_runs_one_active_per_work_item
    ON agent_runs (plane_work_item_id)
    WHERE status IN ('queued', 'running', 'waiting_for_user');

CREATE TABLE agent_run_events (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    run_id UUID NOT NULL REFERENCES agent_runs(id) ON DELETE CASCADE,
    event_type TEXT NOT NULL,
    payload JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE agent_run_artifacts (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    run_id UUID NOT NULL REFERENCES agent_runs(id) ON DELETE CASCADE,
    artifact_type TEXT NOT NULL,
    name TEXT NOT NULL,
    content TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE agent_approvals (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    run_id UUID NOT NULL REFERENCES agent_runs(id) ON DELETE CASCADE,
    approval_type TEXT NOT NULL CHECK (approval_type IN ('brief_approval', 'work_breakdown_approval', 'scope_change_approval', 'project_info_update', 'budget_update', 'mark_done', 'bulk_card_create')),
    requested_payload JSONB NOT NULL DEFAULT '{}'::jsonb,
    plane_work_item_id UUID NOT NULL,
    plane_comment_id UUID,
    status TEXT NOT NULL DEFAULT 'requested' CHECK (status IN ('requested', 'approved', 'rejected', 'cancelled')),
    requested_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    resolved_at TIMESTAMPTZ,
    resolved_by UUID
);

CREATE TABLE project_controls (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    plane_workspace_slug TEXT NOT NULL,
    plane_project_id UUID NOT NULL UNIQUE,
    source TEXT NOT NULL CHECK (source IN ('automation_db', 'plane_page_unavailable')),
    tor_markdown TEXT NOT NULL,
    approved_scope JSONB NOT NULL DEFAULT '{}'::jsonb,
    budget_man_days NUMERIC(10,2),
    billing_rate_per_day NUMERIC(12,2),
    internal_cost_rate_per_day NUMERIC(12,2),
    human_reviewer_id UUID,
    brief_status TEXT NOT NULL DEFAULT 'unknown' CHECK (brief_status IN ('unknown', 'pending', 'approved', 'rejected')),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE scope_change_logs (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    run_id UUID NOT NULL REFERENCES agent_runs(id) ON DELETE CASCADE,
    classification TEXT NOT NULL CHECK (classification IN ('none', 'clarification', 'minor_scope_change', 'major_scope_change', 'out_of_scope', 'budget_risk', 'timeline_risk')),
    original_scope TEXT,
    new_request TEXT,
    estimated_impact TEXT,
    decision TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE agent_costs (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    run_id UUID NOT NULL UNIQUE REFERENCES agent_runs(id) ON DELETE CASCADE,
    planned_man_days NUMERIC(10,2),
    actual_agent_runtime_seconds INT NOT NULL DEFAULT 0,
    llm_cost NUMERIC(12,4) NOT NULL DEFAULT 0,
    internal_cost NUMERIC(12,4) NOT NULL DEFAULT 0,
    billable_amount NUMERIC(12,4) NOT NULL DEFAULT 0,
    gross_margin NUMERIC(12,4) NOT NULL DEFAULT 0,
    gross_margin_percent NUMERIC(12,4) NOT NULL DEFAULT 0,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
