use loccle::{agent, InMemoryStorage, GenerateOptions, Memory, MemoryConfig, TaskSignalProvider, Task, Storage, AgentStreamEvent};
use std::env;
use std::io::{self, Write};
use std::sync::Arc;

// Helper to redraw the Terminal UI
async fn draw_ui(
    storage: &Arc<InMemoryStorage>,
    thread_id: &str,
    accumulated_text: &str,
    current_action: &str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // \x1B[H moves the cursor to the top-left corner
    // \x1B[J clears from the cursor to the end of the screen
    print!("\x1B[H\x1B[J");
    let _ = io::stdout().flush();

    println!("\x1B[1;36m==================================================\x1B[0m");
    println!("\x1B[1;35m📋 VIBE CODING - AGENT WORKFLOW DASHBOARD\x1B[0m");
    println!("\x1B[1;36m==================================================\x1B[0m");

    // Load tasks from storage
    if let Some(session) = storage.get_thread(thread_id).await? {
        if let Some(tasks_val) = session.state.get("tasks") {
            let tasks: Vec<Task> = serde_json::from_value(tasks_val.clone())?;
            for task in &tasks {
                let status_icon = match task.status.as_str() {
                    "completed" => "\x1B[1;32m[✓]\x1B[0m", // Green checkmark
                    "in_progress" => "\x1B[1;33m[▶]\x1B[0m", // Yellow play icon
                    _ => "\x1B[90m[ ]\x1B[0m",            // Gray empty box
                };
                
                let active_text = if task.status == "in_progress" {
                    format!(" \x1B[3m(Active: {})\x1B[0m", task.active_form)
                } else {
                    "".to_string()
                };

                println!("  {} {} {}", status_icon, task.content, active_text);
            }
        } else {
            println!("  \x1B[90m(No tasks initialized yet... Planning...)\x1B[0m");
        }
    } else {
        println!("  \x1B[90m(Initializing thread session...)\x1B[0m");
    }

    println!("\x1B[1;36m==================================================\x1B[0m");
    if !current_action.is_empty() {
        println!("⚡ \x1B[1;33mCurrent Action:\x1B[0m {}", current_action);
        println!("\x1B[1;36m--------------------------------------------------\x1B[0m");
    }
    println!("\n\x1B[1mAgent output:\x1B[0m");
    println!("{}", accumulated_text);
    let _ = io::stdout().flush();

    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // Load local .env file
    let _ = dotenvy::dotenv();

    // Check for API Key
    if env::var("OPENAI_API_KEY").is_err() {
        println!("\x1B[1;31mError: OPENAI_API_KEY environment variable is not set.\x1B[0m");
        println!("Please add it to your .env file or export it before running this example.");
        return Ok(());
    }

    // Clear screen initially
    print!("\x1B[2J\x1B[H");
    let _ = io::stdout().flush();

    println!("\x1B[1;35mWelcome to the Loccle TUI Agent Checklist!\x1B[0m");
    print!("\nEnter a goal for the agent (e.g., 'Plan to write a simple calculator in Rust'): ");
    let _ = io::stdout().flush();

    let mut user_input = String::new();
    io::stdin().read_line(&mut user_input)?;
    let prompt = user_input.trim();
    if prompt.is_empty() {
        println!("Empty input. Exiting.");
        return Ok(());
    }

    let storage = Arc::new(InMemoryStorage::new());
    let memory = Memory::new(storage.clone(), MemoryConfig { last_messages: Some(10) });

    let test_agent = agent! {
        id: "tui-agent",
        name: "TUI Coordinator",
        instructions: "You are an agent that plans and coordinates tasks. Always use task tools to track your progress. First call `task_write` to plan the tasks. Then update their status using `task_update` and `task_complete` as you progress through each task step-by-step.",
        memory: memory,
        signal: TaskSignalProvider::new(),
    };

    let thread_id = "tui-thread-session";
    let options = GenerateOptions::new()
        .thread_id(thread_id)
        .resource_id("tui-user");

    storage.create_thread(thread_id, Some("tui-user".to_string())).await?;

    let mut accumulated_text = String::new();
    let mut current_action = String::from("Starting stream...");

    // Render initial UI
    draw_ui(&storage, thread_id, &accumulated_text, &current_action).await?;

    match test_agent.stream_with_options(prompt, options).await {
        Ok(mut stream) => {
            while let Some(event_res) = stream.next().await {
                match event_res {
                    Ok(event) => match event {
                        AgentStreamEvent::TextDelta(text) => {
                            accumulated_text.push_str(&text);
                            draw_ui(&storage, thread_id, &accumulated_text, &current_action).await?;
                        }
                        AgentStreamEvent::ReasoningDelta(reasoning) => {
                            current_action = format!("Thinking: {}", reasoning);
                            draw_ui(&storage, thread_id, &accumulated_text, &current_action).await?;
                        }
                        AgentStreamEvent::ToolCall { name, arguments, .. } => {
                            current_action = format!("Running tool `{}` with args: {}", name, arguments);
                            draw_ui(&storage, thread_id, &accumulated_text, &current_action).await?;
                        }
                        AgentStreamEvent::ToolResult { name, .. } => {
                            current_action = format!("Tool `{}` finished execution", name);
                            draw_ui(&storage, thread_id, &accumulated_text, &current_action).await?;
                        }
                        AgentStreamEvent::Finish { .. } => {
                            current_action = "Completed!".to_string();
                            draw_ui(&storage, thread_id, &accumulated_text, &current_action).await?;
                        }
                    }
                    Err(e) => {
                        current_action = format!("Error: {}", e);
                        draw_ui(&storage, thread_id, &accumulated_text, &current_action).await?;
                    }
                }
            }
        }
        Err(e) => {
            println!("Failed to start stream: {}", e);
        }
    }

    println!("\n\x1B[1;32m=== Agent Finished Processing! ===\x1B[0m");

    Ok(())
}
