use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Serialize)]
struct ChatCompletionRequest {
    model: String,
    messages: Vec<ChatMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
}

#[derive(Debug, Deserialize)]
struct ChatCompletionChoice {
    message: ChatMessage,
}

#[derive(Debug, Deserialize)]
struct ChatCompletionResponse {
    choices: Vec<ChatCompletionChoice>,
}

pub struct OpenAIClient {
    base_url: String,
    api_key: String,
    client: reqwest::Client,
}

impl OpenAIClient {
    pub fn new(base_url: String, api_key: String) -> Self {
        Self {
            base_url,
            api_key,
            client: reqwest::Client::new(),
        }
    }

    pub fn parse_model_string(model: &str) -> (String, String) {
        if let Some((provider, name)) = model.split_once('/') {
            let provider_lower = provider.to_lowercase();
            match provider_lower.as_str() {
                "openai" | "ollama" => (provider_lower, name.to_string()),
                _ => (provider_lower, model.to_string()),
            }
        } else {
            ("openai".to_string(), model.to_string())
        }
    }

    pub async fn chat_completion(
        &self,
        model: &str,
        messages: Vec<ChatMessage>,
        temperature: Option<f32>,
    ) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        let mut base = self.base_url.trim_end_matches('/').to_string();
        if !base.ends_with("/v1") {
            base = format!("{}/v1", base);
        }
        let url = format!("{}/chat/completions", base);
        
        let (_, model_name) = Self::parse_model_string(model);

        let request = ChatCompletionRequest {
            model: model_name,
            messages,
            temperature,
        };

        let response = self
            .client
            .post(&url)
            .bearer_auth(&self.api_key)
            .json(&request)
            .send()
            .await?;

        let status = response.status();
        let text = response.text().await.unwrap_or_default();

        if !status.is_success() {
            return Err(format!("API Request failed with status {}: {}", status, text).into());
        }

        // Clean trailing "data: [DONE]" or other SSE endings if present
        let mut clean_text = text.trim();
        if let Some(idx) = clean_text.find("data: [DONE]") {
            clean_text = clean_text[..idx].trim();
        }

        let resp_body: ChatCompletionResponse = match serde_json::from_str(clean_text) {
            Ok(body) => body,
            Err(e) => {
                return Err(format!(
                    "Failed to deserialize response body: {}. Raw response: {}",
                    e, text
                ).into());
            }
        };

        if let Some(choice) = resp_body.choices.first() {
            Ok(choice.message.content.clone())
        } else {
            Err("No completions returned from the model".into())
        }
    }
}
