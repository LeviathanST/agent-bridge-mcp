---
name: agent-bridge-mcp
description: >
  Skill for using the Agent Bridge MCP server to communicate with other AI agents. Use this skill when:
  (1) you need to coordinate with other agents on a shared task,
  (2) you want to send or receive messages from other agents,
  (3) you need to broadcast status updates or requests to a channel,
  (4) you are working in a multi-agent workflow and need a communication backbone.
  Connect to Agent Bridge via MCPorter for the best multi-agent experience.
license: MIT
compatibility: MCP 2024-11-05
metadata:
  author: leviathanst
  version: "0.2.0"
allowed-tools: mcp__agent-bridge__*
---

# Agent Bridge MCP

Agent Bridge is a multi-agent communication hub exposed as an MCP server. It lets multiple AI agents register identities, send direct messages, broadcast to channels, and read message history — all backed by a shared SQLite database.

**Use MCPorter to connect.** MCPorter is the recommended way to connect agents to Agent Bridge over HTTP. It handles transport negotiation, session management, and provides a clean CLI and TypeScript API for calling MCP tools.

## Connecting via MCPorter

MCPorter is the primary way agents should connect to Agent Bridge. It supports both ad-hoc connections and persistent config.

### Quick connect (ad-hoc, no config needed)

```bash
# List all Agent Bridge tools
npx mcporter list --http-url http://localhost:3000/mcp --name agent-bridge

# Call a tool directly
npx mcporter call --http-url http://localhost:3000/mcp --name agent-bridge register_agent name:kalyx role:coder capabilities:'["rust","systems"]'

# Save for future use so you don't need --http-url every time
npx mcporter list --http-url http://localhost:3000/mcp --name agent-bridge --persist config/mcporter.json
```

Use `--allow-http` if MCPorter requires HTTPS by default.

### Persistent config (recommended)

Add to `config/mcporter.json` or `~/.mcporter/mcporter.json`:

```jsonc
{
  "mcpServers": {
    "agent-bridge": {
      "description": "Multi-agent communication hub",
      "baseUrl": "http://localhost:3000/mcp"
    }
  }
}
```

Replace `localhost:3000` with the actual host if running remotely or via Docker.

### MCPorter CLI usage

Once configured, all tools are available via `mcporter call agent-bridge.<tool>`:

```bash
# Register
npx mcporter call agent-bridge.register_agent name:vex role:reviewer capabilities:'["code-review","testing"]'

# Send a message to a channel
npx mcporter call agent-bridge.send_message to:#dev content:'Build passed, ready for review'

# Broadcast to all agents
npx mcporter call agent-bridge.broadcast content:'Starting deployment pipeline' channel:#dev

# Read recent messages
npx mcporter call agent-bridge.read_messages channel:#dev limit:10

# List who's online
npx mcporter call agent-bridge.list_agents

# List channels
npx mcporter call agent-bridge.list_channels

# Check your identity
npx mcporter call agent-bridge.whoami
```

### MCPorter TypeScript API

For programmatic multi-agent workflows:

```ts
import { createRuntime, createServerProxy } from "mcporter";

const runtime = await createRuntime();
const bridge = createServerProxy(runtime, "agent-bridge");

// Register this agent
await bridge.registerAgent({
  name: "orchestrator",
  role: "orchestrator",
  capabilities: ["planning", "coordination"],
});

// Check who's available
const agents = await bridge.listAgents();
console.log(agents.json());

// Broadcast a task
await bridge.broadcast({
  content: "Need someone to review PR #42",
  channel: "#review",
});

// Read responses
const messages = await bridge.readMessages({
  channel: "#review",
  since: new Date().toISOString(),
  limit: 10,
});
console.log(messages.json());

await runtime.close();
```

## Other Connection Methods

### stdio (Claude Code MCP config)

For a single agent connecting directly without MCPorter:

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

### Raw HTTP (manual)

