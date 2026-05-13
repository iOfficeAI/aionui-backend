-- Migration 003: Backfill handshake-derived columns for builtin ACP agents
-- and fix three seed rows whose spawn command never worked.
--
-- `agent_capabilities` and `auth_methods` are normally populated by the live
-- ACP `initialize` handshake and written through `registry::apply_handshake`.
-- Seed rows in migration 001 leave both columns NULL until that first spawn.
-- This migration pre-fills them from real handshake captures so the UI can
-- render auth buttons / capability badges without requiring a round-trip.
--
-- Values are snake_case JSON matching the same contract enforced by
-- `normalize_keys_to_snake_case` (crates/aionui-common/src/case_convert.rs):
-- every object key is converted camelCase -> snake_case, except `_meta`
-- whose inner keys are preserved verbatim as passthrough metadata.
--
-- Source of each JSON blob: `acp-client <command> [args...] initialize` run
-- against the CLI version pinned by the corresponding agent_metadata row.
-- Captured 2026-05-13 inside the `acp-fake-test` docker image.
--
-- `_meta.terminal-auth.command` values that the CLI emits as an absolute path
-- (e.g. `/root/.local/bin/kimi`, `/usr/local/lib/.../copilot`) are rewritten
-- to the bare command name so the stored JSON does not leak the probe host's
-- install layout. Call sites that resolve the terminal login command must
-- already honour PATH lookup — see `resolve_command_path` in aionui-runtime.
--
-- Agents not backfilled here (Snow, Cursor) have no known public install
-- channel for a Linux environment. Their rows stay NULL until a user
-- actually spawns the agent on a host that has the CLI available.

-- Qoder (1e4afc51) -- qodercli --acp
--
-- Fix command/binary_name: `@qoder-ai/qodercli` installs a single binary
-- named `qodercli`, not `qoder`. The previous values (`qoder` + binary_name
-- `qoder`) caused `probe_resolved_command` to mark this row unavailable on
-- every fresh install. Idempotent: only updated when the current value
-- still matches the broken seed.
UPDATE agent_metadata SET
    command           = 'qodercli',
    agent_source_info = json_set(COALESCE(agent_source_info, '{}'), '$.binary_name', 'qodercli'),
    updated_at        = CAST(strftime('%s','now') AS INTEGER) * 1000
WHERE id = '1e4afc51' AND command = 'qoder'
  AND json_extract(agent_source_info, '$.binary_name') = 'qoder';

UPDATE agent_metadata SET
    agent_capabilities = '{"load_session":true,"session_capabilities":{"list":{}},"prompt_capabilities":{"image":true,"audio":true,"embedded_context":true},"mcp_capabilities":{"http":true,"sse":true}}',
    auth_methods       = '[{"id":"qodercli-login","name":"Use qodercli login","description":"Use your existing qodercli login for this agent. If needed, sign in from qodercli first."},{"type":"env_var","id":"qoder-personal-access-token","name":"Use QODER_PERSONAL_ACCESS_TOKEN","description":"Requires `QODER_PERSONAL_ACCESS_TOKEN` in the agent environment.","vars":[{"name":"QODER_PERSONAL_ACCESS_TOKEN"}]}]',
    updated_at         = CAST(strftime('%s','now') AS INTEGER) * 1000
WHERE id = '1e4afc51' AND agent_capabilities IS NULL AND auth_methods IS NULL;

-- Qwen (26a946ed) -- qwen --acp
UPDATE agent_metadata SET
    agent_capabilities = '{"load_session":true,"prompt_capabilities":{"image":true,"audio":true,"embedded_context":true},"session_capabilities":{"list":{},"resume":{}},"mcp_capabilities":{"sse":true,"http":true}}',
    auth_methods       = '[{"id":"openai","name":"Use OpenAI API key","description":"Requires setting the `OPENAI_API_KEY` environment variable","_meta":{"type":"terminal","args":["--auth-type=openai"]}},{"id":"qwen-oauth","name":"Qwen OAuth","description":"Qwen OAuth (free tier discontinued 2026-04-15)","_meta":{"type":"terminal","args":["--auth-type=qwen-oauth"]}}]',
    updated_at         = CAST(strftime('%s','now') AS INTEGER) * 1000
