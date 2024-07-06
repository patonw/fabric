use async_trait::async_trait;
use anyhow::Result;
use serde_json::Value;
use tokio::sync::mpsc;

use crate::patterns::Pattern;
use crate::session::ChatSession;

pub mod anthropic;

pub trait Provider {
    fn list_models(&self) -> Result<Vec<String>>;
    fn get_client(&self, model: &str) -> Result<Box<dyn Client>>;
}

pub struct ChatResponse {
    pub meta: Value,
    pub body: String,
}

pub struct StreamResponse {
    pub meta: Value,
    pub rx: mpsc::Receiver<Result<String>>,
}

#[async_trait]
pub trait Client {
    async fn send_message(&self, pattern: &Pattern, session: &ChatSession) -> Result<ChatResponse>;
    async fn stream_message(&self, pattern: &Pattern, session: &ChatSession) -> Result<StreamResponse>;
}
