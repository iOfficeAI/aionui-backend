-- Create oauth_tokens table (OAuth token storage for MCP servers)
CREATE TABLE IF NOT EXISTS oauth_tokens (
    server_url TEXT PRIMARY KEY NOT NULL,
    access_token TEXT NOT NULL,
    refresh_token TEXT,
    token_type TEXT NOT NULL DEFAULT 'bearer',
    expires_at INTEGER,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);
