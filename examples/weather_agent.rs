use loccle::{Tool, agent, create_tool};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::env;
use std::sync::Arc;

// Define the input schema structure
#[derive(JsonSchema, Deserialize, Debug)]
pub struct WeatherInput {
    /// The city and state, e.g. San Francisco, CA
    pub location: String,
}

// Define the output schema structure
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

    // 1. Create a weather tool with automatic schema generation
    let weather_tool = create_tool::<WeatherInput, WeatherOutput, _, _>(
        "weather-tool",
        "Fetches weather for a location",
        |args| async move {
            println!(
                "[Tool executing] weather-tool: fetching weather for {}",
                args.location
            );

            // Simulating weather fetch
            let weather_info = format!("Weather in {} is sunny and 72°F", args.location);
            Ok(WeatherOutput {
                weather: weather_info,
            })
        },
    );

    // Print the generated JSON schema to verify it works
    println!("=== Generated Input Schema ===");
    println!(
        "{}",
        serde_json::to_string_pretty(&weather_tool.input_schema()).unwrap()
    );
    println!("=== Generated Output Schema ===");
    println!(
        "{}",
        serde_json::to_string_pretty(&weather_tool.output_schema().unwrap()).unwrap()
    );

    // 2. Define an Agent and register our weather_tool
    println!("\n=== Creating Weather Agent ===");
    let weather_agent = agent! {
        id: "weather-agent",
        name: "Weather Assistant",
        instructions: "You are a helpful assistant. Use the weather-tool to retrieve weather details. Summarize the details to the user.",
        tools: vec![Arc::new(weather_tool) as Arc<dyn Tool>],
    };

    println!("Agent Name: {}", weather_agent.name());
    println!("Agent Model: {}", weather_agent.model());

    // 3. Run the reasoning loop using a mock prompt
    println!("\n=== Prompting Agent to use weather-tool ===");
    let prompt = "What is the weather in Tokyo?";
    println!("User: {}", prompt);

    match weather_agent.generate(prompt).await {
        Ok(response) => {
            println!("\n[LLM Final Response]:\n{}", response);
        }
        Err(e) => {
            println!("\n[Execution Note]: Could not get response: {}", e);
            println!("This is expected if your API key or model configuration is invalid.");
        }
    }
}
