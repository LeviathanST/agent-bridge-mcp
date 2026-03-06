use std::sync::Arc;

use axum::{
    extract::{
        State, WebSocketUpgrade,
        ws::{Message as WsMsg, WebSocket},
    },
    response::IntoResponse,
};
use futures::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use tokio::sync::{RwLock, mpsc};

use crate::db::Db;
use crate::hub::Hub;
use crate::models::Message;

#[derive(Clone)]
pub struct WsState {
    pub db: Arc<Db>,
    pub hub: Hub,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
enum ClientMsg {
    #[serde(rename = "register")]
    Register { name: String, role: String, capabilities: Vec<String> },
    #[serde(rename = "send")]
    Send { to: String, content: String },
    #[serde(rename = "broadcast")]
    Broadcast { content: String, channel: Option<String> },
    #[serde(rename = "read")]
    Read { channel: Option<String>, since: Option<String>, limit: Option<u32> },
    #[serde(rename = "list_agents")]
    ListAgents,
    #[serde(rename = "list_channels")]
    ListChannels,
}

#[derive(Debug, Serialize)]
#[serde(tag = "type")]
enum ServerMsg {
    #[serde(rename = "ok")]
    Ok { data: serde_json::Value },
    #[serde(rename = "error")]
    Error { message: String },
    #[serde(rename = "message")]
    Message {
        id: String,
        from: String,
        to: String,
        content: String,
        channel: Option<String>,
        created_at: String,
    },
}

pub async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<WsState>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_socket(socket, state))
}

async fn handle_socket(socket: WebSocket, state: WsState) {
    let (sink, stream) = socket.split();
    let mut sink: futures::stream::SplitSink<WebSocket, WsMsg> = sink;
    let mut stream: futures::stream::SplitStream<WebSocket> = stream;
    let identity: Arc<RwLock<Option<String>>> = Arc::new(RwLock::new(None));

    // mpsc channel so multiple producers (hub forward + reply) can write to the single sink.
    let (out_tx, mut out_rx) = mpsc::channel::<String>(64);

    // Task: drain outbound channel into the WebSocket sink.
    let sink_task = tokio::spawn(async move {
        while let Some(text) = out_rx.recv().await {
            if sink.send(WsMsg::text(text)).await.is_err() {
                break;
            }
        }
    });

    // Task: forward hub messages to outbound channel.
    let mut hub_rx = state.hub.subscribe();
    let hub_tx = out_tx.clone();
    let hub_identity = identity.clone();
    let hub_task = tokio::spawn(async move {
        while let Ok(msg) = hub_rx.recv().await {
            let my_name = hub_identity.read().await;
            let dominated = match &*my_name {
                Some(name) => {
                    msg.to_target == *name
                        || msg.channel.is_some()
                        || msg.to_target.starts_with('#')
                }
                None => false,
            };
            let is_self = my_name.as_deref() == Some(msg.from_agent.as_str());
            drop(my_name);

            if dominated && !is_self {
                let server_msg = ServerMsg::Message {
                    id: msg.id,
                    from: msg.from_agent,
                    to: msg.to_target,
                    content: msg.content,
                    channel: msg.channel,
                    created_at: msg.created_at,
                };
                if hub_tx.send(serde_json::to_string(&server_msg).unwrap()).await.is_err() {
                    break;
                }
            }
        }
    });

    // Process incoming client messages.
    while let Some(Ok(ws_msg)) = stream.next().await {
        let text = match ws_msg {
            WsMsg::Text(t) => t.to_string(),
            WsMsg::Close(_) => break,
            _ => continue,
        };

        let reply = match serde_json::from_str::<ClientMsg>(&text) {
            Ok(client_msg) => process_msg(client_msg, &state, &identity).await,
            Err(e) => ServerMsg::Error { message: format!("Invalid message: {e}") },
        };

        if out_tx.send(serde_json::to_string(&reply).unwrap()).await.is_err() {
            break;
        }
    }

    hub_task.abort();
    drop(out_tx);
    let _ = sink_task.await;
}

async fn process_msg(
    msg: ClientMsg,
    state: &WsState,
    identity: &Arc<RwLock<Option<String>>>,
) -> ServerMsg {
    match msg {
        ClientMsg::Register { name, role, capabilities } => {
            let agent = crate::models::Agent {
                id: uuid::Uuid::new_v4().to_string(),
                name: name.clone(),
                role,
                capabilities,
                registered_at: chrono::Utc::now().to_rfc3339(),
            };
            match state.db.register_agent(&agent).await {
                Ok(()) => {
                    *identity.write().await = Some(name.clone());
                    ServerMsg::Ok {
                        data: serde_json::json!({ "registered": name, "id": agent.id }),
                    }
                }
                Err(e) => ServerMsg::Error { message: e },
            }
        }
        ClientMsg::Send { to, content } => {
            let from = match identity.read().await.clone() {
                Some(n) => n,
                None => return ServerMsg::Error { message: "Not registered. Send a 'register' message first.".into() },
            };
            let (to_target, channel) = if to.starts_with('#') {
                (to.clone(), Some(to.clone()))
            } else {
                (to.clone(), None)
            };
            let msg = Message {
                id: uuid::Uuid::new_v4().to_string(),
                from_agent: from,
                to_target,
                content,
                channel,
                created_at: chrono::Utc::now().to_rfc3339(),
            };
            match state.db.send_message(&msg).await {
                Ok(()) => {
                    state.hub.publish(msg.clone());
                    ServerMsg::Ok { data: serde_json::json!({ "sent": msg.id }) }
                }
                Err(e) => ServerMsg::Error { message: e },
            }
        }
        ClientMsg::Broadcast { content, channel } => {
            let from = match identity.read().await.clone() {
                Some(n) => n,
                None => return ServerMsg::Error { message: "Not registered. Send a 'register' message first.".into() },
            };
            let channel = normalize_channel(channel.as_deref().unwrap_or("#general"));
            let msg = Message {
                id: uuid::Uuid::new_v4().to_string(),
                from_agent: from,
                to_target: channel.clone(),
                content,
                channel: Some(channel),
                created_at: chrono::Utc::now().to_rfc3339(),
            };
            match state.db.send_message(&msg).await {
                Ok(()) => {
                    state.hub.publish(msg.clone());
                    ServerMsg::Ok { data: serde_json::json!({ "broadcast": msg.id }) }
                }
                Err(e) => ServerMsg::Error { message: e },
            }
        }
        ClientMsg::Read { channel, since, limit } => {
            let channel = channel.map(|ch| normalize_channel(&ch));
            match state.db.read_messages(channel.as_deref(), since.as_deref(), limit.unwrap_or(50)).await {
                Ok(messages) => ServerMsg::Ok { data: serde_json::json!(messages) },
                Err(e) => ServerMsg::Error { message: e },
            }
        }
        ClientMsg::ListAgents => {
            match state.db.list_agents().await {
                Ok(agents) => ServerMsg::Ok { data: serde_json::json!(agents) },
                Err(e) => ServerMsg::Error { message: e },
            }
        }
        ClientMsg::ListChannels => {
            match state.db.list_channels().await {
                Ok(channels) => ServerMsg::Ok { data: serde_json::json!(channels) },
                Err(e) => ServerMsg::Error { message: e },
            }
        }
    }
}

fn normalize_channel(name: &str) -> String {
    if name.starts_with('#') { name.to_string() } else { format!("#{name}") }
}
