-- owner: a2m-sched
-- What the scheduler keeps (spec sections 211 and 223): jobs and their
-- leases, runners, who holds each exclusive resource, whose turn it is,
-- scopes on hold, and rate budgets. Written by `a2m-sched`, which is the
-- only crate that reads or writes these. Moments are milliseconds since
-- 1970, from the store's clock.

-- One piece of work handed to one runner under a lease. `seq` is the order
-- of arrival. `requirements` is the job's needs as JSON, as it was
-- submitted. `epoch` is the lease number the runner was given; for a job of
-- a run it is the run's lease number at the moment of the lease.
-- `directive` is what the next heartbeat tells the runner. `end_state`,
-- `end_class`, and `end_code` say how the job ended.
CREATE TABLE jobs (
    id TEXT PRIMARY KEY,
    seq INTEGER NOT NULL UNIQUE,
    run_id TEXT,
    workspace_id TEXT NOT NULL,
    principal_id TEXT NOT NULL,
    kind TEXT NOT NULL CHECK (kind IN (
        'preview', 'execute', 'try', 'discover', 'reconcile', 'verify_after_loss', 'canary',
        'session_keepalive')),
    target TEXT NOT NULL,
    environment TEXT NOT NULL,
    requirements TEXT NOT NULL,
    session_key TEXT,
    home_runner_id TEXT,
    writes INTEGER NOT NULL CHECK (writes IN (0, 1)),
    priority INTEGER NOT NULL CHECK (priority IN (0, 1)),
    available_at INTEGER NOT NULL,
    queue_until INTEGER NOT NULL,
    state TEXT NOT NULL CHECK (state IN ('pending', 'leased', 'done', 'lost', 'dead')),
    epoch INTEGER NOT NULL DEFAULT 0,
    runner_id TEXT,
    lease_mode TEXT,
    lease_until INTEGER,
    heartbeat_at INTEGER,
    attempts INTEGER NOT NULL DEFAULT 0,
    directive TEXT NOT NULL DEFAULT 'continue' CHECK (directive IN ('continue', 'cancel')),
    end_state TEXT,
    end_class TEXT,
    end_code TEXT,
    created_at INTEGER NOT NULL,
    finished_at INTEGER,
    CHECK ((state = 'leased') = (runner_id IS NOT NULL AND lease_until IS NOT NULL))
) STRICT;

CREATE INDEX jobs_pending ON jobs (priority, seq) WHERE state = 'pending';
CREATE INDEX jobs_by_runner ON jobs (runner_id, state);
CREATE INDEX jobs_by_session ON jobs (session_key) WHERE state = 'leased';

-- At most one job of a run is waiting or leased at any moment, so a run is
-- never worked on by two runners at once.
CREATE UNIQUE INDEX jobs_one_live_per_run ON jobs (run_id)
WHERE run_id IS NOT NULL AND state IN ('pending', 'leased');

-- A lease number only grows, and a job that has ended never changes again.
CREATE TRIGGER jobs_lease_only_grows
BEFORE UPDATE OF epoch ON jobs
WHEN NEW.epoch < OLD.epoch
BEGIN
    SELECT RAISE(ABORT, 'a2m: a lease number only grows');
END;

CREATE TRIGGER jobs_ended_means_ended
BEFORE UPDATE ON jobs
WHEN OLD.state IN ('done', 'lost', 'dead')
BEGIN
    SELECT RAISE(ABORT, 'a2m: a job that has ended never changes');
END;

-- A runner and what it offers, as JSON: targets, modes, operating system,
-- and the person it works for, when it is a person's own device.
CREATE TABLE runners (
    id TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL,
    owner_principal_id TEXT,
    capabilities TEXT NOT NULL,
    protocol_version INTEGER NOT NULL,
    state TEXT NOT NULL CHECK (state IN ('enrolled', 'online', 'draining', 'offline', 'revoked')),
    last_seen_at INTEGER NOT NULL
) STRICT;

-- Who holds each exclusive resource. The primary key is the rule: one holder
-- for one key.
CREATE TABLE resource_holds (
    resource_key TEXT PRIMARY KEY,
    job_id TEXT NOT NULL,
    acquired_at INTEGER NOT NULL
) STRICT;

CREATE INDEX resource_holds_by_job ON resource_holds (job_id);

-- Round robin between callers: `served` is the number of the dispatch that
-- last gave this caller a job. The caller with the lowest number is next.
CREATE TABLE caller_turns (
    workspace_id TEXT NOT NULL,
    principal_id TEXT NOT NULL,
    served INTEGER NOT NULL,
    PRIMARY KEY (workspace_id, principal_id)
) STRICT;

-- Scopes no job is handed out for. `scope` is `everything`, `workspace`,
-- `target`, or `caller`; `workspace_id` and `name` are empty where the scope
-- does not name them.
CREATE TABLE scheduler_holds (
    scope TEXT NOT NULL CHECK (scope IN ('everything', 'workspace', 'target', 'caller')),
    workspace_id TEXT NOT NULL,
    name TEXT NOT NULL,
    held_at INTEGER NOT NULL,
    PRIMARY KEY (scope, workspace_id, name)
) STRICT;

-- Token buckets. A bucket holds at most `capacity` requests and gains
-- `refill` requests every `per_ms` milliseconds. `level` is what it holds
-- now, in requests times `per_ms`, so that no fraction is ever lost;
-- `updated_at` is the moment `level` was true.
CREATE TABLE budgets (
    budget_key TEXT PRIMARY KEY,
    capacity INTEGER NOT NULL CHECK (capacity > 0),
    refill INTEGER NOT NULL CHECK (refill > 0),
    per_ms INTEGER NOT NULL CHECK (per_ms > 0),
    level INTEGER NOT NULL CHECK (level >= 0),
    updated_at INTEGER NOT NULL
) STRICT;