WHERE id = '26a946ed' AND agent_capabilities IS NULL AND auth_methods IS NULL;

-- Copilot (3cd9d436) -- copilot --acp --stdio
UPDATE agent_metadata SET
    agent_capabilities = '{"load_session":true,"mcp_capabilities":{"http":true,"sse":true},"prompt_capabilities":{"image":true,"audio":false,"embedded_context":true},"session_capabilities":{"list":{}}}',
    auth_methods       = '[{"id":"copilot-login","name":"Log in with Copilot CLI","description":"Run `copilot login` in the terminal","_meta":{"terminal-auth":{"command":"copilot","args":["login"],"label":"Copilot Login"}}}]',
    updated_at         = CAST(strftime('%s','now') AS INTEGER) * 1000
WHERE id = '3cd9d436' AND agent_capabilities IS NULL AND auth_methods IS NULL;

-- Goose (600c6601) -- goose acp
UPDATE agent_metadata SET
    agent_capabilities = '{"load_session":true,"prompt_capabilities":{"image":true,"audio":false,"embedded_context":true},"mcp_capabilities":{"http":true,"sse":false},"session_capabilities":{"list":{},"close":{}},"auth":{}}',
    auth_methods       = '[{"id":"goose-provider","name":"Configure Provider","description":"Run `goose configure` to set up your AI provider and API key"}]',
    updated_at         = CAST(strftime('%s','now') AS INTEGER) * 1000
WHERE id = '600c6601' AND agent_capabilities IS NULL AND auth_methods IS NULL;

-- Vibe (65d0f5b2) -- vibe-acp
--
-- Fix command/args/binary_name: `vibe` is the interactive TUI and does not
-- speak ACP. The installer (`curl -LsSf https://mistral.ai/vibe/install.sh
-- | bash`) ships a dedicated `vibe-acp` binary that serves the ACP
-- endpoint. Idempotent: only updated when the row still carries the
-- broken seed values.
UPDATE agent_metadata SET
    command           = 'vibe-acp',
    args              = '[]',
    agent_source_info = json_set(COALESCE(agent_source_info, '{}'), '$.binary_name', 'vibe-acp'),
    updated_at        = CAST(strftime('%s','now') AS INTEGER) * 1000
WHERE id = '65d0f5b2' AND command = 'vibe' AND args = '[]'
  AND json_extract(agent_source_info, '$.binary_name') = 'vibe';

UPDATE agent_metadata SET
    agent_capabilities = '{"load_session":true,"prompt_capabilities":{"audio":false,"embedded_context":true,"image":false},"session_capabilities":{"close":{},"fork":{},"list":{}}}',
    auth_methods       = '[]',
    updated_at         = CAST(strftime('%s','now') AS INTEGER) * 1000
WHERE id = '65d0f5b2' AND agent_capabilities IS NULL AND auth_methods IS NULL;

-- CodeBuddy (8b20fd41) -- bun x --bun @tencent-ai/codebuddy-code@2.73.0 --acp
UPDATE agent_metadata SET
    agent_capabilities = '{"prompt_capabilities":{"image":true,"embedded_context":true},"mcp_capabilities":{"http":true,"sse":true},"load_session":true,"delegate_tools_support":true}',
    auth_methods       = '[{"id":"iOA","name":"Login with iOA","description":null},{"id":"external","name":"Login with Google/Github","description":null},{"id":"internal","name":"Login with WeChat","description":null},{"id":"selfhosted","name":"Login with Enterprise Domain","description":null}]',
    updated_at         = CAST(strftime('%s','now') AS INTEGER) * 1000
WHERE id = '8b20fd41' AND agent_capabilities IS NULL AND auth_methods IS NULL;

