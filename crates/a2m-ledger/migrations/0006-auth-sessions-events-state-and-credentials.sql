-- owner: a2m-auth
-- Sessions, their timeline, their saved state, and stored credentials (spec
-- sections 215 and 216). The session keeper owns these tables and is the only
-- reader and writer. Nothing here is in the clear that could sign anyone in:
-- saved state and credentials are envelopes sealed by the vault under their
-- owner's key.

-- One row per session: one identity on one target in one environment of one
-- workspace. Every change of `state` is one UPDATE that names the state and
-- the epoch it expects, so that two keepers on one store cannot both start a
-- renewal. `epoch` increases whenever the credentials are replaced or ended.
CREATE TABLE sessions (
    id TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL,
    target TEXT NOT NULL,
    environment TEXT NOT NULL,
    identity_ref TEXT NOT NULL,
    owner_principal_id TEXT,
    provider TEXT NOT NULL,
    state TEXT NOT NULL CHECK (state IN (
        'absent', 'signing_in', 'valid', 'suspect', 'renewing', 'needs_person', 'expired',
        'revoked'
    )),
    epoch INTEGER NOT NULL,
    home_runner_id TEXT,
    movable INTEGER NOT NULL CHECK (movable IN (0, 1)),
    profile TEXT,
    state_since INTEGER NOT NULL,
    signed_in_at INTEGER,
    expires_at INTEGER,
    checked_at INTEGER,
    last_used_at INTEGER,
    stored_attempts INTEGER NOT NULL,
    wants TEXT,
    UNIQUE (workspace_id, target, environment, identity_ref)
) STRICT;

-- The session timeline: what happened, when, and why, as fixed words. No
-- cookie, token, or password is ever written here.
CREATE TABLE session_events (
    seq INTEGER PRIMARY KEY,
    session_id TEXT NOT NULL,
    at INTEGER NOT NULL,
    kind TEXT NOT NULL,
    state TEXT NOT NULL,
    epoch INTEGER NOT NULL,
    cause TEXT NOT NULL,
    run_id TEXT,
    decision_id TEXT
) STRICT;

CREATE INDEX session_events_by_session ON session_events (session_id, seq);

-- A movable session's saved state, sealed under the owner's key. Written in
-- the same transaction as the change of state it belongs to.
CREATE TABLE session_state (
    session_id TEXT PRIMARY KEY,
    sealed BLOB NOT NULL,
    saved_at INTEGER NOT NULL
) STRICT;

-- Stored credentials, by the reference an auth file names them with, each
-- sealed under the key of the identity it belongs to.
CREATE TABLE session_credentials (
    workspace_id TEXT NOT NULL,
    owner TEXT NOT NULL,
    reference TEXT NOT NULL,
    sealed BLOB NOT NULL,
    stored_at INTEGER NOT NULL,
    PRIMARY KEY (workspace_id, owner, reference)
) STRICT;
