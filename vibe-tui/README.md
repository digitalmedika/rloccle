# Vibe-TUI: Interactive Agent Orchestration Terminal UI

`vibe-tui` is a terminal-based interface designed to monitor and run `loccle-rs` AI agents interactively. Built with **Ratatui** and **Crossterm**, it provides a real-time visualization of streaming agent reasoning, text generation, and tool invocations.

---

## 🎨 UI Architecture & Layout

The interface is divided into two distinct vertical sections:
1. **Upper Panel (Scroll View)**: 
   - Displays a scrollable log of the conversation.
   - Shows streaming text delta, reasoning/thinking traces, tool calls, and tool execution results sequentially.
   - Updates dynamically as events are received.
2. **Lower Panel (Input Prompt)**:
   - Contains a text prompt field.
   - Locked while the agent is actively working/streaming.
   - Accepts user input and triggers execution upon pressing `Enter`.

---

## 🛠 Features

- **Sequential Event Feed**: Real-time display of text deltas and tool results as they are returned by the agent loop.
- **Visual Log Styling**:
  - 👤 **User Prompts** are highlighted in bold blue.
  - 🤖 **Agent Text** is rendered in green.
  - 🧠 **Thinking Traces** are styled in gray italics (supporting reasoning models like DeepSeek-R1/o1/o3-mini).
  - 🔧 **Tool Calls** show argument bindings in yellow.
  - ✅ **Tool Results** show return payloads in cyan.
- **Accurate Auto-Scroll & Scrollback Navigation**:
  - Custom pixel-perfect word-wrapping based on the current window width.
  - Keyboard scrollback controls: `Up Arrow` / `Down Arrow` to scroll by single lines, and `Page Up` / `Page Down` to scroll by blocks.
  - Auto-scroll is automatically re-enabled when sending a new prompt.
- **Clean Exit**: Clean terminal recovery upon pressing `Esc`.

---

## ⌨️ Controls

- **`Char Keys` / `Backspace`**: Edit your prompt query (when agent is idle).
- **`Enter`**: Submit prompt and start the agent stream.
- **`Arrow Up` / `Arrow Down`**: Scroll up or down by 1 line in the history panel (disables auto-scroll).
- **`Page Up` / `Page Down`**: Scroll up or down by 10 lines in the history panel (disables auto-scroll).
- **`Esc`**: Exit the TUI and restore the terminal to normal mode.

---

## 🚀 Getting Started

### 1. Configuration
Ensure you have a `.env` file set up in the root workspace (`d:\project\rloccle\.env`) with your API details:
```ini
OPENAI_API_KEY=your-api-key
OPENAI_MODEL=model-name
OPENAI_BASE_URL=https://api.openai.com/v1 # or custom endpoint
```

### 2. Execution
Run the TUI binary from the `vibe-tui` directory or the root directory:
```bash
# Run directly from vibe-tui directory
cargo run

# Or run from workspace root
cargo run -p vibe-tui
```
