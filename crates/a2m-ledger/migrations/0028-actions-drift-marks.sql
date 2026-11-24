-- owner: a2m-actions
-- The drift a canary or a run found (spec section 217, decision 0380): the
-- knowledge that no longer fits the system, held until a canary's pass
-- through it vouches for it again. Written by `a2m-actions` when the engine
-- reports a binding's health, and read by the runner when it fills a job,
-- so that every binding the knowledge stops is refused before anything is
-- sent. Moments are milliseconds since 1970, from the store's clock.

-- `knowledge` is `entry`, one entry of a pack section, held by the digest
-- of the whole section (`digest`) and the entry's name (`entry`); or
-- `binding`, a binding whose drift no step named, held by the binding's
-- digest, with `entry` empty. A mark is of the target, not of a version:
-- a version that holds the same knowledge is stopped by it, and one that
-- changed it is not. `operation` and `run_id` say which binding drifted and
-- in which run; `code` is the drift's failure code. Never a value.
CREATE TABLE actions_drift_marks (
    workspace_id TEXT NOT NULL,
    target TEXT NOT NULL,
    surface TEXT NOT NULL,
    mode TEXT NOT NULL,
    knowledge TEXT NOT NULL CHECK (knowledge IN ('entry', 'binding')),
    digest TEXT NOT NULL,
    entry TEXT NOT NULL,
    operation TEXT NOT NULL,
    code TEXT NOT NULL,
    run_id TEXT NOT NULL,
    found_at INTEGER NOT NULL,
    PRIMARY KEY (workspace_id, target, surface, mode, knowledge, digest, entry)
) STRICT;
