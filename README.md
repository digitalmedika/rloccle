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

---

## Future Roadmap

This framework aims to build out full agentic capabilities in Rust:
- [x] **Tools & Tool Calling**: Type-safe automatic schema generation (`schemars` + `serde`) and autonomous reasoning/execution loop.
- [ ] **Workflows**: Add step-based async DAG workflows with state transmission, retry mechanisms, and branching logic.
- [ ] **Memory & Threads**: Implement short-term state storage and long-term conversation thread management.
- [ ] **Vector DB Integrations**: Support native semantic search and RAG indexing.
