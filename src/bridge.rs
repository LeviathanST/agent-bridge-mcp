use std::sync::Arc;

use rmcp::{
    ServerHandler,
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::{ServerCapabilities, ServerInfo},
    schemars, tool, tool_handler, tool_router,
};
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

use crate::db::Db;
use crate::models::{Agent, Message};

#[derive(Debug, Serialize, Deserialize, schemars::JsonSchema)]
pub struct RegisterAgentParams {
    /// The agent's display name
    pub name: String,
    /// The agent's role (e.g. "coder", "reviewer", "orchestrator")
    pub role: String,
    /// List of capabilities this agent has
    pub capabilities: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize, schemars::JsonSchema)]
pub struct SendMessageParams {
    /// Your agent name (required if not registered in this session)
    pub from: Option<String>,
    /// Target agent name or channel (prefixed with #)
    pub to: String,
    /// Message content
    pub content: String,
}

#[derive(Debug, Serialize, Deserialize, schemars::JsonSchema)]
pub struct BroadcastParams {
    /// Your agent name (required if not registered in this session)
    pub from: Option<String>,
    /// Message content to broadcast
    pub content: String,
    /// Channel to broadcast to (defaults to #general)
    pub channel: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ReadMessagesParams {
    /// Filter by channel name
    pub channel: Option<String>,
    /// Only return messages after this ISO timestamp
    pub since: Option<String>,
    /// Max number of messages to return (default 50)
    pub limit: Option<u32>,
}

#[derive(Debug, Serialize, Deserialize, schemars::JsonSchema)]
pub struct CreateChannelParams {
    /// Channel name (# prefix will be added if missing)
    pub name: String,
}

#[derive(Clone)]
pub struct AgentBridge {
    db: Arc<Db>,
    identity: Arc<RwLock<Option<String>>>,
    tool_router: ToolRouter<Self>,
}

impl AgentBridge {
    pub fn new(db: Arc<Db>) -> Self {
        Self {
            db,
            identity: Arc::new(RwLock::new(None)),
            tool_router: Self::tool_router(),
        }
    }

    async fn resolve_identity(&self, from: Option<String>) -> Result<String, String> {
        if let Some(name) = from {
            // Validate agent exists in DB
            match self.db.get_agent_by_name(&name).await {
                Ok(Some(_)) => return Ok(name),
                Ok(None) => return Err(format!("Agent '{name}' not found. Call register_agent first.")),
                Err(e) => return Err(format!("Error: {e}")),
            }
        }
        self.identity
            .read()
            .await
            .clone()
            .ok_or_else(|| "Provide 'from' param or call register_agent first.".to_string())
    }

    fn normalize_channel(name: &str) -> String {
        if name.starts_with('#') {
            name.to_string()
        } else {
            format!("#{name}")
        }
    }
}

#[tool_router]
impl AgentBridge {
    /// Register this agent with the bridge. Must be called before using other tools.
    #[tool(description = "Register this agent with the bridge")]
    async fn register_agent(&self, Parameters(params): Parameters<RegisterAgentParams>) -> String {
        let agent = Agent {
            id: uuid::Uuid::new_v4().to_string(),
            name: params.name.clone(),
            role: params.role,
            capabilities: params.capabilities,
            registered_at: chrono::Utc::now().to_rfc3339(),
        };

        match self.db.register_agent(&agent).await {
            Ok(()) => {
                let mut identity = self.identity.write().await;
                *identity = Some(params.name.clone());
                format!("Registered as '{}' (id: {})", agent.name, agent.id)
            }
            Err(e) => format!("Registration failed: {e}"),
        }
    }

    /// List all registered agents
    #[tool(description = "List all registered agents")]
    async fn list_agents(&self) -> String {
        match self.db.list_agents().await {
            Ok(agents) => serde_json::to_string_pretty(&agents).unwrap(),
            Err(e) => format!("Error: {e}"),
        }
    }

