use loccle::agent;
use std::env;

#[tokio::main]
async fn main() {
    // Load local .env file if available
    let _ = dotenvy::dotenv();

    // Check if real API key is set, otherwise default to a dummy for compilation and basic running purposes
    if env::var("OPENAI_API_KEY").is_err() {
        println!("Notice: OPENAI_API_KEY is not set. Setting a dummy key to run compilation.");
        unsafe {
            env::set_var("OPENAI_API_KEY", "sk-dummy-key-for-compilation");
        }
    }

    // 1. Defining an agent using the agent! macro (mimicking TypeScript style)
    println!("=== Agent 1: standard OpenAI Agent ===");
    let test_agent = agent! {
        id: "test-agent",
        name: "Test Agent",
        instructions: "You are a helpful assistant. Keep your answers brief.",
        model: "openai/gpt-4o",
    };

    println!("Agent ID:          {}", test_agent.id());
    println!("Agent Name:        {}", test_agent.name());
    println!("Agent Instructions: {}", test_agent.instructions());
    println!("Agent Model:       {}", test_agent.model());

    // 2. Defining an agent with a custom base_url (like Ollama, LM Studio, or LocalAI)
    println!("\n=== Agent 2: Custom Base URL / Local Agent ===");
    let local_agent = agent! {
        id: "local-agent",
        name: "Local Agent",
        instructions: "You are a helpful local assistant.",
        model: "ollama/llama3",
        base_url: "http://localhost:11434/v1", // custom endpoint override
    };

    println!("Agent ID:          {}", local_agent.id());
    println!("Agent Name:        {}", local_agent.name());
    println!("Agent Model:       {}", local_agent.model());

    // 3. Defining an agent that loads its model dynamically from the environment
    println!("\n=== Agent 3: Env-Configured Agent ===");
    let env_agent = agent! {
        id: "env-agent",
        name: "Env Agent",
        instructions: "You are configured entirely via env vars. Keep your answers brief.",
    };
    println!("Agent ID:          {}", env_agent.id());
    println!("Agent Name:        {}", env_agent.name());
    println!("Agent Model:       {}", env_agent.model());

    // 4. Let's make an execution attempt with Agent 3 (Env-Configured)
    println!("\n=== Prompting Agent 3 (Env-Configured) ===");

    match env_agent.generate("Hello, explain Rust in one short sentence.").await {
        Ok(response) => {
            println!("\n[LLM Response]:\n{}", response);
        }
        Err(e) => {
            println!("\n[Execution Note]: Could not get response: {}", e);
            println!("\nTo run the agent with a real provider, set your environment variables:");
            println!("  $env:OPENAI_API_KEY=\"your_real_key\"");
            println!("  # Optionals if you're using a custom OpenAI-compatible proxy/model:");
            println!("  $env:OPENAI_BASE_URL=\"https://api.openai.com/v1\"");
            println!("  $env:OPENAI_API_BASE=\"https://api.openai.com/v1\"");
        }
    }
}
