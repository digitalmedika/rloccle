use loccle::{
    agent,
    tools::{all_fs_tools, all_system_tools},
};
use std::env;

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

    // 1. Collect all filesystem and system tools
    let mut tools = all_fs_tools();
    tools.extend(all_system_tools());

    // 2. Create the System Agent
    println!("=== Creating System Agent ===");
    let system_agent = agent! {
        id: "system-agent",
        name: "System Operations Assistant",
        instructions: "You are a system operations assistant. You have access to filesystem tools (like mkdir, delete, file_stat) and command execution tools (like execute_command). Perform tasks requested by the user.",
        tools: tools,
    };

    println!("Agent Name: {}", system_agent.name());
    println!("Agent Model: {}", system_agent.model());

    // 3. Ask the agent to execute cargo --version, make a temporary folder, check its stats, and clean it up
    println!("\n=== Prompting Agent to run commands and filesystem operations ===");
    let prompt = "Please do the following tasks: \
                  1. Run 'cargo' with argument '--version' using execute_command. \
                  2. Create a temporary folder named 'temp_agent_dir' using mkdir. \
                  3. Check the stats of 'temp_agent_dir' using file_stat. \
                  4. Delete 'temp_agent_dir' using delete. \
                  Provide a brief summary of what happened.";

    println!("User: {}", prompt);

    match system_agent.generate(prompt).await {
        Ok(response) => {
            println!("\n[LLM Final Response]:\n{}", response);
        }
        Err(e) => {
            println!("\n[Execution Note]: Could not get response: {}", e);
            println!("This is expected if your API key or model configuration is invalid.");
        }
    }
}
