# Agent Developer Guide (AGENTS.md)

Welcome to the **Loccle** repository! This document serves as a comprehensive reference guide for AI agents and human developers working in this codebase. It highlights the overall architecture, control/data flow, patterns, conventions, essential commands, and subtle gotchas to prevent common development errors.

---

## 1. Project Architecture & Components

The repository is organized into a modular workspace containing two main crates:

```
D:/project/rloccle/
├── Cargo.toml            # Workspace config & dependencies
├── src/                  # Core Library: loccle
│   ├── lib.rs            # Module definitions & macro exports
│   ├── agent.rs          # Agent model, builder, and execution loops
│   ├── tool.rs           # Tool & TypedTool traits and builders
│   ├── tools/            # Built-in Tool suites (fs, system, task)
│   ├── memory.rs         # Conversation memory configuration
│   ├── storage.rs        # InMemory / File storage adapters
│   └── openai.rs         # OpenAI API Client & schema mappings
├── vibe-tui/             # TUI Client: ratatui & tachyonfx interface
│   ├── Cargo.toml
│   └── src/main.rs       # TUI application loop, event handler, renderer
└── examples/             # Executable scenarios demonstrating capabilities
```

### Core Concepts

1. **`Agent` & `AgentBuilder` (`src/agent.rs`)**:
   Built using the builder pattern, configured with an ID, name, instructions, model (defaults to `openai/gpt-4o`), and associated tools/memory.
2. **`agent!` macro (`src/lib.rs`)**:
   A declarative Rust macro that mimics TypeScript-style declarative definitions for instantiating Agents easily.
3. **`Tool` & `TypedTool<I, O, F>` (`src/tool.rs`)**:
   `Tool` is the core trait. `TypedTool` leverages `schemars::JsonSchema` to automatically generate JSON-schemas for function parameters directly from Rust structs. `create_tool` acts as a developer-friendly constructor.
4. **`Memory` & `Storage` (`src/memory.rs` / `src/storage.rs`)**:
   Stores message histories and thread states. Support is provided for `InMemoryStorage` (and can be extended to file/persistent engines).

---

## 2. Key Flows & Patterns

### 2.1. Task Signal Injection & Tracking
When `task_signal_provider` is enabled on an Agent (`signal: TaskSignalProvider::new()`), and the execution context contains an active thread, **Loccle automatically injects the active tasks** as an XML structure into the agent's system message instructions:

```xml
<tasks>
  <task id="task-1" status="pending" activeForm="Working on: ...">Task content</task>
</tasks>
```

The agent is expected to manipulate these tasks using:
* `task_write` - define or overwrite the task list.
* `task_update` - change task status, active form description, or content.
* `task_complete` - mark a task as done.
* `task_check` - inspect current list and see if everything is finished.

### 2.2. Tokio Task-Local Execution Context
Because tools are executed inside independent async futures, context like the current `thread_id` and `Memory` isn't passed as explicit parameters to tool arguments. Instead, Loccle uses **`tokio::task_local!`** context propagation.

Stateful tools (like task tracking) access context through:
```rust
let ctx = CURRENT_CONTEXT.try_with(|c| c.clone())?;
```

Any tool execution loop MUST be scoped within the context:
```rust
CURRENT_CONTEXT.scope(execution_context, async move {
    // execute agent loop/tools
}).await;
```

---

## 3. Essential Commands

Loccle is a standard Rust Cargo workspace. The following commands are standard:

| Task | Command |
| :--- | :--- |
| **Build all workspace crates** | `cargo build --all` |
| **Run the interactive TUI** | `cargo run --package vibe-tui` |
| **Run tests** | `cargo test --all` |
| **Run examples** | `cargo run --example <example_name>` |

### Available Examples (in `/examples`):
* `simple_agent` - Basic prompt/response agent.
* `streaming_agent` - Text & reasoning streaming event patterns.
* `fs_agent` - Agent with filesystem coding tools.
* `system_agent` - Running bash/system commands synchronously or in background.
* `task_agent` - Structuring work plans and using task tracking.
* `weather_agent` - Integration with custom mock tools.
* `tui_agent` / `memory_agent` - Stateful context usage.

---

## 4. Built-in Tools Reference

Loccle comes equipped with several high-performance tool suites designed for agents:

### 4.1. Filesystem Suite (`src/tools/fs.rs`)
* `read_file` - Read complete text file.
* `read_file_range` - Read **1-based inclusive** range of lines (prevents context overload on huge files).
* `write_file` - Write complete file, automatically creating parent directories.
* `replace_in_file` - Find exact text and replace it. Offers an optional `expected_replacements` parameter for structural validation.
* `replace_lines` - Overwrites an inclusive 1-based range of lines.
* `list_dir` - List directory contents.
* `grep` / `glob` - Search for patterns or glob-matched file paths.
* `delete` / `file_stat` / `mkdir` - File metadata and directory utilities.

### 4.2. System Suite (`src/tools/system.rs`)
* `execute_command` - Execute processes. Supports `background` flag to run in background and immediately yield a PID.
* `get_process_output` - Retrieve stdout/stderr and status of background PID.
* `kill_process` - Terminate background PID.

### 4.3. Task Suite (`src/tools/task.rs`)
* `task_write` / `task_update` / `task_complete` / `task_check` - Manipulate structured lists.

---

## 5. Development Gotchas & Conventions

Avoid these common pitfalls when developing or modifying code:

* **Line Range Indexing**: All range-based tools (`read_file_range` and `replace_lines`) use **1-based inclusive** indexing. Passing `start_line: 0` is an invalid argument and will return an error.
* **Environment Variable Safety**: Setting `LOCCLE_SILENT=true` is critical when running inside interactive environments (like the TUI) to prevent standard library logging or trace logs from corrupting terminal render grids.
* **Task Context Bound**: Tools manipulating task lists will return an error if called without an active `CURRENT_CONTEXT` thread scope.
* **Replace-in-file Safety**: When updating source code using `replace_in_file`, always supply the `expected_replacements` parameter when possible to ensure the agent doesn't perform accidental partial matches or overwrite wrong occurrences.
* **API Key Fallback**: The codebase expects `OPENAI_API_KEY` to be present. In examples, if it is missing, a compilation dummy key (`sk-dummy-key-for-compilation`) is safely set in the process env so that the crate compiles and can be verified.
* **Adding New Tools**: When writing a new tool, define input/output structs deriving `schemars::JsonSchema`, `serde::Deserialize`/`serde::Serialize`. Use `create_tool("tool_name", "Description", |args| async { ... })` to generate a `TypedTool` instantly.