    /// Return the current agent's identity
    #[tool(description = "Return the current agent's identity")]
    async fn whoami(&self) -> String {
        match self.identity.read().await.as_ref() {
            Some(name) => match self.db.get_agent_by_name(name).await {
                Ok(Some(agent)) => serde_json::to_string_pretty(&agent).unwrap(),
                Ok(None) => format!("Registered as '{name}' but agent not found in DB"),
                Err(e) => format!("Error: {e}"),
            },
            None => "Not registered. Call register_agent first.".to_string(),
        }
    }

    /// Send a message to a specific agent or channel
    #[tool(description = "Send a message to a specific agent or channel. Pass 'from' if not registered in this session.")]
    async fn send_message(&self, Parameters(params): Parameters<SendMessageParams>) -> String {
        let from = match self.resolve_identity(params.from).await {
            Ok(name) => name,
            Err(e) => return e,
        };

        let (to_target, channel) = if params.to.starts_with('#') {
            (params.to.clone(), Some(params.to.clone()))
        } else {
            (params.to.clone(), None)
        };

        let msg = Message {
            id: uuid::Uuid::new_v4().to_string(),
            from_agent: from,
            to_target,
            content: params.content,
            channel,
            created_at: chrono::Utc::now().to_rfc3339(),
        };

        match self.db.send_message(&msg).await {
            Ok(()) => format!("Message sent (id: {})", msg.id),
            Err(e) => format!("Send failed: {e}"),
        }
    }

    /// Broadcast a message to all agents in a channel
    #[tool(description = "Broadcast a message to all agents in a channel. Pass 'from' if not registered in this session.")]
    async fn broadcast(&self, Parameters(params): Parameters<BroadcastParams>) -> String {
        let from = match self.resolve_identity(params.from).await {
            Ok(name) => name,
            Err(e) => return e,
        };

        let channel = Self::normalize_channel(
            params.channel.as_deref().unwrap_or("#general"),
        );

        let msg = Message {
            id: uuid::Uuid::new_v4().to_string(),
            from_agent: from,
            to_target: channel.clone(),
            content: params.content,
            channel: Some(channel.clone()),
            created_at: chrono::Utc::now().to_rfc3339(),
        };

        match self.db.send_message(&msg).await {
            Ok(()) => format!("Broadcast to {channel} (id: {})", msg.id),
            Err(e) => format!("Broadcast failed: {e}"),
        }
    }

    /// Read messages, optionally filtered by channel and time
    #[tool(description = "Read messages, optionally filtered by channel and time")]
    async fn read_messages(&self, Parameters(params): Parameters<ReadMessagesParams>) -> String {
        let channel = params.channel.as_deref().map(Self::normalize_channel);
        let limit = params.limit.unwrap_or(50);

        match self
            .db
            .read_messages(channel.as_deref(), params.since.as_deref(), limit)
            .await
        {
            Ok(messages) => serde_json::to_string_pretty(&messages).unwrap(),
            Err(e) => format!("Error: {e}"),
        }
    }

    /// List all available channels
    #[tool(description = "List all available channels")]
    async fn list_channels(&self) -> String {
        match self.db.list_channels().await {
            Ok(channels) => serde_json::to_string_pretty(&channels).unwrap(),
            Err(e) => format!("Error: {e}"),
        }
    }

    /// Create a new channel
    #[tool(description = "Create a new channel")]
    async fn create_channel(&self, Parameters(params): Parameters<CreateChannelParams>) -> String {
        let identity = self.identity.read().await;
        let created_by = identity.as_deref();
        let name = Self::normalize_channel(&params.name);

        match self.db.create_channel(&name, created_by).await {
            Ok(()) => format!("Channel '{name}' created"),
            Err(e) => format!("Failed to create channel: {e}"),
        }
    }
}

#[tool_handler]
impl ServerHandler for AgentBridge {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_instructions(
                "Agent Bridge: Multi-agent communication hub. Call register_agent first to set your identity, then use messaging tools to communicate with other agents.",
            )
    }
}
