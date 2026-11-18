-- owner: a2m-ledger
-- A caller's idempotency key, bound to the run the call started (spec
-- section 208, decision 0160). The same caller making the same call with
-- the same key is given that run back, unfinished or finished, for as long
-- as the run is kept; the same key with another call is refused.
--
-- `key_digest` is the workspace's keyed digest of the key as the caller
-- gave it, never the key itself: a key is the caller's own text and may
-- hold anything. `request_hash` is the digest of what the call asked for:
-- the tool, the environment, and the input with its source. `run_id` is
-- empty from the moment the key is held, before the run is admitted, until
-- the run is bound to it; a key with no run is one whose call admitted
-- nothing yet or stopped in between, and the caller that holds it next
-- looks for that run before it starts another.
CREATE TABLE caller_idempotency (
    workspace_id TEXT NOT NULL,
    principal_id TEXT NOT NULL,
    key_digest TEXT NOT NULL,
    request_hash TEXT NOT NULL,
    run_id TEXT REFERENCES runs (id) ON DELETE CASCADE,
    held_at INTEGER NOT NULL,
    PRIMARY KEY (workspace_id, principal_id, key_digest)
) STRICT;

CREATE INDEX caller_idempotency_by_run ON caller_idempotency (run_id);

-- A key once bound names its run, and its request, for as long as the run
-- is kept.
CREATE TRIGGER caller_idempotency_bound_once
BEFORE UPDATE ON caller_idempotency
WHEN OLD.run_id IS NOT NULL
    AND (NEW.run_id IS NOT OLD.run_id
        OR NEW.request_hash IS NOT OLD.request_hash
        OR NEW.workspace_id IS NOT OLD.workspace_id
        OR NEW.principal_id IS NOT OLD.principal_id
        OR NEW.key_digest IS NOT OLD.key_digest)
BEGIN
    SELECT RAISE(ABORT, 'a2m: an idempotency key once bound names its run while the run is kept');
END;

-- A bound key leaves only with its run, when retention purges it.
CREATE TRIGGER caller_idempotency_leaves_with_its_run
BEFORE DELETE ON caller_idempotency
WHEN OLD.run_id IS NOT NULL
    AND EXISTS (SELECT 1 FROM runs WHERE id = OLD.run_id AND retention_state IS NOT 'purging')
BEGIN
    SELECT RAISE(ABORT, 'a2m: a bound idempotency key leaves only with its run');
END;
