-- Allow users to override the preset_agent_type of a built-in assistant
-- without having to duplicate it. `NULL` means "inherit from the source row".
ALTER TABLE assistant_overrides ADD COLUMN preset_agent_type TEXT;