-- Droid (da386544) -- droid exec --output-format acp
UPDATE agent_metadata SET
    agent_capabilities = '{"load_session":true,"session_capabilities":{"list":{},"resume":{}},"prompt_capabilities":{"image":true,"embedded_context":true},"_meta":{"terminal_output":true,"terminal-auth":true}}',
    auth_methods       = '[{"id":"device-pairing","name":"Login","description":"Authenticate with Factory using a device pairing code in your browser."},{"id":"factory-api-key","name":"Factory API Key","description":"Authenticate using a Factory API key set in the FACTORY_API_KEY environment variable."}]',
    updated_at         = CAST(strftime('%s','now') AS INTEGER) * 1000
WHERE id = 'da386544' AND agent_capabilities IS NULL AND auth_methods IS NULL;

-- Kiro (e044000d) -- kiro-cli-chat acp
--
-- Fix command/binary_name: the installer (`curl -fsSL https://cli.kiro.dev/
-- install | bash`) ships three binaries -- `kiro-cli`, `kiro-cli-chat`,
-- `kiro-cli-term` -- and none of them are called `kiro`. `kiro-cli-chat
-- acp` is the entry point that serves ACP without requiring a prior
-- `kiro-cli login`, so unauthenticated users still get a proper
-- `initialize` response with the login `authMethod`. Idempotent: only
-- updated when the row still carries the broken seed values.
UPDATE agent_metadata SET
    command           = 'kiro-cli-chat',
    agent_source_info = json_set(COALESCE(agent_source_info, '{}'), '$.binary_name', 'kiro-cli-chat'),
    updated_at        = CAST(strftime('%s','now') AS INTEGER) * 1000
WHERE id = 'e044000d' AND command = 'kiro'
  AND json_extract(agent_source_info, '$.binary_name') = 'kiro';

UPDATE agent_metadata SET
    agent_capabilities = '{"load_session":true,"prompt_capabilities":{"image":true,"audio":false,"embedded_context":false},"mcp_capabilities":{"http":true,"sse":false},"session_capabilities":{}}',
    auth_methods       = '[{"id":"kiro-login","name":"Kiro Login","description":"Run ''kiro-cli login'' in terminal to authenticate. See https://kiro.dev/docs/cli/authentication/"}]',
    updated_at         = CAST(strftime('%s','now') AS INTEGER) * 1000
WHERE id = 'e044000d' AND agent_capabilities IS NULL AND auth_methods IS NULL;

-- Kimi (e241c49c) -- kimi acp
UPDATE agent_metadata SET
    agent_capabilities = '{"load_session":true,"mcp_capabilities":{"http":true,"sse":false},"prompt_capabilities":{"audio":false,"embedded_context":true,"image":true},"session_capabilities":{"list":{},"resume":{}}}',
    auth_methods       = '[{"_meta":{"terminal-auth":{"command":"kimi","args":["login"],"label":"Kimi Code Login","env":{},"type":"terminal"}},"description":"Run `kimi login` command in the terminal, then follow the instructions to finish login.","id":"login","name":"Login with Kimi account"}]',
    updated_at         = CAST(strftime('%s','now') AS INTEGER) * 1000
WHERE id = 'e241c49c' AND agent_capabilities IS NULL AND auth_methods IS NULL;

-- Auggie (eb895030) -- auggie --acp
UPDATE agent_metadata SET
    agent_capabilities = '{"load_session":true,"prompt_capabilities":{"image":true},"session_capabilities":{"list":{}}}',
    auth_methods       = '[]',
    updated_at         = CAST(strftime('%s','now') AS INTEGER) * 1000
WHERE id = 'eb895030' AND agent_capabilities IS NULL AND auth_methods IS NULL;

-- Skipped agents (no public install channel we could probe):
--   346b0041 Snow     -- install path not known
--   a0dfb1ec Cursor   -- Cursor IDE binary, not distributed standalone
