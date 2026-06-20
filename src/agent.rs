use crate::openai::{ChatMessage, OpenAIClient, OpenAITool, OpenAIFunction};
use crate::tool::Tool;
use std::env;
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct AgentConfig {
    pub id: String,
    pub name: String,
    pub instructions: String,
    pub model: String,
    pub base_url: Option<String>,
    pub api_key: Option<String>,
    pub temperature: Option<f32>,
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            id: "default-agent".to_string(),
            name: "Agent".to_string(),
            instructions: "".to_string(),
            model: "openai/gpt-4o".to_string(),
            base_url: None,
            api_key: None,
            temperature: None,
        }
    }
}

pub struct Agent {
    config: AgentConfig,
    client: OpenAIClient,
    tools: HashMap<String, Arc<dyn Tool>>,
}

impl Agent {
    pub fn new(config: AgentConfig) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        Self::new_with_tools(config, HashMap::new())
    }

    pub fn new_with_tools(
        mut config: AgentConfig,
        tools: HashMap<String, Arc<dyn Tool>>,
    ) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let _ = dotenvy::dotenv();

        if config.model.is_empty() {
            config.model = env::var("OPENAI_MODEL")
                .or_else(|_| env::var("AGENT_MODEL"))
                .unwrap_or_else(|_| "openai/gpt-4o".to_string());
        }

        let api_key = config.api_key.clone()
            .or_else(|| env::var("OPENAI_API_KEY").ok())
            .ok_or("API key is required. Set OPENAI_API_KEY env var or provide it in the configuration.")?;

        let (provider, _) = OpenAIClient::parse_model_string(&config.model);
        let base_url = config.base_url.clone()
            .or_else(|| env::var("OPENAI_API_BASE").ok())
            .or_else(|| env::var("OPENAI_BASE_URL").ok())
            .unwrap_or_else(|| {
                if provider == "openai" {
                    "https://api.openai.com/v1".to_string()
                } else {
                    "https://api.openai.com/v1".to_string()
                }
            });

        let client = OpenAIClient::new(base_url, api_key);
        Ok(Self { config, client, tools })
    }

    pub fn builder() -> AgentBuilder {
        AgentBuilder::default()
    }

    pub fn id(&self) -> &str {
        &self.config.id
    }

    pub fn name(&self) -> &str {
        &self.config.name
    }

    pub fn instructions(&self) -> &str {
        &self.config.instructions
    }

    pub fn model(&self) -> &str {
        &self.config.model
    }

    pub async fn generate(&self, prompt: &str) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        let mut messages = Vec::new();
        
        // Add system instructions if present
        if !self.config.instructions.is_empty() {
            messages.push(ChatMessage::system(self.config.instructions.clone()));
        }

        // Add user prompt
        messages.push(ChatMessage::user(prompt));

        // Construct OpenAI tools specification
        let tools_spec: Option<Vec<OpenAITool>> = if self.tools.is_empty() {
            None
        } else {
            Some(
                self.tools
                    .values()
                    .map(|tool| OpenAITool {
                        r#type: "function".to_string(),
                        function: OpenAIFunction {
                            name: tool.id().to_string(),
                            description: tool.description().to_string(),
                            parameters: tool.input_schema(),
                        },
                    })
                    .collect(),
            )
        };

        let mut steps = 0;
        let max_steps = 5;

        loop {
            if steps >= max_steps {
                return Err("Exceeded maximum tool execution steps".into());
            }

            // Call LLM with current messages and tools
            let response = self
                .client
                .chat_completion_raw(
                    &self.config.model,
                    messages.clone(),
                    self.config.temperature,
                    tools_spec.clone(),
                )
                .await?;

            // If there are tool calls, we execute them
            if let Some(ref tool_calls) = response.tool_calls {
                if tool_calls.is_empty() {
                    // No tool calls, return text response if any
                    return Ok(response.content.unwrap_or_default());
                }

                // Add assistant's message (which contains the tool calls) to the history
                messages.push(response.clone());

                // Execute each tool call
                for tool_call in tool_calls {
                    let tool_name = &tool_call.function.name;
                    let tool_args_str = &tool_call.function.arguments;
                    
                    println!("Agent [{}] calling tool [{}] with arguments: {}", self.config.name, tool_name, tool_args_str);

                    let result_str = if let Some(tool) = self.tools.get(tool_name) {
                        // Parse arguments as JSON Value
                        let args: serde_json::Value = serde_json::from_str(tool_args_str)
                            .unwrap_or(serde_json::Value::Null);
                        
                        match tool.execute(args).await {
                            Ok(res) => res.to_string(),
                            Err(e) => {
                                println!("Error executing tool [{}]: {}", tool_name, e);
                                serde_json::json!({ "error": e.to_string() }).to_string()
                            }
                        }
                    } else {
                        println!("Tool [{}] not found", tool_name);
                        serde_json::json!({ "error": format!("Tool {} not found", tool_name) }).to_string()
                    };

                    // Add tool response to history
                    messages.push(ChatMessage::tool(
                        tool_call.id.clone(),
                        tool_name.clone(),
                        result_str,
                    ));
                }

                steps += 1;
            } else {
                // If no tool calls, return text response
                return Ok(response.content.unwrap_or_default());
            }
        }
    }
}

#[derive(Default)]
pub struct AgentBuilder {
    id: Option<String>,
    name: Option<String>,
    instructions: Option<String>,
    model: Option<String>,
    base_url: Option<String>,
    api_key: Option<String>,
    temperature: Option<f32>,
    tools: Vec<Arc<dyn Tool>>,
}

impl AgentBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn id(mut self, id: impl Into<String>) -> Self {
        self.id = Some(id.into());
        self
    }

    pub fn name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    pub fn instructions(mut self, instructions: impl Into<String>) -> Self {
        self.instructions = Some(instructions.into());
        self
    }

    pub fn model(mut self, model: impl Into<String>) -> Self {
        self.model = Some(model.into());
        self
    }

    pub fn base_url(mut self, base_url: impl Into<String>) -> Self {
        self.base_url = Some(base_url.into());
        self
    }

    pub fn api_key(mut self, api_key: impl Into<String>) -> Self {
        self.api_key = Some(api_key.into());
        self
    }

    pub fn temperature(mut self, temperature: f32) -> Self {
        self.temperature = Some(temperature);
        self
    }

    pub fn tool(mut self, tool: Arc<dyn Tool>) -> Self {
        self.tools.push(tool);
        self
    }

    pub fn tools(mut self, tools: Vec<Arc<dyn Tool>>) -> Self {
        self.tools.extend(tools);
        self
    }

    pub fn build(self) -> Result<Agent, Box<dyn std::error::Error + Send + Sync>> {
        let _ = dotenvy::dotenv();

        let model = self.model
            .or_else(|| env::var("OPENAI_MODEL").ok())
            .or_else(|| env::var("AGENT_MODEL").ok())
            .unwrap_or_else(|| "openai/gpt-4o".to_string());

        let config = AgentConfig {
            id: self.id.unwrap_or_else(|| "default-agent".to_string()),
            name: self.name.unwrap_or_else(|| "Agent".to_string()),
            instructions: self.instructions.unwrap_or_default(),
            model,
            base_url: self.base_url,
            api_key: self.api_key,
            temperature: self.temperature,
        };

        let mut tools_map = HashMap::new();
        for t in self.tools {
            tools_map.insert(t.id().to_string(), t);
        }

        Agent::new_with_tools(config, tools_map)
    }
}
