use async_trait::async_trait;
use anyhow::{anyhow, bail, Result};
use serde_json::{json, Value};
use tracing::{debug, info, info_span, warn};
use reqwest;
use tokio::sync::mpsc;
use tokio::task;
use futures::stream::StreamExt;
use reqwest_eventsource::{Event, EventSource};
use eventsource_stream::Event as MessageEvent;

use super::{Client, Provider, ChatResponse, StreamResponse};
use crate::patterns::Pattern;
use crate::app::App;
use crate::session::ChatSession;

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
    fn list_models(&self) -> Result<Vec<String>> {
        Ok(Vec::from(MODELS.map(|s| s.to_string())))
    }

    fn get_client(&self, model: &str) -> Result<Box<dyn Client>> {
        Ok(Box::new(AnthropicClient {
            api_key: self.api_key.clone(),
            model: model.to_string(),
            session: None,
        }))
    }
}

pub struct AnthropicClient {
    pub api_key: String,
    pub model: String,
    pub session: Option<ChatSession>,
}

impl AnthropicClient {
    fn build_request(&self, pattern: &Pattern, session: &ChatSession, stream: bool) -> reqwest::RequestBuilder {
        use crate::session::ChatEntry::*;
        let args = App::args();
        let messages: Vec<Value> = session.messages().iter()
            .filter_map(|m| match m {
                Query { content, .. } => Some(json!({"role": "user", "content": content})),
                Reply { content, .. } => Some(json!({"role": "assistant", "content": content})),
                _ => None,
            })
            .collect();

        //debug!("Building request with messages {:?}", &messages);

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
                "messages": &messages,
            }))
    }

    async fn start_event_stream(&self, req: reqwest::RequestBuilder) -> Result<(EventSource, Value)> {
        use Event::*;
        let mut es = EventSource::new(req)?;

        while let Some(event) = es.next().await {
            match event? {
                Open => info!("Connection opened"),
                Message(MessageEvent {event, data, ..}) if event == "message_start" => {
                    let mut envelope: Value = serde_json::from_str(&data)?;
                    let meta = envelope["message"].take();
                    return Ok((es, meta));
                },
                Message(body) => {
                    bail!("Message content before start: {:?}", &body)
                },
            }
        }

        bail!("Stream closed before start")
    }
}

#[async_trait]
impl Client for AnthropicClient {
    async fn send_message(&self, pattern: &Pattern, session: &ChatSession) -> Result<ChatResponse> {
        let span = info_span!("send_message", pattern=pattern.name);
        let _span = span.enter();

        info!(pattern=&pattern.system, "Sending message");
        let req = self.build_request(pattern, session, false);
        let resp = req.send().await?;

        let status = resp.status();
        info!(status=status.as_u16(), "Response headers {:?}", resp.headers());

        if !resp.status().is_success() {
            let reason = status.canonical_reason()
                .unwrap_or(status.as_str());
            return Err(anyhow!("Request failed: {}", reason))
        }

        let mut envelope = resp.json::<Value>().await?;
        let content = envelope["content"].take();
        let body = process_content(content)?;
        let meta = envelope;
        Ok(ChatResponse { meta, body })
    }

    async fn stream_message(&self, pattern: &Pattern, session: &ChatSession) -> Result<StreamResponse> {
        let span = info_span!("stream_message", pattern=pattern.name);
        let _span = span.enter();

        info!(pattern=&pattern.system, "Starting stream");

        let (tx, rx) = mpsc::channel::<Result<String>>(8);
        let req = self.build_request(pattern, session, true);
        let (es, meta) = self.start_event_stream(req).await?;

        task::spawn(async move {
            let span = info_span!("sse_consumer");
            let _span = span.enter();

            match consume_event_stream(es, tx).await {
                Ok(_) => info!("Finished streaming"),
                Err(e) => warn!("Stream consumer finished with errors: {:?}", e),
            }
        });

        Ok(StreamResponse { meta, rx })
    }
}

async fn consume_event_stream(mut es: EventSource, tx: mpsc::Sender<Result<String>>) -> Result<()> {
    use Event::*;
    while let Some(event) = es.next().await {
        match event {
            Ok(Open) => warn!("Connection reopened in-flight"),
            Ok(Message(MessageEvent {event, ..})) if event == "message_stop" => es.close(),
            Ok(Message(message)) => {
                match process_event(message) {
                    Ok(data) => {
                        for d in data {
                            tx.send(Ok(d)).await?;
                        }
                    },
                    Err(ex) => {
                        tx.send(Err(ex)).await?;
                        es.close();
                    },
                }
            },
            Err(err) => {
                warn!("Error: {}", err);
                es.close();
            }
        }
    }

    Ok(())
}

fn process_content(content: Value) -> Result<String> {
    let blocks = content
        .as_array()
        .ok_or(anyhow!("Response content missing"))?;

    let result = blocks.iter()
        .filter(|c| if c["type"] == "text" { true } else {
            warn!("Unexpected content block: {:?}", c);
            false
        })
        .filter_map(|c| c["text"].as_str())
        .fold(String::new(), |mut s, t| { s.push_str(t); s});

    Ok(result)
}

fn process_event(message: MessageEvent) -> Result<Vec<String>> {
    match message.event.as_str() {
        "message_start" => {
            debug!(data=message.data, "message_start");
            let msg = serde_json::from_str::<Value>(&message.data)
                .map(|data| {
                    data["message"]["content"].as_array()
                        .map(|t| t.to_vec())
                        .unwrap_or(Vec::new())
                })?;

            let content = msg.iter()
                .filter_map(|block| block["text"].as_str())
                .map(|s| s.to_string())
                .collect::<Vec<_>>();

            Ok(content)
        },
        "content_block_delta" => {
            let msg = serde_json::from_str::<Value>(&message.data)
                .map(|data| {
                    data["delta"]["text"].as_str()
                        .map(|t| t.to_string())
                        .unwrap_or(String::new())
                })?;

            Ok(vec![msg])
        },
        "content_block_stop" => Ok(vec!["\n".to_string()]),
        "message_delta" => {
            debug!(data=message.data, "message_delta");
            Ok(vec![])
        },
        "content_block_start" | "ping" => Ok(vec![]),
        _ => {
            warn!("Unhandled event type {:?}", message);
            Ok(vec![])
        },
    }
}

#[cfg(test)]
mod tests {
    use cool_asserts::assert_matches;
    use super::*;

    fn make_event(name: &str, data: &str) -> MessageEvent {
        MessageEvent {
            event: name.to_string(),
            data: data.to_string(),
            id: "".to_string(),
            retry: None,
        }
    }

    #[test]
    #[ignore = "not implemented"]
    fn test_build_request() {
        todo!()
    }

    #[test]
    fn empty_content_fails() {
        let result = process_content(json!({}));
        assert_matches!(result, Err(_));
    }

    #[test]
    #[ignore = "not implemented"]
    fn valid_content_returns_data() {
        todo!()
    }

    #[test]
    fn unknown_event_ignored() {
        let result = process_event(make_event("unknown", ""));
        let expected: Vec<String> = Vec::new();

        assert_matches!(result, Ok(arr) if arr == expected);
    }

    #[test]
    fn malformed_delta_event_fails() {
        let result = process_event(make_event("content_block_delta", "not json"));

        assert_matches!(result, Err(_));
    }

    #[test]
    #[ignore = "not implemented"]
    fn empty_message_start_event_pass() {
        todo!()
    }

    #[test]
    #[ignore = "not implemented"]
    fn valid_delta_event_produces_data() {
        todo!()
    }
}
