# Loccle-rs

An ergonomic, lightweight AI Agent framework for Rust, heavily inspired by [Mastra AI](https://mastra.ai/). It allows you to build, configure, and execute LLM agents using a highly declarative syntax that closely mirrors TypeScript object definitions.

## Features

- 🚀 **TypeScript-like Syntax**: Define agents using the declarative `agent!` macro, reducing Rust builder boilerplate.
- 🔌 **OpenAI-Compatible Client**: Works out-of-the-box with OpenAI, OpenRouter, Ollama, LM Studio, Cherry Studio, and other compatible gateways.
- ⚙️ **Automatic Env Resolution**: Loads API keys (`OPENAI_API_KEY`), model names (`OPENAI_MODEL`/`AGENT_MODEL`), and base URLs (`OPENAI_BASE_URL`/`OPENAI_API_BASE`) automatically from your `.env` file.
- 🛠️ **Type-Safe Struct-based Tools**: Define LLM tools using standard Rust structs. The JSON Schema is automatically generated via `schemars` (including parameter descriptions from doc comments), and Serde validates types at runtime.
- 🔄 **Autonomous Reasoning Loop**: The agent automatically runs an execution loop to execute tools called by the LLM and feeds results back in real-time.
- 🗂️ **Built-in Filesystem Tools**: Packaged out-of-the-box tools for file operations: `read_file`, `write_file`, `list_dir`, `grep`, `glob`, `delete`, `file_stat`, and `mkdir`.
- 💻 **Built-in System Tools**: Process and shell execution tools: `execute_command` (sync/background), `get_process_output`, and `kill_process`.
- 🧹 **SSE Response Sanitization**: Gracefully handles and strips trailing streaming endings (e.g., `data: [DONE]`) returned by certain model proxies, preventing JSON parsing crashes.

---

## Syntax Comparison

### TypeScript (Mastra)
```typescript
import { Agent } from '@mastra/core/agent'

export const testAgent = new Agent({
  id: 'test-agent',
  name: 'Test Agent',
  instructions: 'You are a helpful assistant.',
  model: 'openai/gpt-4o',
})
```

### Rust (Loccle-rs)
```rust
use loccle::agent;

let test_agent = agent! {
    id: "test-agent",
    name: "Test Agent",
    instructions: "You are a helpful assistant.",
    model: "openai/gpt-4o",
};
```

---

## Using Tools

You can define tools using input and output structs, and pass them to agents. The agent will automatically call them when requested by the model:

```rust
use loccle::{agent, create_tool, Tool};
use serde::{Deserialize, Serialize};
use schemars::JsonSchema;
use std::sync::Arc;

// 1. Define input/output structs
#[derive(JsonSchema, Deserialize)]
pub struct WeatherInput {
    /// The city and state, e.g. San Francisco, CA
    pub location: String,
}

#[derive(JsonSchema, Serialize)]
pub struct WeatherOutput {
    pub weather: String,
}

// 2. Define the tool with auto schema generation
let weather_tool = create_tool::<WeatherInput, WeatherOutput, _, _>(
    "weather-tool",
    "Fetches weather for a location",
    |args| async move {
        let weather_info = format!("Weather in {} is sunny and 72°F", args.location);
        Ok(WeatherOutput { weather: weather_info })
    }
);

// 3. Register tool with agent!
let weather_agent = agent! {
    id: "weather-agent",
    name: "Weather Assistant",
    instructions: "Use weather-tool to get current weather data.",
    tools: vec![Arc::new(weather_tool) as Arc<dyn Tool>],
};
```

---

## Streaming Responses

Similar to Mastra, `loccle-rs` supports real-time streaming response delivery using an async event stream. You can call `.stream(prompt)` on an agent to receive an `AgentStream` and iterate over `AgentStreamEvent` items chunk-by-chunk using `.next().await`.

The streaming parser automatically runs the tool execution reasoning loops and emits intermediate events so your client application can distinguish what the agent is doing in real-time.

### Event Types (`AgentStreamEvent`)
- `TextDelta(String)`: Incremental assistant response text.
- `ReasoningDelta(String)`: Incremental reasoning/thinking trace (e.g., for o1/o3-mini/DeepSeek models).
- `ToolCall`: Emitted when the agent decides to invoke a tool, containing the tool ID, name, and final arguments.
- `ToolResult`: Emitted when the tool execution completes, containing the tool ID, name, and result string.
- `Finish`: Emitted when the agent finishes streaming.

### Usage Example

```rust
use loccle::{agent, AgentStreamEvent};
use std::io::Write;

#[tokio::main]
async fn main() {
    let assistant = agent! {
        id: "stream-agent",
        name: "Streaming Helper",
        instructions: "You are a helpful assistant.",
    };

    match assistant.stream("Explain Rust ownership in one short paragraph.").await {
        Ok(mut stream) => {
            while let Some(event_res) = stream.next().await {
                match event_res {
                    Ok(event) => match event {
                        AgentStreamEvent::TextDelta(text) => {
                            print!("{}", text);
                            let _ = std::io::stdout().flush();
                        }
                        AgentStreamEvent::ReasoningDelta(reasoning) => {
                            print!("[Thinking: {}]", reasoning);
                            let _ = std::io::stdout().flush();
                        }
                        AgentStreamEvent::ToolCall { name, .. } => {
                            println!("\n[Agent is running tool: {}]", name);
                        }
                        AgentStreamEvent::ToolResult { result, .. } => {
                            println!("\n[Tool returned: {}]", result);
                        }
                        AgentStreamEvent::Finish { .. } => {
                            println!("\n[Done]");
                        }
                    }
                    Err(e) => eprintln!("Error in stream: {}", e),
                }
            }
        }
        Err(err) => eprintln!("Failed to initiate stream: {}", err),
    }
}
```

To run the streaming demo:
```bash
cargo run --example streaming_agent
```

---

## Conversation Memory

`loccle-rs` includes a thread-based conversation memory system, allowing agents to retain context across turns by persisting message histories to a storage backend.

### Storage Providers
- **`InMemoryStorage`**: Stores conversation threads in-memory using a thread-safe `Mutex<HashMap>`. Ideal for testing or short-lived processes.
- **`FileStorage`**: Persists conversation threads as JSON files locally under a specified directory. Ensures that memory survives application restarts.

### Usage Example

```rust
use loccle::{agent, Memory, MemoryConfig, FileStorage, GenerateOptions};
use std::sync::Arc;

#[tokio::main]
async fn main() {
    // 1. Initialize local file-based storage
    let storage = Arc::new(FileStorage::new("./target/memory_dir"));
    
    // 2. Wrap it inside Memory with configuration options
    let memory = Memory::new(storage, MemoryConfig {
        last_messages: Some(10), // Limit history sent to the LLM to the last 10 messages
    });

    // 3. Register memory with the agent
    let chat_agent = agent! {
        id: "chat-agent",
        name: "Memory Companion",
        instructions: "You are a friendly chat companion.",
        memory: memory,
    };

    // 4. Set thread options for the conversation
    let options = GenerateOptions::new()
        .thread_id("unique-session-123")
        .resource_id("user-billy");

    // First Turn
    let res1 = chat_agent.generate_with_options("Hi, my name is Billy.", options.clone()).await.unwrap();
    println!("Agent: {}", res1);

    // Second Turn (Agent remembers the user's name from context)
    let res2 = chat_agent.generate_with_options("What is my name?", options.clone()).await.unwrap();
    println!("Agent: {}", res2);
}
```

#### Why `generate_with_options`?
In Rust, overloading method names or using optional arguments isn't supported natively. To prevent breaking existing stateless usages of `.generate(prompt)` and `.stream(prompt)`, `loccle-rs` provides `.generate_with_options(prompt, options)` and `.stream_with_options(prompt, options)`.

---

## Built-in Tools

Loccle comes with pre-packaged filesystem and system tools to make building agents easy.

### Filesystem Tools
Register all filesystem tools at once using the `all_fs_tools()` helper (includes `read_file`, `write_file`, `list_dir`, `grep`, `glob`, `delete`, `file_stat`, and `mkdir`):

```rust
use loccle::{agent, tools::all_fs_tools};

#[tokio::main]
async fn main() {
    let coder_agent = agent! {
        id: "coder-agent",
        name: "Coding Assistant",
        instructions: "You are a coding assistant. Use the filesystem tools to explore and edit this repository.",
        tools: all_fs_tools(),
    };
    
    // ...
}
```

### System Tools
Register all system process execution tools at once using the `all_system_tools()` helper (includes `execute_command`, `get_process_output`, and `kill_process`):

```rust
use loccle::{agent, tools::all_system_tools};

#[tokio::main]
async fn main() {
    let terminal_agent = agent! {
        id: "terminal-agent",
        name: "Terminal Runner",
        instructions: "Run system commands and monitor processes.",
        tools: all_system_tools(),
    };
    
    // ...
}
```

---

## Getting Started

### 1. Configuration (`.env`)
Create a `.env` file in the root of your project (see [.env.example](.env.example) for a template):

```ini
# Required for standard OpenAI models
OPENAI_API_KEY=your_openai_api_key_here

# Optional: Default model used if omitted in code
OPENAI_MODEL=openai/gpt-4o

# Optional: Custom provider endpoint
OPENAI_BASE_URL=https://api.openai.com/v1
```

### 2. Usage Example

Create a simple script:

```rust
use loccle::agent;

#[tokio::main]
async fn main() {
    // 1. Declare an agent loading config from environment
    let assistant = agent! {
        id: "math-assistant",
        name: "Math Helper",
        instructions: "You are a math tutor. Keep explanations simple and brief.",
    };

    // 2. Generate response
    match assistant.generate("What is the derivative of x^2?").await {
        Ok(reply) => println!("Assistant:\n{}", reply),
        Err(err) => eprintln!("Error: {}", err),
    }
}
```

---

## Running the Included Example

To run the built-in demonstration:

```bash
# 1. Copy the environment template
copy .env.example .env

# 2. Add your API credentials and preferred model to .env
# (e.g., OPENAI_API_KEY, OPENAI_MODEL, OPENAI_BASE_URL)

# 3. Run the example
cargo run --example simple_agent
```

To run the new memory persistence demonstration:
```bash
cargo run --example memory_agent
```

---

## Future Roadmap

This framework aims to build out full agentic capabilities in Rust:
- [x] **Tools & Tool Calling**: Type-safe automatic schema generation (`schemars` + `serde`) and autonomous reasoning/execution loop.
- [x] **Streaming Responses**: Real-time response deltas (text & reasoning) and step-by-step tool execution streams.
- [ ] **Workflows**: Add step-based async DAG workflows with state transmission, retry mechanisms, and branching logic.
- [x] **Memory & Threads**: Implement short-term state storage and long-term conversation thread management.
- [ ] **Vector DB Integrations**: Support native semantic search and RAG indexing.
