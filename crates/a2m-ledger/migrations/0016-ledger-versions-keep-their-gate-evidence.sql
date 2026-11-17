-- owner: a2m-ledger
-- A published version keeps the evidence of the publish gates it passed:
-- which gates ran, what each found, and which the documents name that did
-- not run (spec section 403, decision 0180). Canonical JSON, written once
-- with the version and never changed. A version stored before this has none.
ALTER TABLE target_versions ADD COLUMN gate_evidence TEXT;
