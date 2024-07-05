use async_trait::async_trait;
use anyhow::Result;
use serde_json::Value;
use tokio::sync::mpsc;

use crate::patterns::Pattern;

pub mod anthropic;

pub trait Provider {
    fn list_models(&self) -> Vec<String>;
    fn get_client(&self, model: &str) -> Result<Box<dyn Client>>;
}

pub struct StreamResponse {
    pub value: Value,
    pub rx: mpsc::Receiver<Result<String>>,
}

#[async_trait]
pub trait Client {
    async fn send_message(&self, pattern: &Pattern, text: &str) -> Result<String>;
    async fn stream_message(&self, pattern: &Pattern, text: &str) -> Result<StreamResponse>;
}
