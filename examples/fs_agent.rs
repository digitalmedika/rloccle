use loccle::{agent, tools::all_fs_tools};
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

    // 1. Get all built-in filesystem tools
    let fs_tools = all_fs_tools();

    // 2. Create the Filesystem Coding Agent with all the filesystem tools
    println!("=== Creating Filesystem Agent ===");
    let coder_agent = agent! {
        id: "fs-coder-agent",
        name: "Filesystem Coding Assistant",
        instructions: "You are a helpful coding assistant with access to the local filesystem. You can list directories, read/write files, glob patterns, and search/grep inside files. Use these tools to perform tasks requested by the user.",
        tools: fs_tools,
    };

    println!("Agent Name: {}", coder_agent.name());
    println!("Agent Model: {}", coder_agent.model());

    // 3. Ask the agent to search for Cargo.toml, read it, and write a summary
    println!("\n=== Prompting Agent to inspect project and write a summary ===");
    let prompt = "Find the Cargo.toml file using glob, read its contents, and write a summary of dependencies to a new file named project_summary.txt.";
    println!("User: {}", prompt);

    match coder_agent.generate(prompt).await {
        Ok(response) => {
            println!("\n[LLM Final Response]:\n{}", response);
        }
        Err(e) => {
            println!("\n[Execution Note]: Could not get response: {}", e);
            println!("This is expected if your API key or model configuration is invalid.");
        }
    }
}
