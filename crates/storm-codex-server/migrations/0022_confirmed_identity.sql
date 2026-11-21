-- Persisted "confirmed" accounts: a ToonHandle kept in "my identity" independent of whether its
-- replay folder stays configured (docs/specs/2026-11-21-standalone-confirmed-identity.md).
CREATE TABLE confirmed_identity (
    toon TEXT PRIMARY KEY,
    confirmed_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
