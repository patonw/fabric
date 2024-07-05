use async_trait::async_trait;
use anyhow::{anyhow, Context, Result};
use serde_json::{json, Value};
use tracing::{debug, info, info_span, warn};
use reqwest;
use tokio::sync::mpsc;
use tokio::task;
use futures::stream::StreamExt;
use reqwest_eventsource::{Event, EventSource};

use super::{Client, Provider, StreamResponse};
use crate::patterns::Pattern;
use crate::app::App;

pub const FOO: u64 = 1;
pub const MODELS: [&str; 5] = [
    "claude-3-5-sonnet-20240620",
    "claude-3-opus-20240229",
    "claude-3-sonnet-20240229",
    "claude-3-haiku-20240307",
    "claude-2.1"
];

pub struct AnthropicProvider {
    pub api_key: String,
}

impl AnthropicProvider {
    pub fn new(api_key: &str) -> Self {
        Self {
            api_key: api_key.to_string(),
        }
    }
}

impl Provider for AnthropicProvider {
    fn list_models(&self) -> Vec<String> {
        Vec::from(MODELS.map(|s| s.to_string()))
    }

    fn get_client(&self, model: &str) -> Result<Box<dyn Client>> {
        Ok(Box::new(AnthropicClient {
            api_key: self.api_key.clone(),
            model: model.to_string(),
        }))
    }
}

pub struct AnthropicClient {
    pub api_key: String,
    pub model: String,
}

impl AnthropicClient {
    fn build_request(&self, pattern: &Pattern, text: &str, stream: bool) -> reqwest::RequestBuilder {
        let args = App::args();
        reqwest::Client::new()
            //.post("https://httpbin.org/post")
            .post("https://api.anthropic.com/v1/messages")
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .json(&json!({
                "stream": stream,
                "model": &self.model,
                "max_tokens": args.max_tokens,
                "temperature": args.temperature,
                "system": &pattern.system,
                "messages": [
                    { "role": "user", "content": text },
                ],
            }))
    }
}

#[async_trait]
impl Client for AnthropicClient {
    async fn send_message(&self, pattern: &Pattern, text: &str) -> Result<String> {
        let span = info_span!("send_message", pattern=pattern.name);
        let _span = span.enter();

        info!(pattern=&pattern.system, text=text, "Sending message");
        let req = self.build_request(pattern, text, false);
        let resp = req.send().await?;

        let status = resp.status();
        info!(status=status.as_u16(), "Response headers {:?}", resp.headers());

        if !resp.status().is_success() {
            let reason = status.canonical_reason()
                .unwrap_or(status.as_str());
            return Err(anyhow!("Request failed: {}", reason))
        }

        let body = resp.json::<Value>().await?;
        let content = body["content"]
            .as_array()
            .ok_or(anyhow!("Response content missing"))?;

        let result = content.iter()
            .filter(|c| if c["type"] == "text" { true } else {
                warn!("Unexpected content block: {:?}", c);
                false
            })
            .filter_map(|c| c["text"].as_str())
            .fold(String::new(), |mut s, t| { s.push_str(t); s});

        Ok(result)
    }

    async fn stream_message(&self, pattern: &Pattern, text: &str) -> Result<StreamResponse> {
        let span = info_span!("stream_message", pattern=pattern.name);
        let _span = span.enter();

        info!(pattern=&pattern.system, text=text, "Starting stream");

        let req = self.build_request(pattern, text, true);
        let mut es = EventSource::new(req)?;

        let value = json!({});
        let (tx, rx) = mpsc::channel::<Result<String>>(8);

        task::spawn(async move {
            let span = info_span!("sse_consumer");
            let _span = span.enter();

            while let Some(event) = es.next().await {
                match event {
                    Ok(Event::Open) => info!("Connection open"),
                    Ok(Event::Message(message)) => {
                        match message.event.as_str() {
                            "message_start" => {
                                debug!(data=message.data, "message_start");
                                let msg = serde_json::from_str::<Value>(&message.data)
                                    .context("SSE content start")
                                    .map(|data| {
                                        data["message"]["content"].as_array()
                                            .map(|t| t.to_vec())
                                            .unwrap_or(Vec::new())
                                    });
                                match msg {
                                    Ok(content) => for block in content {
                                        tx.send(Ok(block["text"].to_string())).await.unwrap()
                                    },
                                    Err(e) => tx.send(Err(e)).await.unwrap(),
                                }
                            },
                            "content_block_delta" => {
                                let msg = serde_json::from_str::<Value>(&message.data)
                                    .context("SSE decode data")
                                    .map(|data| {
                                        data["delta"]["text"].as_str()
                                            .map(|t| t.to_string())
                                            .unwrap_or(String::new())
                                    });

                                tx.send(msg).await.unwrap()
                            },
                            "content_block_stop" => tx.send(Ok("\n\n".to_string())).await.unwrap(),
                            "message_delta" => debug!(data=message.data, "message_delta"),
                            "message_stop" => es.close(),
                            "content_block_start" | "ping" => {},
                            _ => warn!("Unhandled event type {:?}", message),
                        }
                    },
                    Err(err) => {
                        warn!("Error: {}", err);
                        es.close();
                    }
                }
            }

            info!("Finished streaming")
        });

        Ok(StreamResponse {value, rx})
    }
}
