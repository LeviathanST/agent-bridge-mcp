use rusqlite::{Connection, params};
use std::path::Path;
use tokio::sync::Mutex;

use crate::models::{Agent, Channel, Message};

pub struct Db {
    conn: Mutex<Connection>,
}

impl Db {
    pub fn open(path: &Path) -> Result<Self, rusqlite::Error> {
        let conn = Connection::open(path)?;
        conn.execute_batch("PRAGMA journal_mode=WAL;")?;
        Self::migrate(&conn)?;
        Ok(Db {
            conn: Mutex::new(conn),
        })
    }

    fn migrate(conn: &Connection) -> Result<(), rusqlite::Error> {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS agents (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL UNIQUE,
                role TEXT NOT NULL,
                capabilities TEXT NOT NULL,
                registered_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS channels (
                name TEXT PRIMARY KEY,
                created_by TEXT,
                created_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS messages (
                id TEXT PRIMARY KEY,
                from_agent TEXT NOT NULL,
                to_target TEXT NOT NULL,
                content TEXT NOT NULL,
                channel TEXT,
                created_at TEXT NOT NULL
            );",
        )?;

        // Seed default channels
        let default_channels = ["#general", "#dev", "#review"];
        let now = chrono::Utc::now().to_rfc3339();
        for ch in &default_channels {
            conn.execute(
                "INSERT OR IGNORE INTO channels (name, created_by, created_at) VALUES (?1, NULL, ?2)",
                params![ch, now],
            )?;
        }

        Ok(())
    }

    pub async fn register_agent(&self, agent: &Agent) -> Result<(), String> {
        let conn = self.conn.lock().await;
        conn.execute(
            "INSERT OR REPLACE INTO agents (id, name, role, capabilities, registered_at) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                agent.id,
                agent.name,
                agent.role,
                serde_json::to_string(&agent.capabilities).unwrap(),
                agent.registered_at,
            ],
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    }

    pub async fn list_agents(&self) -> Result<Vec<Agent>, String> {
        let conn = self.conn.lock().await;
        let mut stmt = conn
            .prepare("SELECT id, name, role, capabilities, registered_at FROM agents")
            .map_err(|e| e.to_string())?;
        let agents = stmt
            .query_map([], |row| {
                let caps_str: String = row.get(3)?;
                let capabilities: Vec<String> =
                    serde_json::from_str(&caps_str).unwrap_or_default();
                Ok(Agent {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    role: row.get(2)?,
                    capabilities,
                    registered_at: row.get(4)?,
                })
            })
            .map_err(|e| e.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?;
        Ok(agents)
    }

    pub async fn get_agent_by_name(&self, name: &str) -> Result<Option<Agent>, String> {
        let conn = self.conn.lock().await;
        let mut stmt = conn
            .prepare("SELECT id, name, role, capabilities, registered_at FROM agents WHERE name = ?1")
            .map_err(|e| e.to_string())?;
        let mut rows = stmt
            .query_map(params![name], |row| {
                let caps_str: String = row.get(3)?;
                let capabilities: Vec<String> =
                    serde_json::from_str(&caps_str).unwrap_or_default();
                Ok(Agent {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    role: row.get(2)?,
                    capabilities,
                    registered_at: row.get(4)?,
                })
            })
            .map_err(|e| e.to_string())?;
        match rows.next() {
            Some(Ok(agent)) => Ok(Some(agent)),
            Some(Err(e)) => Err(e.to_string()),
            None => Ok(None),
        }
    }

    pub async fn send_message(&self, msg: &Message) -> Result<(), String> {
        let conn = self.conn.lock().await;
        conn.execute(
            "INSERT INTO messages (id, from_agent, to_target, content, channel, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![msg.id, msg.from_agent, msg.to_target, msg.content, msg.channel, msg.created_at],
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    }

    pub async fn read_messages(
        &self,
        channel: Option<&str>,
        since: Option<&str>,
        limit: u32,
    ) -> Result<Vec<Message>, String> {
        let conn = self.conn.lock().await;
        let mut sql = String::from(
            "SELECT id, from_agent, to_target, content, channel, created_at FROM messages WHERE 1=1",
        );
        let mut param_values: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();

        if let Some(ch) = channel {
            sql.push_str(&format!(" AND channel = ?{}", param_values.len() + 1));
            param_values.push(Box::new(ch.to_string()));
        }
        if let Some(s) = since {
            sql.push_str(&format!(" AND created_at > ?{}", param_values.len() + 1));
            param_values.push(Box::new(s.to_string()));
        }
        sql.push_str(" ORDER BY created_at DESC");
        sql.push_str(&format!(" LIMIT ?{}", param_values.len() + 1));
        param_values.push(Box::new(limit));

        let params_ref: Vec<&dyn rusqlite::types::ToSql> =
            param_values.iter().map(|p| p.as_ref()).collect();

        let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
        let messages = stmt
            .query_map(params_ref.as_slice(), |row| {
                Ok(Message {
                    id: row.get(0)?,
                    from_agent: row.get(1)?,
                    to_target: row.get(2)?,
                    content: row.get(3)?,
                    channel: row.get(4)?,
                    created_at: row.get(5)?,
                })
            })
            .map_err(|e| e.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?;
        Ok(messages)
    }

    pub async fn list_channels(&self) -> Result<Vec<Channel>, String> {
        let conn = self.conn.lock().await;
        let mut stmt = conn
            .prepare("SELECT name, created_by, created_at FROM channels ORDER BY name")
            .map_err(|e| e.to_string())?;
        let channels = stmt
            .query_map([], |row| {
                Ok(Channel {
                    name: row.get(0)?,
                    created_by: row.get(1)?,
                    created_at: row.get(2)?,
                })
            })
            .map_err(|e| e.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?;
        Ok(channels)
    }

    pub async fn create_channel(&self, name: &str, created_by: Option<&str>) -> Result<(), String> {
        let conn = self.conn.lock().await;
        let now = chrono::Utc::now().to_rfc3339();
        conn.execute(
            "INSERT INTO channels (name, created_by, created_at) VALUES (?1, ?2, ?3)",
            params![name, created_by, now],
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    }
}
