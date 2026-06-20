# Loccle-rs

An ergonomic, lightweight AI Agent framework for Rust, heavily inspired by [Mastra AI](https://mastra.ai/). It allows you to build, configure, and execute LLM agents using a highly declarative syntax that closely mirrors TypeScript object definitions.

## Features

- 🚀 **TypeScript-like Syntax**: Define agents using the declarative `agent!` macro, reducing Rust builder boilerplate.
- 🔌 **OpenAI-Compatible Client**: Works out-of-the-box with OpenAI, OpenRouter, Ollama, LM Studio, Cherry Studio, and other compatible gateways.
- ⚙️ **Automatic Env Resolution**: Loads API keys (`OPENAI_API_KEY`), model names (`OPENAI_MODEL`/`AGENT_MODEL`), and base URLs (`OPENAI_BASE_URL`/`OPENAI_API_BASE`) automatically from your `.env` file.
- 🛠️ **Smart Route Normalization**: Automatically ensures API endpoints correctly map to `/v1/chat/completions`, even if you omit the version suffix in the URL.
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
- [ ] **Tools & Tool Calling**: Implement a decorator or macro system to register local Rust functions as LLM-callable tools.
- [ ] **Workflows**: Add step-based async DAG workflows with state transmission, retry mechanisms, and branching logic.
- [ ] **Memory & Threads**: Implement short-term state storage and long-term conversation thread management.
- [ ] **Vector DB Integrations**: Support native semantic search and RAG indexing.
