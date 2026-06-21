use loccle::{AgentStreamEvent, Tool, agent, create_tool};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::env;
use std::io::Write;
use std::sync::Arc;

#[derive(JsonSchema, Deserialize, Debug)]
pub struct WeatherInput {
    /// The city and state, e.g. San Francisco, CA
    pub location: String,
}

#[derive(JsonSchema, Serialize, Debug)]
pub struct WeatherOutput {
    pub weather: String,
}

#[tokio::main]
async fn main() {
    // Load local .env file if available
    let _ = dotenvy::dotenv();

    // Check if real API key is set, otherwise default to a dummy
    if env::var("OPENAI_API_KEY").is_err() {
        println!("Notice: OPENAI_API_KEY is not set. Setting a dummy key to run compilation.");
        unsafe {
            env::set_var("OPENAI_API_KEY", "sk-dummy-key-for-compilation");
        }
    }

    // Create a weather tool with automatic schema generation
    let weather_tool = create_tool::<WeatherInput, WeatherOutput, _, _>(
        "weather-tool",
        "Fetches weather for a location",
        |args| async move {
            let weather_info = format!("Weather in {} is sunny and 72°F", args.location);
            Ok(WeatherOutput {
                weather: weather_info,
            })
        },
    );

    // Define an Agent and register our weather_tool
    println!("=== Creating Weather Agent with Streaming ===");
    let weather_agent = agent! {
        id: "weather-agent",
        name: "Weather Assistant",
        instructions: "You are a helpful assistant. Use the weather-tool to retrieve weather details. Summarize the details to the user.",
        tools: vec![Arc::new(weather_tool) as Arc<dyn Tool>],
    };

    let prompt = "What is the weather in Tokyo?";
    println!("User: {}", prompt);
    println!("--- Streaming Response ---");

    match weather_agent.stream(prompt).await {
        Ok(mut stream) => {
            while let Some(event_res) = stream.next().await {
                match event_res {
                    Ok(event) => match event {
                        AgentStreamEvent::TextDelta(text) => {
                            print!("{}", text);
                            let _ = std::io::stdout().flush();
                        }
                        AgentStreamEvent::ReasoningDelta(reasoning) => {
                            print!("[Reasoning: {}]", reasoning);
                            let _ = std::io::stdout().flush();
                        }
                        AgentStreamEvent::ToolCall {
                            id,
                            name,
                            arguments,
                        } => {
                            println!(
                                "\n[Stream Event] Tool Call: {} (ID: {}) with args: {}",
                                name, id, arguments
                            );
                        }
                        AgentStreamEvent::ToolResult { id, name, result } => {
                            println!(
                                "[Stream Event] Tool Result from {} (ID: {}): {}",
                                name, id, result
                            );
                        }
                        AgentStreamEvent::Finish { finish_reason, .. } => {
                            println!("\n[Stream Event] Finish: {:?}", finish_reason);
                        }
                    },
                    Err(e) => {
                        println!("\nError in stream: {}", e);
                    }
                }
            }
        }
        Err(e) => {
            println!("Failed to start stream: {}", e);
        }
    }
}
