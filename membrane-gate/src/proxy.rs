use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use tracing::{info, warn};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatRequest {
    pub model: String,
    pub messages: Vec<ChatMessage>,
    #[serde(default)]
    pub stream: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatChoice {
    pub index: u32,
    pub message: ChatMessage,
    pub finish_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatResponse {
    pub id: String,
    pub object: String,
    pub model: String,
    pub choices: Vec<ChatChoice>,
}

pub struct LlmProxy {
    model_api_url: Option<String>,
}

impl LlmProxy {
    pub fn new(model_api_url: Option<String>) -> Self {
        Self { model_api_url }
    }

    pub fn model_api_url(&self) -> Option<&str> {
        self.model_api_url.as_deref()
    }

    pub async fn chat(&self, req: &ChatRequest) -> Result<ChatResponse> {
        if req.stream {
            bail!("streaming not supported in Phase 0 gate");
        }
        if let Some(url) = &self.model_api_url {
            match self.provider_chat(url, req).await {
                Ok(resp) => return Ok(resp),
                Err(err) => warn!(error = %err, "model API unavailable, using mock response"),
            }
        }
        Ok(mock_response(req))
    }

    pub async fn complete(&self, model: &str, prompt: &str) -> Result<String> {
        let resp = self
            .chat(&ChatRequest {
                model: model.to_string(),
                messages: vec![ChatMessage {
                    role: "user".into(),
                    content: prompt.to_string(),
                }],
                stream: false,
            })
            .await?;
        Ok(resp
            .choices
            .first()
            .map(|c| c.message.content.clone())
            .unwrap_or_default())
    }

    async fn provider_chat(&self, base: &str, req: &ChatRequest) -> Result<ChatResponse> {
        let endpoint = format!("{}/v1/chat/completions", base.trim_end_matches('/'));
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(120))
            .build()?;
        let resp = client
            .post(&endpoint)
            .json(req)
            .send()
            .await
            .context("model API request")?;
        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            bail!("model API status {status}: {body}");
        }
        let parsed: ChatResponse = resp.json().await.context("model API response")?;
        info!(model = %parsed.model, "model API completion");
        Ok(parsed)
    }
}

fn mock_response(req: &ChatRequest) -> ChatResponse {
    let prompt_len: usize = req.messages.iter().map(|m| m.content.len()).sum();
    ChatResponse {
        id: "membrane-mock".into(),
        object: "chat.completion".into(),
        model: req.model.clone(),
        choices: vec![ChatChoice {
            index: 0,
            message: ChatMessage {
                role: "assistant".into(),
                content: format!("[membrane-mock] received {prompt_len} bytes"),
            },
            finish_reason: Some("stop".into()),
        }],
    }
}
