-- Create mcp_servers table (MCP server configuration management)
CREATE TABLE IF NOT EXISTS mcp_servers (
    id TEXT PRIMARY KEY NOT NULL,
    name TEXT NOT NULL UNIQUE,
    description TEXT,
    enabled INTEGER NOT NULL DEFAULT 0,
    transport_type TEXT NOT NULL,
    transport_config TEXT NOT NULL,
    tools TEXT,
    status TEXT NOT NULL DEFAULT 'disconnected',
    last_connected INTEGER,
    original_json TEXT,
    builtin INTEGER NOT NULL DEFAULT 0,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_mcp_servers_name ON mcp_servers(name);
CREATE INDEX IF NOT EXISTS idx_mcp_servers_enabled ON mcp_servers(enabled);
