use crate::openai::{ChatMessage, OpenAIClient, OpenAITool, OpenAIFunction, ToolCall, FunctionCall, OpenAIStream};
use crate::tool::Tool;
use crate::memory::Memory;
use std::env;
use std::collections::HashMap;
use std::sync::Arc;
use serde::{Deserialize, Serialize};

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
    memory: Option<Memory>,
    task_signal_provider: bool,
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
        Ok(Self { config, client, tools, memory: None, task_signal_provider: false })
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

    pub fn tools(&self) -> &HashMap<String, Arc<dyn Tool>> {
        &self.tools
    }

    pub fn memory(&self) -> Option<&Memory> {
        self.memory.as_ref()
    }

    pub async fn generate(&self, prompt: &str) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        self.generate_with_options(prompt, GenerateOptions::default()).await
    }

    pub async fn generate_with_options(
        &self,
        prompt: &str,
        options: GenerateOptions,
    ) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        let mut messages = Vec::new();

        let has_memory_and_thread = self.memory.is_some() && options.thread_id.is_some();
        let user_prompt_index = if has_memory_and_thread {
            let memory = self.memory.as_ref().unwrap();
            let thread_id = options.thread_id.as_ref().unwrap();
            
            let history = memory.storage().get_messages(thread_id).await?;
            
            if !self.config.instructions.is_empty() {
                messages.push(ChatMessage::system(self.config.instructions.clone()));
            }

            if self.task_signal_provider {
                if let Ok(Some(session)) = memory.storage().get_thread(thread_id).await {
                    if let Some(tasks_val) = session.state.get("tasks") {
                        if let Ok(tasks) = serde_json::from_value::<Vec<crate::tools::task::Task>>(tasks_val.clone()) {
                            if !tasks.is_empty() {
                                let mut tasks_xml = String::from("<tasks>\n");
                                for task in &tasks {
                                    tasks_xml.push_str(&format!(
                                        "  <task id=\"{}\" status=\"{}\" activeForm=\"{}\">{}</task>\n",
                                        task.id, task.status, task.active_form, task.content
                                    ));
                                }
                                tasks_xml.push_str("</tasks>");
                                messages.push(ChatMessage::system(format!(
                                    "Here is the list of tasks currently tracked for this thread:\n{}",
                                    tasks_xml
                                )));
                            }
                        }
                    }
                }
            }

            if !history.is_empty() {
                let start_idx = if let Some(limit) = memory.config().last_messages {
                    if history.len() > limit {
                        history.len() - limit
                    } else {
                        0
                    }
                } else {
                    0
                };

                for msg in &history[start_idx..] {
                    if msg.role == "system" {
                        continue;
                    }
                    messages.push(msg.clone());
                }
            }
            messages.push(ChatMessage::user(prompt));
            messages.len() - 1
        } else {
            if !self.config.instructions.is_empty() {
                messages.push(ChatMessage::system(self.config.instructions.clone()));
            }
            messages.push(ChatMessage::user(prompt));
            messages.len() - 1
        };

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
        let max_steps = 15;

        let final_content = loop {
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
                    break response.content.clone().unwrap_or_default();
                }

                // Add assistant's message (which contains the tool calls) to the history
                messages.push(response.clone());

                // Execute each tool call
                for tool_call in tool_calls {
                    let tool_name = &tool_call.function.name;
                    let tool_args_str = &tool_call.function.arguments;
                    
                    println!("Agent [{}] calling tool [{}] with arguments: {}", self.config.name, tool_name, tool_args_str);

                    let result_str = if let Some(tool) = self.tools.get(tool_name) {
                        let args: serde_json::Value = serde_json::from_str(tool_args_str)
                            .unwrap_or(serde_json::Value::Null);
                        
                        let context = crate::tools::task::ExecutionContext {
                            thread_id: options.thread_id.clone(),
                            resource_id: options.resource_id.clone(),
                            memory: self.memory.clone(),
                        };
                        let tool_name_for_scope = tool_name.clone();
                        crate::tools::task::CURRENT_CONTEXT.scope(context, async move {
                            match tool.execute(args).await {
                                Ok(res) => res.to_string(),
                                Err(e) => {
                                    println!("Error executing tool [{}]: {}", tool_name_for_scope, e);
                                    serde_json::json!({ "error": e.to_string() }).to_string()
                                }
                            }
                        }).await
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
                break response.content.clone().unwrap_or_default();
            }
        };

        // Add the final assistant response to messages
        messages.push(ChatMessage::assistant(Some(final_content.clone()), None));

        // Save new messages to memory
        if has_memory_and_thread {
            let memory = self.memory.as_ref().unwrap();
            let thread_id = options.thread_id.as_ref().unwrap();
            
            let thread_exists = memory.storage().get_thread(thread_id).await?.is_some();
            if !thread_exists {
                memory.storage().create_thread(thread_id, options.resource_id.clone()).await?;
            }
            let mut full_history = memory.storage().get_messages(thread_id).await?;

            let new_messages = &messages[user_prompt_index..];
            full_history.extend(new_messages.iter().cloned());
            memory.storage().save_messages(thread_id, full_history).await?;
        }

        Ok(final_content)
    }

    pub async fn stream(&self, prompt: &str) -> Result<AgentStream, Box<dyn std::error::Error + Send + Sync>> {
        self.stream_with_options(prompt, GenerateOptions::default()).await
    }

    pub async fn stream_with_options(
        &self,
        prompt: &str,
        options: GenerateOptions,
    ) -> Result<AgentStream, Box<dyn std::error::Error + Send + Sync>> {
        let mut messages = Vec::new();
        
        let has_memory_and_thread = self.memory.is_some() && options.thread_id.is_some();
            let user_prompt_index = if has_memory_and_thread {
                let memory = self.memory.as_ref().unwrap();
                let thread_id = options.thread_id.as_ref().unwrap();
                
                let history = memory.storage().get_messages(thread_id).await?;
                
                if !self.config.instructions.is_empty() {
                    messages.push(ChatMessage::system(self.config.instructions.clone()));
                }

                if self.task_signal_provider {
                    if let Ok(Some(session)) = memory.storage().get_thread(thread_id).await {
                        if let Some(tasks_val) = session.state.get("tasks") {
                            if let Ok(tasks) = serde_json::from_value::<Vec<crate::tools::task::Task>>(tasks_val.clone()) {
                                if !tasks.is_empty() {
                                    let mut tasks_xml = String::from("<tasks>\n");
                                    for task in &tasks {
                                        tasks_xml.push_str(&format!(
                                            "  <task id=\"{}\" status=\"{}\" activeForm=\"{}\">{}</task>\n",
                                            task.id, task.status, task.active_form, task.content
                                        ));
                                    }
                                    tasks_xml.push_str("</tasks>");
                                    messages.push(ChatMessage::system(format!(
                                        "Here is the list of tasks currently tracked for this thread:\n{}",
                                        tasks_xml
                                    )));
                                }
                            }
                        }
                    }
                }

            if !history.is_empty() {
                let start_idx = if let Some(limit) = memory.config().last_messages {
                    if history.len() > limit {
                        history.len() - limit
                    } else {
                        0
                    }
                } else {
                    0
                };

                for msg in &history[start_idx..] {
                    if msg.role == "system" {
                        continue;
                    }
                    messages.push(msg.clone());
                }
            }
            messages.push(ChatMessage::user(prompt));
            messages.len() - 1
        } else {
            if !self.config.instructions.is_empty() {
                messages.push(ChatMessage::system(self.config.instructions.clone()));
            }
            messages.push(ChatMessage::user(prompt));
            messages.len() - 1
        };

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

        Ok(AgentStream {
            client: self.client.clone(),
            model: self.config.model.clone(),
            agent_name: self.config.name.clone(),
            messages,
            temperature: self.config.temperature,
            tools: self.tools.clone(),
            tools_spec,
            steps: 0,
            max_steps: 15,
            state: StreamState::Init,
            memory: self.memory.clone(),
            thread_id: options.thread_id,
            resource_id: options.resource_id,
            user_prompt_index,
        })
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
    memory: Option<Memory>,
    task_signal_provider: bool,
}

