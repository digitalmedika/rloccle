use loccle::{FileStorage, GenerateOptions, Memory, MemoryConfig, agent};
use std::env;
use std::fs;
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // Load local .env file if available
    let _ = dotenvy::dotenv();

    // Check if real API key is set, otherwise default to a dummy for compilation purposes
    if env::var("OPENAI_API_KEY").is_err() {
        println!("Notice: OPENAI_API_KEY is not set. Setting a dummy key to run compilation.");
        unsafe {
            env::set_var("OPENAI_API_KEY", "sk-dummy-key-for-compilation");
        }
    }

    // 1. Configure local file storage for memory
    let memory_dir = "./target/memory_test";
    println!(
        "=== 1. Initializing Memory & FileStorage at {} ===",
        memory_dir
    );
    let storage = Arc::new(FileStorage::new(memory_dir));
    let memory = Memory::new(
        storage,
        MemoryConfig {
            last_messages: Some(10),
        },
    );

    // 2. Build the Agent with the Memory configuration
    println!("\n=== 2. Building Agent with Memory ===");
    let test_agent = agent! {
        id: "memory-agent",
        name: "Memory Agent",
        instructions: "You are a helpful assistant. Keep your responses short.",
        memory: memory,
    };

    println!("Agent ID:   {}", test_agent.id());
    println!("Agent Name: {}", test_agent.name());

    // 3. Define the thread options
    let thread_id = "thread-billy-1";
    let options = GenerateOptions::new()
        .thread_id(thread_id)
        .resource_id("billy-user-1");

    println!("\n=== 3. First Turn: Introducing user ===");
    let prompt1 = "Hi, my name is Billy.";
    println!("User: {}", prompt1);

    match test_agent
        .generate_with_options(prompt1, options.clone())
        .await
    {
        Ok(res) => {
            println!("Agent: {}", res);
        }
        Err(e) => {
            println!(
                "Execution Note: Could not get response (likely due to invalid API key): {}",
                e
            );
            return Ok(());
        }
    }

    println!("\n=== 4. Second Turn: Asking follow-up question ===");
    let prompt2 = "What is my name?";
    println!("User: {}", prompt2);

    match test_agent
        .generate_with_options(prompt2, options.clone())
        .await
    {
        Ok(res) => {
            println!("Agent: {}", res);
        }
        Err(e) => {
            println!("Execution Note: {}", e);
        }
    }

    // 5. Read the generated JSON file to inspect the storage contents
    println!("\n=== 5. Inspecting Persisted Storage File ===");
    let json_path = format!("{}/{}.json", memory_dir, thread_id);
    if fs::metadata(&json_path).is_ok() {
        let content = fs::read_to_string(&json_path)?;
        println!("Stored thread content:\n{}", content);
    } else {
        println!(
            "No storage file found at {}. (Maybe mock key was used and no API call was made?)",
            json_path
        );
    }

    // Cleanup the test memory dir
    let _ = fs::remove_dir_all(memory_dir);

    Ok(())
}
