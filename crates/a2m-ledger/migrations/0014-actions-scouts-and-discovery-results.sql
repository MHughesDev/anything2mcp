-- owner: a2m-actions
-- What the scout keeps (spec section 402, decisions 0170 and 0171): one row
-- per scout, beside the results of its stages in `actions_scout_stages`, and
-- what each discoverer of a discovery run reported. Written by
-- `a2m-actions`, which is the only crate that reads or writes these.
-- Moments are milliseconds since 1970, from the store's clock.

-- One scout of one target by one person. `scout_id` is the identifier its
-- stages are stored under in `actions_scout_stages` (column `run_id`). A
-- scout with no `finished_at` is resumed by the next `scout.run` of the same
-- target by the same person. `report_hash` is the hash of its report once
-- the last stage is stored.
CREATE TABLE actions_scouts (
    scout_id TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL,
    principal_id TEXT NOT NULL,
    target TEXT NOT NULL,
    started_at INTEGER NOT NULL,
    finished_at INTEGER,
    report_hash TEXT
) STRICT;

CREATE INDEX actions_scouts_by_target
    ON actions_scouts (workspace_id, principal_id, target, started_at);

-- What one discoverer reported in one discovery run, as the engine sent it:
-- the surface, the findings it made by identifier (a JSON list), and how it
-- ended (JSON: coverage, the bounds it reached, how many things it dropped,
-- or the class and code of its failure). Never a value of the system: a
-- finding is stored in `findings`, already redacted.
CREATE TABLE actions_discovery_results (
    run_id TEXT NOT NULL,
    provider TEXT NOT NULL,
    discoverer TEXT NOT NULL,
    surface TEXT NOT NULL,
    findings TEXT NOT NULL,
    report TEXT NOT NULL,
    sent_at INTEGER NOT NULL,
    PRIMARY KEY (run_id, provider, discoverer)
) STRICT;
