-- Migration 004: Update builtin bun-launched ACP package pins.
--
-- Keep these as exact versions rather than `latest` so one AionUi release
-- remains reproducible across installs and restarts.

-- Claude Code (2d23ff1c)
-- Source: @agentclientprotocol/claude-agent-acp latest published package.
UPDATE agent_metadata SET
    args       = '["x","--bun","@agentclientprotocol/claude-agent-acp@0.33.1"]',
    updated_at = CAST(strftime('%s','now') AS INTEGER) * 1000
WHERE id = '2d23ff1c'
  AND agent_source = 'builtin'
  AND backend = 'claude'
  AND command = 'bun'
  AND args != '["x","--bun","@agentclientprotocol/claude-agent-acp@0.33.1"]';

-- Codex CLI (8e1acf31)
-- Source: zed-industries/codex-acp latest release.
UPDATE agent_metadata SET
    args       = '["x","--bun","@zed-industries/codex-acp@0.14.0"]',
    updated_at = CAST(strftime('%s','now') AS INTEGER) * 1000
WHERE id = '8e1acf31'
  AND agent_source = 'builtin'
  AND backend = 'codex'
  AND command = 'bun'
  AND args != '["x","--bun","@zed-industries/codex-acp@0.14.0"]';

-- CodeBuddy (8b20fd41)
-- Source: ACP Registry entry for Codebuddy Code.
UPDATE agent_metadata SET
    args       = '["x","--bun","@tencent-ai/codebuddy-code@2.97.0","--acp"]',
    updated_at = CAST(strftime('%s','now') AS INTEGER) * 1000
WHERE id = '8b20fd41'
  AND agent_source = 'builtin'
  AND backend = 'codebuddy'
  AND command = 'bun'
  AND args != '["x","--bun","@tencent-ai/codebuddy-code@2.97.0","--acp"]';
