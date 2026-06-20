use loccle::{agent, InMemoryStorage, GenerateOptions, Memory, MemoryConfig, TaskSignalProvider, Task, Storage, AgentStreamEvent};
use std::env;
use std::io::Write;
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // Load local .env file if available
    let _ = dotenvy::dotenv();

    // Check if real API key is set
    let api_key_is_dummy = env::var("OPENAI_API_KEY").is_err() || env::var("OPENAI_API_KEY").unwrap() == "sk-dummy-key-for-compilation";
    if api_key_is_dummy {
        println!("Notice: OPENAI_API_KEY is not set or is dummy. Setting dummy key for compilation check.");
        unsafe {
            env::set_var("OPENAI_API_KEY", "sk-dummy-key-for-compilation");
        }
    }

    println!("=== 1. Initializing Memory & Storage ===");
    let storage = Arc::new(InMemoryStorage::new());
    let memory = Memory::new(storage.clone(), MemoryConfig { last_messages: Some(10) });

    println!("\n=== 2. Building Agent with Memory and TaskSignalProvider ===");
    let test_agent = agent! {
        id: "task-coordinator-agent",
        name: "Task Coordinator",
        instructions: "You are an agent that plans and coordinates tasks. Always use task tools to track your progress. When given a request, first call `task_write` to create a task list. Then execute the tasks step-by-step, calling `task_update` or `task_complete` to mark your progress as you work on each item. Make sure you complete all tasks.",
        memory: memory,
        signal: TaskSignalProvider::new(),
    };

    println!("Agent ID:                  {}", test_agent.id());
    println!("Agent Name:                {}", test_agent.name());

    if api_key_is_dummy {
        println!("\n[INFO] Skipping live LLM run because a real OPENAI_API_KEY is not configured.");
        println!("Configure OPENAI_API_KEY in your .env or environment to run the live task tracking stream.");
        println!("Compilation check successful.");
        return Ok(());
    }

    let thread_id = "thread-agentic-tasks";
    let options = GenerateOptions::new()
        .thread_id(thread_id)
        .resource_id("billy-user");

    // Create the thread session in storage
    storage.create_thread(thread_id, Some("billy-user".to_string())).await?;

    println!("\n=== 3. Starting live Agent Task Orchestration Stream ===");
    let prompt = "Please create a task list to research top 3 Rust databases, compare them, and verify the results. Then go ahead and execute each task one by one, updating the task status using the tools (e.g. setting them to in_progress and then completed) as you proceed.";
    
    println!("User prompt: {}\n", prompt);
    println!("--- Streaming Response ---");

    match test_agent.stream_with_options(prompt, options).await {
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
                        AgentStreamEvent::ToolCall { id, name, arguments } => {
                            println!("\n\n[Agent Call Tool] => {} (ID: {})", name, id);
                            println!("Arguments: {}", arguments);
                        }
                        AgentStreamEvent::ToolResult { id, name, result } => {
                            println!("[Tool Result] from {} (ID: {}):", name, id);
                            println!("{}\n", result);
                        }
                        AgentStreamEvent::Finish { finish_reason, .. } => {
                            println!("\n[Stream Event] Finish Reason: {:?}", finish_reason);
                        }
                    }
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

    // Print final stored tasks
    println!("\n=== 4. Stored Tasks in ThreadState Storage ===");
    if let Some(session) = storage.get_thread(thread_id).await? {
        if let Some(tasks_val) = session.state.get("tasks") {
            let tasks: Vec<Task> = serde_json::from_value(tasks_val.clone())?;
            println!("Final Thread state tasks list:");
            for task in tasks {
                println!("- [{}] ID: {}, Status: {}, ActiveForm: '{}'", task.content, task.id, task.status, task.active_form);
            }
        } else {
            println!("No tasks found in thread session state.");
        }
    }

    Ok(())
}
