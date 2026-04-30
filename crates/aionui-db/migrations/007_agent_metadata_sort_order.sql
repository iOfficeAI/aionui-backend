-- Add sort_order column to agent_metadata for deterministic display ordering.
--
-- The value is a plain integer: smaller = higher in the list. The scheme
-- reserves ranges by agent_source so future custom / extension rows slot
-- in naturally:
--
--   100–199   internal   (aionrs = 100)
--   1000–1999 custom     (future)
--   2000–2999 extension  (future)
--   3000–3999 builtin    (ACP vendors, openclaw, nanobot)
--
-- Within builtin ACP, backend priority:
--   claude = 3100, codex = 3110, gemini = 3120, others = 3130
-- Special non-ACP builtins:
--   openclaw = 3900, nanobot = 3990

ALTER TABLE agent_metadata ADD COLUMN sort_order INTEGER NOT NULL DEFAULT 1000;

-- ── Backfill sort_order for seed rows ────────────────────────────────

-- internal: aionrs
UPDATE agent_metadata SET sort_order = 100 WHERE agent_type = 'aionrs' AND agent_source = 'internal';

-- builtin ACP: claude highest, then codex, gemini, rest
UPDATE agent_metadata SET sort_order = 3100 WHERE agent_source = 'builtin' AND agent_type = 'acp' AND backend = 'claude';
UPDATE agent_metadata SET sort_order = 3110 WHERE agent_source = 'builtin' AND agent_type = 'acp' AND backend = 'codex';
UPDATE agent_metadata SET sort_order = 3120 WHERE agent_source = 'builtin' AND agent_type = 'acp' AND backend = 'gemini';
UPDATE agent_metadata SET sort_order = 3130 WHERE agent_source = 'builtin' AND agent_type = 'acp' AND backend NOT IN ('claude', 'codex', 'gemini');

-- ── Fix agent_source for openclaw and nanobot ────────────────────────
-- These were incorrectly seeded as 'internal'; they should be 'builtin'.

UPDATE agent_metadata SET agent_source = 'builtin' WHERE agent_type = 'openclaw-gateway';
UPDATE agent_metadata SET agent_source = 'builtin' WHERE agent_type = 'nanobot';

-- Now assign their sort_order (after source correction).
UPDATE agent_metadata SET sort_order = 3900 WHERE agent_type = 'openclaw-gateway' AND agent_source = 'builtin';
UPDATE agent_metadata SET sort_order = 3990 WHERE agent_type = 'nanobot' AND agent_source = 'builtin';

CREATE INDEX IF NOT EXISTS idx_agent_metadata_sort_order ON agent_metadata(sort_order);
