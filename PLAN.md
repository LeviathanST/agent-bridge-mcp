# Agent Bridge — Multi-Agent MCP Communication Hub

## Overview
A Rust MCP server that enables multiple AI agents (Claude Code instances, etc.) to communicate through a shared message bus. Agents register identities, send direct/broadcast messages, and organize via channels — all backed by SQLite.

## Architecture

```
┌─────────────┐     ┌─────────────┐     ┌─────────────┐
│  Agent A     │     │  Agent B     │     │  Agent C     │
│ (Claude Code)│     │ (Claude Code)│     │ (MCPorter)   │
└──────┬───────┘     └──────┬───────┘     └──────┬───────┘
       │ stdio              │ stdio              │ HTTP
       └────────────┬───────┘────────────────────┘
                    │
            ┌───────▼───────┐
            │  Agent Bridge  │
            │  (MCP Server)  │
            ├────────────────┤
            │  SQLite DB     │
            └────────────────┘
```

## MCP Tools

| Tool | Description |
|------|-------------|
| `register_agent` | Register identity (name, role, capabilities) |
| `list_agents` | List all registered agents |
| `whoami` | Return current agent's identity |
| `send_message` | Send message to agent or channel |
| `broadcast` | Broadcast to a channel |
| `read_messages` | Read messages (filter by channel/time/limit) |
| `list_channels` | List all channels |
| `create_channel` | Create a new channel |

## Transports

- **stdio** (`--stdio`): For Claude Code MCP config
- **Streamable HTTP** (`--sse-port <PORT>`): For web clients, MCPorter, etc. Endpoint at `/mcp`

## Files

```
agent-bridge/
├── Cargo.toml
├── PLAN.md
└── src/
    ├── main.rs      — CLI, transport setup (stdio + HTTP)
    ├── bridge.rs    — MCP tool implementations
    ├── db.rs        — SQLite persistence layer
    └── models.rs    — Data types (Agent, Message, Channel)
```

## Database

SQLite with WAL mode. Tables: `agents`, `messages`, `channels`.
Default channels seeded on first run: `#general`, `#dev`, `#review`.
Default path: `~/.agent-bridge/bridge.db`

## Usage

```bash
# stdio mode (for Claude Code MCP config)
agent-bridge --stdio

# HTTP mode
agent-bridge --sse-port 3000

# Both simultaneously
agent-bridge --stdio --sse-port 3000

# Custom DB path
agent-bridge --stdio --db-path /tmp/bridge.db
```

### Claude Code MCP Config

```json
{
  "mcpServers": {
    "agent-bridge": {
      "command": "/path/to/agent-bridge",
      "args": ["--stdio"]
    }
  }
}
```

## Dependencies

- `rmcp` 1.1.0 — MCP protocol implementation
- `rusqlite` — SQLite (bundled)
- `axum` — HTTP server for streamable HTTP transport
- `tokio` — async runtime
- `clap` — CLI parsing
- `serde`/`serde_json` — serialization

## Phase 2 Ideas

- Subscriptions / real-time notifications (MCP notifications)
- Agent presence / heartbeat
- Message threading / reply-to
- Role-based permissions
- Multi-bridge federation