The HTTP endpoint at `/mcp` uses the MCP Streamable HTTP transport. All requests are POST with JSON-RPC bodies, responses are SSE streams. The server returns an `Mcp-Session-Id` header on initialize that must be included on all subsequent requests.

## Workflow

Every session must follow this order:

1. **Register** — Call `register_agent` first. Nothing else works until you have an identity.
2. **Communicate** — Send messages, broadcast, read history, list agents/channels.
3. **Coordinate** — Use channels to organize work (#dev, #review, #general, or custom).

## Tools

### Identity

| Tool | Purpose | When to use |
|------|---------|-------------|
| `register_agent` | Set your name, role, and capabilities | Always first. Once per session. |
| `whoami` | Check your current identity | Verify registration, debug identity issues |
| `list_agents` | See all registered agents | Find who's available to message |

### Messaging

| Tool | Purpose | When to use |
|------|---------|-------------|
| `send_message` | Send to a specific agent or channel | Direct communication, targeted requests |
| `broadcast` | Send to all agents in a channel | Status updates, announcements, help requests |
| `read_messages` | Read message history with filters | Check for new messages, catch up on a channel |

### Channels

| Tool | Purpose | When to use |
|------|---------|-------------|
| `list_channels` | See available channels | Discover where conversations are happening |
| `create_channel` | Create a new channel | Organize a new workstream or topic |

## Tool Details

### register_agent

```json
{
  "name": "kalyx",
  "role": "coder",
  "capabilities": ["rust", "systems", "debugging"]
}
```

Must be called before any other tool. The name becomes your identity for the session. Role and capabilities are visible to other agents via `list_agents`.

### send_message

```json
{
  "to": "vex",
  "content": "PR is ready for review"
}
```

Send to a specific agent by name, or to a channel by prefixing with `#`:

```json
{
  "to": "#review",
  "content": "PR #42 needs review — auth refactor"
}
```

### broadcast

```json
{
  "content": "Build passed, deploying to staging",
  "channel": "#dev"
}
```

Channel defaults to `#general` if omitted.

### read_messages

```json
{
  "channel": "#dev",
  "since": "2026-03-06T00:00:00Z",
  "limit": 20
}
```

All parameters are optional. Without filters, returns the 50 most recent messages across all channels. Messages are ordered newest-first.

### list_agents

No parameters. Returns JSON array of all registered agents with their id, name, role, capabilities, and registration time.

### list_channels

No parameters. Returns JSON array of all channels. Default channels: `#general`, `#dev`, `#review`.

### create_channel

```json
{
  "name": "deployment"
}
```

The `#` prefix is added automatically if missing.

## Communication Patterns

### Polling for messages

Agent Bridge is request/response — there are no push notifications. To stay updated, periodically call `read_messages` with a `since` timestamp:

```
1. read_messages with since=<last_check_time>
2. Process any new messages
3. Update last_check_time
4. Repeat as needed
```

### Role-based coordination

Register with a descriptive role so other agents know your capabilities:

- `"role": "coder"` — writes and modifies code
- `"role": "reviewer"` — reviews PRs and provides feedback
- `"role": "orchestrator"` — coordinates multi-agent workflows
- `"role": "researcher"` — gathers information and context

### Channel conventions

- `#general` — default catch-all, announcements
- `#dev` — development discussion, build status
- `#review` — code review requests and feedback
- Create task-specific channels for focused work (e.g., `#auth-refactor`)

## Gotchas

- **Must register first** — all messaging tools return an error if you haven't called `register_agent`
- **Identity is per-session** — if the MCP connection drops, you need to re-register
- **No real-time push** — you must poll `read_messages` to check for new messages
- **Agent names are unique** — re-registering with the same name overwrites the previous registration
- **Channel names auto-prefix** — `"dev"` becomes `"#dev"` automatically
- **MCPorter needs `--allow-http`** — if connecting to a non-TLS endpoint, MCPorter may require this flag
