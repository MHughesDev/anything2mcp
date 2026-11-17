-- owner: a2m-ledger
-- The vault's wrapped keys and the records of evidence blobs (spec sections
-- 208 and 216). A key row holds what the vault handed over, already wrapped.

CREATE TABLE vault_keys (
    key_id TEXT NOT NULL,
    generation INTEGER NOT NULL,
    layer TEXT NOT NULL CHECK (layer IN ('workspace', 'owner', 'digest')),
    wrapped BLOB NOT NULL,
    wrapped_by_id TEXT NOT NULL,
    wrapped_by_generation INTEGER NOT NULL,
    PRIMARY KEY (key_id, generation)
) STRICT;

CREATE TRIGGER vault_keys_are_kept
BEFORE DELETE ON vault_keys
BEGIN
    SELECT RAISE(ABORT, 'a2m: a wrapped key is never removed');
END;

-- `hash` is the workspace's keyed digest of the plain content, never its
-- plain hash. `sealed` holds the envelope when the blob is kept inline.
CREATE TABLE blobs (
    hash TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL,
    owner_kind TEXT NOT NULL CHECK (owner_kind IN ('person', 'service')),
    owner_id TEXT NOT NULL,
    kind TEXT NOT NULL CHECK (kind IN ('values', 'diff', 'snapshot', 'trace', 'screenshot', 'input', 'result')),
    size INTEGER NOT NULL,
    location TEXT NOT NULL CHECK (location IN ('inline', 'file')),
    sealed BLOB,
    key_ref TEXT NOT NULL,
    retention_class TEXT NOT NULL CHECK (retention_class IN ('short', 'long', 'longest')),
    created_at INTEGER NOT NULL,
    CHECK ((location = 'inline') = (sealed IS NOT NULL))
) STRICT;