pub struct TaskSignalProvider;

impl TaskSignalProvider {
    pub fn new() -> Self {
        Self
    }
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

    pub fn memory(mut self, memory: Memory) -> Self {
        self.memory = Some(memory);
        self
    }

    pub fn signal(mut self, _provider: TaskSignalProvider) -> Self {
        self.task_signal_provider = true;
        self
    }

    pub fn signals(mut self, _providers: Vec<TaskSignalProvider>) -> Self {
        self.task_signal_provider = true;
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
        if self.task_signal_provider {
            for t in crate::tools::all_task_tools() {
                tools_map.insert(t.id().to_string(), t.clone());
            }
        }
        let mut agent = Agent::new_with_tools(config, tools_map)?;
        agent.memory = self.memory;
        agent.task_signal_provider = self.task_signal_provider;
        Ok(agent)
    }
}

#[derive(Debug, Default, Clone)]
pub struct GenerateOptions {
    pub thread_id: Option<String>,
    pub resource_id: Option<String>,
}

impl GenerateOptions {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn thread_id(mut self, id: impl Into<String>) -> Self {
        self.thread_id = Some(id.into());
        self
    }

    pub fn resource_id(mut self, id: impl Into<String>) -> Self {
        self.resource_id = Some(id.into());
        self
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum AgentStreamEvent {
    #[serde(rename = "text-delta")]
    TextDelta(String),
    #[serde(rename = "reasoning-delta")]
    ReasoningDelta(String),
    #[serde(rename = "tool-call")]
    ToolCall {
        id: String,
        name: String,
        arguments: String,
    },
    #[serde(rename = "tool-result")]
    ToolResult {
        id: String,
        name: String,
        result: String,
    },
    #[serde(rename = "finish")]
    Finish {
        finish_reason: Option<String>,
        prompt_tokens: Option<u32>,
        completion_tokens: Option<u32>,
        total_tokens: Option<u32>,
    },
}

#[derive(Debug, Clone)]
struct AccumulatedToolCall {
    id: String,
    name: String,
    arguments: String,
}

enum StreamState {
    Init,
    StreamingLLM {
        stream: OpenAIStream,
        accumulated_content: String,
        accumulated_reasoning: String,
        accumulated_tool_calls: Vec<AccumulatedToolCall>,
    },
    ExecutingTools {
        tool_calls: Vec<ToolCall>,
        index: usize,
        yielded_call: bool,
    },
    Finished,
}

pub struct AgentStream {
    client: OpenAIClient,
    model: String,
    agent_name: String,
    messages: Vec<ChatMessage>,
    temperature: Option<f32>,
    tools: HashMap<String, Arc<dyn Tool>>,
    tools_spec: Option<Vec<OpenAITool>>,
    steps: usize,
    max_steps: usize,
    state: StreamState,
    memory: Option<Memory>,
    thread_id: Option<String>,
    resource_id: Option<String>,
    user_prompt_index: usize,
}

impl AgentStream {
    async fn save_memory_if_needed(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        if let (Some(memory), Some(thread_id)) = (&self.memory, &self.thread_id) {
            let thread_exists = memory.storage().get_thread(thread_id).await?.is_some();
            if !thread_exists {
                memory.storage().create_thread(thread_id, self.resource_id.clone()).await?;
            }
            let mut full_history = memory.storage().get_messages(thread_id).await?;

            let new_messages = &self.messages[self.user_prompt_index..];
            full_history.extend(new_messages.iter().cloned());
            memory.storage().save_messages(thread_id, full_history).await?;
        }
        Ok(())
    }
}

impl AgentStream {
    pub async fn next(&mut self) -> Option<Result<AgentStreamEvent, Box<dyn std::error::Error + Send + Sync>>> {
        loop {
            match &mut self.state {
                StreamState::Init => {
                    if self.steps >= self.max_steps {
                        self.state = StreamState::Finished;
                        return Some(Err("Exceeded maximum tool execution steps".into()));
                    }

                    let stream_res = self.client.chat_completion_stream(
                        &self.model,
                        self.messages.clone(),
                        self.temperature,
                        self.tools_spec.clone(),
                    ).await;

                    match stream_res {
                        Ok(openai_stream) => {
                            self.state = StreamState::StreamingLLM {
                                stream: openai_stream,
                                accumulated_content: String::new(),
                                accumulated_reasoning: String::new(),
                                accumulated_tool_calls: Vec::new(),
                            };
                        }
                        Err(e) => {
                            self.state = StreamState::Finished;
                            return Some(Err(e));
                        }
                    }
                }
                StreamState::StreamingLLM {
                    stream,
                    accumulated_content,
                    accumulated_reasoning,
                    accumulated_tool_calls,
                } => {
                    match stream.next().await {
                        Some(Ok(chunk)) => {
                            let mut text_delta = None;
                            let mut reasoning_delta = None;

                            for choice in &chunk.choices {
                                if let Some(ref content) = choice.delta.content {
                                    accumulated_content.push_str(content);
                                    text_delta = Some(content.clone());
                                }
                                if let Some(ref reasoning) = choice.delta.reasoning_content {
                                    accumulated_reasoning.push_str(reasoning);
                                    reasoning_delta = Some(reasoning.clone());
                                }
                                if let Some(ref tool_calls) = choice.delta.tool_calls {
                                    for delta in tool_calls {
                                        let idx = delta.index as usize;
                                        while accumulated_tool_calls.len() <= idx {
                                            accumulated_tool_calls.push(AccumulatedToolCall {
                                                id: String::new(),
                                                name: String::new(),
                                                arguments: String::new(),
                                            });
                                        }
                                        if let Some(id) = &delta.id {
                                            accumulated_tool_calls[idx].id = id.clone();
                                        }
                                        if let Some(func) = &delta.function {
                                            if let Some(name) = &func.name {
                                                accumulated_tool_calls[idx].name = name.clone();
                                            }
                                            if let Some(args) = &func.arguments {
                                                accumulated_tool_calls[idx].arguments.push_str(args);
                                            }
                                        }
                                    }
                                }
                            }

                            if let Some(text) = text_delta {
                                return Some(Ok(AgentStreamEvent::TextDelta(text)));
                            }
                            if let Some(reasoning) = reasoning_delta {
                                return Some(Ok(AgentStreamEvent::ReasoningDelta(reasoning)));
                            }
                        }
                        Some(Err(e)) => {
                            self.state = StreamState::Finished;
                            return Some(Err(e));
                        }
                        None => {
                            let (content, _reasoning, tool_calls) = match std::mem::replace(&mut self.state, StreamState::Init) {
                                StreamState::StreamingLLM {
                                    accumulated_content,
                                    accumulated_reasoning,
                                    accumulated_tool_calls,
                                    ..
                                } => (accumulated_content, accumulated_reasoning, accumulated_tool_calls),
                                _ => unreachable!(),
                            };

                            let assistant_content = if content.is_empty() { None } else { Some(content) };
                            
                            let open_tool_calls = if tool_calls.is_empty() {
                                None
                            } else {
                                Some(tool_calls.iter().map(|tc| ToolCall {
                                    id: tc.id.clone(),
                                    r#type: "function".to_string(),
                                    function: FunctionCall {
                                        name: tc.name.clone(),
                                        arguments: tc.arguments.clone(),
                                    },
                                }).collect::<Vec<_>>())
                            };

                            self.messages.push(ChatMessage::assistant(assistant_content, open_tool_calls.clone()));

                            if let Some(calls) = open_tool_calls {
                                if !calls.is_empty() {
                                    self.state = StreamState::ExecutingTools {
                                        tool_calls: calls,
                                        index: 0,
                                        yielded_call: false,
                                    };
                                    continue;
                                }
                            }

                            if let Err(err) = self.save_memory_if_needed().await {
                                println!("Failed to save memory: {}", err);
                            }
                            self.state = StreamState::Finished;
                            return Some(Ok(AgentStreamEvent::Finish {
                                finish_reason: Some("stop".to_string()),
                                prompt_tokens: None,
                                completion_tokens: None,
                                total_tokens: None,
                            }));
                        }
                    }
                }
                StreamState::ExecutingTools { tool_calls, index, yielded_call } => {
                    let idx = *index;
                    if idx >= tool_calls.len() {
                        self.steps += 1;
                        self.state = StreamState::Init;
                        continue;
                    }

                    let tool_call = &tool_calls[idx];
                    if !*yielded_call {
                        *yielded_call = true;
                        return Some(Ok(AgentStreamEvent::ToolCall {
                            id: tool_call.id.clone(),
                            name: tool_call.function.name.clone(),
                            arguments: tool_call.function.arguments.clone(),
                        }));
                    } else {
                        let tool_name = tool_call.function.name.clone();
                        let tool_args_str = tool_call.function.arguments.clone();
                        let tool_id = tool_call.id.clone();

                        println!("Agent [{}] calling tool [{}] with arguments: {}", self.agent_name, tool_name, tool_args_str);

                        let result_str = if let Some(tool) = self.tools.get(&tool_name) {
                            let args: serde_json::Value = serde_json::from_str(&tool_args_str)
                                .unwrap_or(serde_json::Value::Null);
                            
                            let context = crate::tools::task::ExecutionContext {
                                thread_id: self.thread_id.clone(),
                                resource_id: self.resource_id.clone(),
                                memory: self.memory.clone(),
                            };
                            let tool_name_for_scope = tool_name.clone();
                            crate::tools::task::CURRENT_CONTEXT.scope(context, async move {
                                match tool.execute(args).await {
                                    Ok(res) => res.to_string(),
                                    Err(e) => {
                                        println!("Error executing tool [{}]: {}", tool_name_for_scope, e);
                                        serde_json::json!({ "error": e.to_string() }).to_string()
                                    }
                                }
                            }).await
                        } else {
                            println!("Tool [{}] not found", tool_name);
                            serde_json::json!({ "error": format!("Tool {} not found", tool_name) }).to_string()
                        };

                        self.messages.push(ChatMessage::tool(
                            tool_id.clone(),
                            tool_name.clone(),
                            result_str.clone(),
                        ));

                        *index += 1;
                        *yielded_call = false;

                        return Some(Ok(AgentStreamEvent::ToolResult {
                            id: tool_id,
                            name: tool_name,
                            result: result_str,
                        }));
                    }
                }
                StreamState::Finished => {
                    return None;
                }
            }
        }
    }
}
