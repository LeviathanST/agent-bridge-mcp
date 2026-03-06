# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Build & Run

```bash
cargo build                  # dev build
cargo build --release        # release build
cargo clippy --all-targets --all-features -- -D warnings  # lint

# No test suite yet — verify via manual smoke test:
echo '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"test","version":"0.1.0"}}}
{"jsonrpc":"2.0","method":"notifications/initialized"}
{"jsonrpc":"2.0","id":2,"method":"tools/list"}' | cargo run -- --stdio
```

### Running

```bash
agent-bridge --stdio                  # stdio transport (for Claude Code MCP config)
agent-bridge --sse-port 3000          # HTTP transport at /mcp endpoint
agent-bridge --stdio --sse-port 3000  # both simultaneously
agent-bridge --db-path /tmp/test.db --stdio  # custom DB path
```

Logging to stderr via `RUST_LOG` env var (e.g. `RUST_LOG=info`).

## Architecture

This is an MCP (Model Context Protocol) server built on `rmcp` 1.1.0 that lets multiple AI agents communicate through a shared SQLite-backed message bus.

### Module Responsibilities

- **`main.rs`** — CLI parsing (clap), transport setup. Stdio uses `rmcp::ServiceExt::serve()` with `rmcp::transport::io::stdio()`. HTTP uses `StreamableHttpService` mounted on axum at `/mcp`. Both can run concurrently.
- **`bridge.rs`** — MCP tool definitions (the core logic). `AgentBridge` struct holds DB handle, per-session identity state, and the tool router. Each MCP session gets its own `AgentBridge` instance (important for HTTP where the factory closure creates a new one per session).
- **`db.rs`** — SQLite persistence via `rusqlite` behind a `tokio::sync::Mutex<Connection>`. Migrations run synchronously at startup before the connection is wrapped in the mutex. All query methods are `async` (they just lock the mutex).
- **`models.rs`** — Plain data structs: `Agent`, `Message`, `Channel`. Serde-serializable.

### rmcp Macro Pattern

The codebase uses three rmcp macros together — this pattern is required for tools to work:

1. **`#[tool_router]`** on an `impl AgentBridge` block containing `#[tool]` methods — generates a `Self::tool_router()` constructor
2. **`tool_router: ToolRouter<Self>`** field on the struct — initialized via `Self::tool_router()` in `new()`
3. **`#[tool_handler]`** on `impl ServerHandler for AgentBridge` — generates `call_tool()` and `list_tools()` from the router

Tool method signatures: `async fn name(&self, Parameters(p): Parameters<ParamsType>) -> String`. Param types must derive `Deserialize` + `schemars::JsonSchema`. No-param tools omit the `Parameters` argument entirely.

### Key Constraints

- **No `blocking_lock()` inside tokio runtime** — `Db::open()` runs migrations on the raw `Connection` before wrapping it in `Mutex` to avoid this panic.
- **SQLite is not async** — all DB access goes through `tokio::sync::Mutex::lock().await` then synchronous rusqlite calls. This is fine for the expected load but won't scale to high concurrency.
- **Identity is per-session** — each `AgentBridge` instance tracks which agent registered via `Arc<RwLock<Option<String>>>`. Agents must call `register_agent` before using messaging tools.
- **DB path default** is `~/.agent-bridge/bridge.db` with `~` expanded manually.
