-- Pre-fill supports_team=true for agents that belong to the legacy whitelist.
-- Uses json_set on the existing behavior_policy JSON column.
-- Targets: backend IN ('claude','codex','gemini','codebuddy') OR agent_type='aionrs'
-- (aionrs has backend=NULL so we match by agent_type instead)

UPDATE agent_metadata
SET behavior_policy = json_set(
    COALESCE(behavior_policy, '{}'),
    '$.supports_team',
    json('true')
),
    updated_at = strftime('%s', 'now') * 1000
WHERE backend IN ('claude', 'codex', 'gemini', 'codebuddy')
   OR agent_type = 'aionrs';
