use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEvent},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use loccle::{
    AgentStreamEvent, GenerateOptions, InMemoryStorage, Memory, MemoryConfig, Storage,
    TaskSignalProvider, agent,
};
use ratatui::{
    Terminal,
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph},
};
use std::env;
use std::io;
use std::sync::Arc;
use tokio::sync::mpsc;

#[derive(Clone, Debug)]
enum LogKind {
    User,
    AgentText,
    AgentReasoning,
    ToolCall,
    ToolResult,
    Error,
    Info,
}

#[derive(Clone, Debug)]
struct LogEntry {
    kind: LogKind,
    text: String,
}
#[derive(Clone, Debug, serde::Deserialize)]
struct TuiTask {
    id: String,
    content: String,
    status: String,
    #[serde(rename = "activeForm", alias = "active_form")]
    active_form: String,
}

#[derive(Clone, Debug, serde::Deserialize)]
struct TasksPayload {
    tasks: Option<Vec<TuiTask>>,
    task: Option<TuiTask>,
}

fn update_task_list_from_tool_result(tasks: &mut Vec<TuiTask>, tool_name: &str, result: &str) {
    if !matches!(
        tool_name,
        "task_write" | "task_update" | "task_complete" | "task_check"
    ) {
        return;
    }

    if let Ok(payload) = serde_json::from_str::<TasksPayload>(result) {
        if let Some(new_tasks) = payload.tasks {
            *tasks = new_tasks;
            return;
        }
        if let Some(updated) = payload.task {
            if let Some(existing) = tasks.iter_mut().find(|task| task.id == updated.id) {
                *existing = updated;
            } else {
                tasks.push(updated);
            }
        }
    }
}

fn task_status_style(status: &str) -> (Span<'static>, Style) {
    match status.to_ascii_lowercase().as_str() {
        "completed" | "done" => (
            Span::styled(
                "?",
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            ),
            Style::default().fg(Color::Green),
        ),
        "in_progress" | "running" | "active" => (
            Span::styled(
                "?",
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ),
        "failed" | "error" => (
            Span::styled(
                "!",
                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
            ),
            Style::default().fg(Color::Red),
        ),
        _ => (
            Span::styled("?", Style::default().fg(Color::DarkGray)),
            Style::default().fg(Color::Gray),
        ),
    }
}

fn render_task_panel(f: &mut ratatui::Frame, area: Rect, tasks: &[TuiTask], is_streaming: bool) {
    let completed = tasks
        .iter()
        .filter(|task| {
            task.status.eq_ignore_ascii_case("completed")
                || task.status.eq_ignore_ascii_case("done")
        })
        .count();
    let title = format!(" Task List ({}/{}) ", completed, tasks.len());

    if tasks.is_empty() {
        let empty = Paragraph::new("No task list yet. Agent will call task_write first.")
            .style(Style::default().fg(Color::DarkGray))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(title)
                    .border_style(Style::default().fg(Color::DarkGray)),
            );
        f.render_widget(empty, area);
        return;
    }

    let items: Vec<ListItem> = tasks
        .iter()
        .map(|task| {
            let (icon, style) = task_status_style(&task.status);
            let display_text = if task.status.eq_ignore_ascii_case("in_progress")
                || task.status.eq_ignore_ascii_case("active")
            {
                task.active_form.as_str()
            } else {
                task.content.as_str()
            };
            ListItem::new(Line::from(vec![
                icon,
                Span::raw(" "),
                Span::styled(
                    format!("{} ", task.id),
                    Style::default().fg(Color::DarkGray),
                ),
                Span::styled(display_text.to_string(), style),
            ]))
        })
        .collect();

    let border_color = if is_streaming {
        Color::Yellow
    } else {
        Color::Cyan
    };
    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(title)
                .border_style(Style::default().fg(border_color)),
        )
        .style(Style::default().fg(Color::White));
    f.render_widget(list, area);
}

enum UiEvent {
    Input(KeyEvent),
    Mouse(crossterm::event::MouseEvent),
    AgentText(String),
    AgentReasoning(String),
    AgentToolCall { name: String, args: String },
    AgentToolResult { name: String, result: String },
    AgentFinished,
    AgentError(String),
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // Silence background framework logging to prevent TUI screen corruption
    unsafe {
        std::env::set_var("LOCCLE_SILENT", "true");
    }

    // Load environment variables
    let _ = dotenvy::from_path("../.env");
    let _ = dotenvy::dotenv();

    if env::var("OPENAI_API_KEY").is_err() {
        println!("Error: OPENAI_API_KEY environment variable is not set.");
        println!("Please add it to your .env file or export it before running this program.");
        return Ok(());
    }

    // Terminal initialization
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Run core application loop
    let run_result = run_app(&mut terminal).await;

    // Restore terminal state
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    if let Err(err) = run_result {
        eprintln!("Application Error: {}", err);
    }

    Ok(())
}

async fn run_app(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let storage = Arc::new(InMemoryStorage::new());
    let memory = Memory::new(
        storage.clone(),
        MemoryConfig {
            last_messages: Some(10),
        },
    );

    let test_agent = Arc::new(agent! {
        id: "tui-agent",
        name: "Vibe Coding Agent",
        instructions: "You are a highly capable Vibe Coding Agent. Your goal is to understand the user request, plan the implementation steps, and execute them. \
                       After understanding the request, you MUST ALWAYS first create a task list using `task_write` to outline the steps of your implementation plan. \
                       As you progress and execute commands, edit files, or perform actions, you MUST update the status of each task using `task_update` and `task_complete` step-by-step. \
                       You have full access to filesystem tools and terminal command execution tools. Use them to implement, test, and complete the tasks autonomously.",
        memory: memory,
        tools: {
            let mut t = loccle::tools::all_fs_tools();
            t.extend(loccle::tools::all_system_tools());
            t
        },
        signal: TaskSignalProvider::new(),
    });

    let thread_id = "tui-thread-session";
    storage
        .create_thread(thread_id, Some("tui-user".to_string()))
        .await?;

    // Create channel for events
    let (tx, mut rx) = mpsc::channel::<UiEvent>(100);

    // Spawn crossterm event polling task
    let tx_input = tx.clone();
    tokio::spawn(async move {
        loop {
            if event::poll(std::time::Duration::from_millis(50)).unwrap() {
                match event::read().unwrap() {
                    Event::Key(key) => {
                        if key.kind != event::KeyEventKind::Release {
                            let _ = tx_input.send(UiEvent::Input(key)).await;
                        }
                    }
                    Event::Mouse(mouse) => {
                        let _ = tx_input.send(UiEvent::Mouse(mouse)).await;
                    }
                    _ => {}
                }
            }
        }
    });

    // App state
    let mut input_buffer = String::new();
    let mut responses: Vec<LogEntry> = vec![
        LogEntry {
            kind: LogKind::Info,
            text: "Welcome to Vibe-TUI Agent Checklist! Enter your request below to plan and execute coding tasks.".to_string(),
        }
    ];
    let mut scroll_offset: u16 = 0;
    let mut auto_scroll = true;
    let mut is_streaming = false;
    let mut current_action = String::from("Idle. Waiting for prompt...");
    let mut task_list: Vec<TuiTask> = Vec::new();

    loop {
        // Draw the terminal
        terminal.draw(|f| {
            let size = f.size();

            // Layout: main content area (scroll view) + input area at the bottom
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Min(8),    // Content area
                    Constraint::Length(3), // Input prompt
                ])
                .split(size);

            let content_chunks = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Percentage(68), Constraint::Percentage(32)])
                .split(chunks[0]);

            // 1. Build rich styled text lines for the scroll view
            let mut lines = Vec::new();
            let scroll_view_width = content_chunks[0].width.saturating_sub(2) as usize; // width inside borders

            for entry in &responses {
                match entry.kind {
                    LogKind::User => {
                        let prefix_text = "👤 User: ";
                        let prefix_width = 9;
                        let wrap_width = scroll_view_width.saturating_sub(prefix_width).max(20);
                        let wrapped = wrap_text(&entry.text, wrap_width);

                        let mut first = true;
                        for part in wrapped {
                            if first {
                                lines.push(Line::from(vec![
                                    Span::styled(
                                        prefix_text,
                                        Style::default()
                                            .fg(Color::Blue)
                                            .add_modifier(Modifier::BOLD),
                                    ),
                                    Span::styled(part, Style::default().fg(Color::White)),
                                ]));
                                first = false;
                            } else {
                                lines.push(Line::from(vec![
                                    Span::raw(" ".repeat(prefix_width)),
                                    Span::styled(part, Style::default().fg(Color::White)),
                                ]));
                            }
                        }
                    }
                    LogKind::AgentText => {
                        let prefix_text = "🤖 Agent: ";
                        let prefix_width = 10;
                        let wrap_width = scroll_view_width.saturating_sub(prefix_width).max(20);
                        let wrapped = wrap_text(&entry.text, wrap_width);

                        let mut first = true;
                        for part in wrapped {
                            if first {
                                lines.push(Line::from(vec![
                                    Span::styled(
                                        prefix_text,
                                        Style::default()
                                            .fg(Color::Green)
                                            .add_modifier(Modifier::BOLD),
                                    ),
                                    Span::styled(part, Style::default().fg(Color::White)),
                                ]));
                                first = false;
                            } else {
                                lines.push(Line::from(vec![
                                    Span::raw(" ".repeat(prefix_width)),
                                    Span::styled(part, Style::default().fg(Color::White)),
                                ]));
                            }
                        }
                    }
                    LogKind::AgentReasoning => {
                        let prefix_text = "🧠 [Thinking] ";
                        let prefix_width = 14;
                        let wrap_width = scroll_view_width.saturating_sub(prefix_width).max(20);
                        let wrapped = wrap_text(&entry.text, wrap_width);

                        let mut first = true;
                        for part in wrapped {
                            if first {
                                lines.push(Line::from(vec![
                                    Span::styled(
                                        prefix_text,
                                        Style::default()
                                            .fg(Color::DarkGray)
                                            .add_modifier(Modifier::ITALIC),
                                    ),
                                    Span::styled(part, Style::default().fg(Color::DarkGray)),
                                ]));
                                first = false;
                            } else {
                                lines.push(Line::from(vec![
                                    Span::raw(" ".repeat(prefix_width)),
                                    Span::styled(part, Style::default().fg(Color::DarkGray)),
                                ]));
                            }
                        }
                    }
                    LogKind::ToolCall => {
                        let prefix_text = "🔧 [Tool Call] ";
                        let prefix_width = 15;
                        let wrap_width = scroll_view_width.saturating_sub(prefix_width).max(20);
                        let wrapped = wrap_text(&entry.text, wrap_width);

                        let mut first = true;
                        for part in wrapped {
                            let line_style = if part.starts_with('+') {
                                Style::default().fg(Color::Green)
                            } else if part.starts_with('-') {
                                Style::default().fg(Color::Red)
                            } else if part.starts_with("@@") {
                                Style::default().fg(Color::Magenta)
                            } else {
                                Style::default().fg(Color::LightYellow)
                            };

                            if first {
                                lines.push(Line::from(vec![
                                    Span::styled(
                                        prefix_text,
                                        Style::default()
                                            .fg(Color::Yellow)
                                            .add_modifier(Modifier::BOLD),
                                    ),
                                    Span::styled(part, line_style),
                                ]));
                                first = false;
                            } else {
                                lines.push(Line::from(vec![
                                    Span::raw(" ".repeat(prefix_width)),
                                    Span::styled(part, line_style),
                                ]));
                            }
                        }
                    }
                    LogKind::ToolResult => {
                        let prefix_text = "✅ [Tool Result] ";
                        let prefix_width = 17;
                        let wrap_width = scroll_view_width.saturating_sub(prefix_width).max(20);
                        let wrapped = wrap_text(&entry.text, wrap_width);

                        let mut first = true;
                        for part in wrapped {
                            let line_style = if part.starts_with('+') {
                                Style::default().fg(Color::Green)
                            } else if part.starts_with('-') {
                                Style::default().fg(Color::Red)
                            } else if part.starts_with("@@") {
                                Style::default().fg(Color::Magenta)
                            } else {
                                Style::default().fg(Color::LightCyan)
                            };

                            if first {
                                lines.push(Line::from(vec![
                                    Span::styled(
                                        prefix_text,
                                        Style::default()
                                            .fg(Color::Cyan)
                                            .add_modifier(Modifier::BOLD),
                                    ),
                                    Span::styled(part, line_style),
                                ]));
                                first = false;
                            } else {
                                lines.push(Line::from(vec![
                                    Span::raw(" ".repeat(prefix_width)),
                                    Span::styled(part, line_style),
                                ]));
                            }
                        }
                    }
                    LogKind::Error => {
                        let prefix_text = "❌ [Error] ";
                        let prefix_width = 11;
                        let wrap_width = scroll_view_width.saturating_sub(prefix_width).max(20);
                        let wrapped = wrap_text(&entry.text, wrap_width);

                        let mut first = true;
                        for part in wrapped {
                            if first {
                                lines.push(Line::from(vec![
                                    Span::styled(
                                        prefix_text,
                                        Style::default()
                                            .fg(Color::Red)
                                            .add_modifier(Modifier::BOLD),
                                    ),
                                    Span::styled(part, Style::default().fg(Color::Red)),
                                ]));
                                first = false;
                            } else {
                                lines.push(Line::from(vec![
                                    Span::raw(" ".repeat(prefix_width)),
                                    Span::styled(part, Style::default().fg(Color::Red)),
                                ]));
                            }
                        }
                    }
                    LogKind::Info => {
                        let prefix_text = "ℹ️ [Info] ";
                        let prefix_width = 10;
                        let wrap_width = scroll_view_width.saturating_sub(prefix_width).max(20);
                        let wrapped = wrap_text(&entry.text, wrap_width);

                        let mut first = true;
                        for part in wrapped {
                            if first {
                                lines.push(Line::from(vec![
                                    Span::styled(prefix_text, Style::default().fg(Color::Magenta)),
                                    Span::styled(part, Style::default().fg(Color::Magenta)),
                                ]));
                                first = false;
                            } else {
                                lines.push(Line::from(vec![
                                    Span::raw(" ".repeat(prefix_width)),
                                    Span::styled(part, Style::default().fg(Color::Magenta)),
                                ]));
                            }
                        }
                    }
                }
                // Add an empty line between sequential blocks for readability
                lines.push(Line::from(""));
            }

            // Calculate auto scroll
            let rendering_height = content_chunks[0].height.saturating_sub(2) as usize; // Area height excluding borders
            let line_count = lines.len();

            if auto_scroll && line_count > rendering_height {
                scroll_offset = (line_count - rendering_height) as u16;
            } else if !auto_scroll {
                // Clamp manual scroll_offset
                let max_scroll = line_count.saturating_sub(rendering_height) as u16;
                if scroll_offset >= max_scroll {
                    scroll_offset = max_scroll;
                    auto_scroll = true; // Auto-scroll resumes when user scrolls back to bottom
                }
            }

            let text = ratatui::text::Text::from(lines);
            let scroll_view = Paragraph::new(text)
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .title(format!(
                            " Streaming Response Logs & Task Signal Lane | Action: {} ",
                            current_action
                        ))
                        .border_style(Style::default().fg(if is_streaming {
                            Color::Yellow
                        } else {
                            Color::Green
                        })),
                )
                .scroll((scroll_offset, 0));

            f.render_widget(scroll_view, content_chunks[0]);
            render_task_panel(f, content_chunks[1], &task_list, is_streaming);

            // 2. Render input prompt
            let input_style = if is_streaming {
                Style::default().fg(Color::DarkGray)
            } else {
                Style::default().fg(Color::White)
            };

            let input_title = if is_streaming {
                " Prompt Input (Locked - Agent is working...) "
            } else {
                " Prompt Input (Press Enter to Send, Esc to Quit) "
            };

            let input_paragraph = Paragraph::new(input_buffer.as_str())
                .style(input_style)
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .title(input_title)
                        .border_style(Style::default().fg(if is_streaming {
                            Color::DarkGray
                        } else {
                            Color::Blue
                        })),
                );

            f.render_widget(input_paragraph, chunks[1]);
        })?;

        // Process incoming events
        if let Some(event) = rx.recv().await {
            match event {
                UiEvent::Mouse(mouse) => match mouse.kind {
                    event::MouseEventKind::ScrollUp => {
                        auto_scroll = false;
                        scroll_offset = scroll_offset.saturating_sub(2);
                    }
                    event::MouseEventKind::ScrollDown => {
                        auto_scroll = false;
                        scroll_offset = scroll_offset.saturating_add(2);
                    }
                    _ => {}
                },
                UiEvent::Input(key) => {
                    match key.code {
                        KeyCode::Esc => {
                            break;
                        }
                        KeyCode::Enter => {
                            if !is_streaming && !input_buffer.trim().is_empty() {
                                let prompt = input_buffer.clone();
                                responses.push(LogEntry {
                                    kind: LogKind::User,
                                    text: prompt.clone(),
                                });

                                input_buffer.clear();
                                is_streaming = true;
                                auto_scroll = true;
                                current_action = "Initiating agent stream...".to_string();

                                // Spawn the async streaming task
                                let agent = test_agent.clone();
                                let tx_agent = tx.clone();
                                tokio::spawn(async move {
                                    let options = GenerateOptions::new()
                                        .thread_id(thread_id)
                                        .resource_id("tui-user");

                                    match agent.stream_with_options(&prompt, options).await {
                                        Ok(mut stream) => {
                                            while let Some(event_res) = stream.next().await {
                                                match event_res {
                                                    Ok(e) => match e {
                                                        AgentStreamEvent::TextDelta(t) => {
                                                            let _ = tx_agent
                                                                .send(UiEvent::AgentText(t))
                                                                .await;
                                                        }
                                                        AgentStreamEvent::ReasoningDelta(r) => {
                                                            let _ = tx_agent
                                                                .send(UiEvent::AgentReasoning(r))
                                                                .await;
                                                        }
                                                        AgentStreamEvent::ToolCall {
                                                            name,
                                                            arguments,
                                                            ..
                                                        } => {
                                                            let _ = tx_agent
                                                                .send(UiEvent::AgentToolCall {
                                                                    name,
                                                                    args: arguments,
                                                                })
                                                                .await;
                                                        }
                                                        AgentStreamEvent::ToolResult {
                                                            name,
                                                            result,
                                                            ..
                                                        } => {
                                                            let _ = tx_agent
                                                                .send(UiEvent::AgentToolResult {
                                                                    name,
                                                                    result,
                                                                })
                                                                .await;
                                                        }
                                                        AgentStreamEvent::Finish { .. } => {
                                                            let _ = tx_agent
                                                                .send(UiEvent::AgentFinished)
                                                                .await;
                                                        }
                                                    },
                                                    Err(err) => {
                                                        let _ = tx_agent
                                                            .send(UiEvent::AgentError(
                                                                err.to_string(),
                                                            ))
                                                            .await;
                                                    }
                                                }
                                            }
                                        }
                                        Err(err) => {
                                            let _ = tx_agent
                                                .send(UiEvent::AgentError(err.to_string()))
                                                .await;
                                        }
                                    }
                                });
                            }
                        }
                        KeyCode::Char(c) => {
                            if !is_streaming {
                                input_buffer.push(c);
                            }
                        }
                        KeyCode::Backspace => {
                            if !is_streaming {
                                input_buffer.pop();
                            }
                        }
                        KeyCode::Up => {
                            auto_scroll = false;
                            scroll_offset = scroll_offset.saturating_sub(1);
                        }
                        KeyCode::Down => {
                            auto_scroll = false;
                            scroll_offset = scroll_offset.saturating_add(1);
                        }
                        KeyCode::PageUp => {
                            auto_scroll = false;
                            scroll_offset = scroll_offset.saturating_sub(10);
                        }
                        KeyCode::PageDown => {
                            auto_scroll = false;
                            scroll_offset = scroll_offset.saturating_add(10);
                        }
                        _ => {}
                    }
                }
                UiEvent::AgentText(t) => {
                    // Append to last AgentText block or create new one
                    if let Some(last) = responses.last_mut() {
                        if matches!(last.kind, LogKind::AgentText) {
                            last.text.push_str(&t);
                        } else {
                            responses.push(LogEntry {
                                kind: LogKind::AgentText,
                                text: t,
                            });
                        }
                    } else {
                        responses.push(LogEntry {
                            kind: LogKind::AgentText,
                            text: t,
                        });
                    }
                }
                UiEvent::AgentReasoning(r) => {
                    // Append to last AgentReasoning block or create new one
                    if let Some(last) = responses.last_mut() {
                        if matches!(last.kind, LogKind::AgentReasoning) {
                            last.text.push_str(&r);
                        } else {
                            responses.push(LogEntry {
                                kind: LogKind::AgentReasoning,
                                text: r,
                            });
                        }
                    } else {
                        responses.push(LogEntry {
                            kind: LogKind::AgentReasoning,
                            text: r,
                        });
                    }
                }
                UiEvent::AgentToolCall { name, args } => {
                    current_action = format!("Invoking tool: {}", name);
                    responses.push(LogEntry {
                        kind: LogKind::ToolCall,
                        text: format_tool_call(&name, &args),
                    });
                }
                UiEvent::AgentToolResult { name, result } => {
                    current_action = format!("Tool {} complete", name);
                    update_task_list_from_tool_result(&mut task_list, &name, &result);
                    if !matches!(
                        name.as_str(),
                        "task_write" | "task_update" | "task_complete" | "task_check"
                    ) {
                        responses.push(LogEntry {
                            kind: LogKind::ToolResult,
                            text: format_tool_result(&name, &result),
                        });
                    }
                }
                UiEvent::AgentFinished => {
                    is_streaming = false;
                    current_action = "Idle. Waiting for prompt...".to_string();
                    responses.push(LogEntry {
                        kind: LogKind::Info,
                        text: "Agent completed execution run successfully.".to_string(),
                    });
                }
                UiEvent::AgentError(err_msg) => {
                    is_streaming = false;
                    current_action = "Error encountered".to_string();
                    responses.push(LogEntry {
                        kind: LogKind::Error,
                        text: err_msg,
                    });
                }
            }
        }
    }

    Ok(())
}

fn wrap_text(text: &str, max_width: usize) -> Vec<String> {
    let mut lines = Vec::new();
    for line in text.split('\n') {
        if line.is_empty() {
            lines.push(String::new());
            continue;
        }
        let mut current_line = String::new();
        let words: Vec<&str> = line.split(' ').collect();
        let mut is_first = true;
        for word in words {
            if is_first {
                current_line.push_str(word);
                is_first = false;
            } else {
                if current_line.len() + 1 + word.len() <= max_width {
                    current_line.push(' ');
                    current_line.push_str(word);
                } else {
                    lines.push(current_line);
                    current_line = word.to_string();
                }
            }
        }
        lines.push(current_line);
    }
    lines
}

fn format_tool_call(name: &str, args_str: &str) -> String {
    let parsed: Result<serde_json::Value, _> = serde_json::from_str(args_str);
    match parsed {
        Ok(serde_json::Value::Object(map)) => match name {
            "execute_command" => {
                let cmd = map.get("command").and_then(|v| v.as_str()).unwrap_or("");
                let args = map
                    .get("args")
                    .and_then(|v| v.as_array())
                    .map(|arr| {
                        arr.iter()
                            .map(|v| v.as_str().unwrap_or("").to_string())
                            .collect::<Vec<_>>()
                            .join(" ")
                    })
                    .unwrap_or_default();
                let bg = map
                    .get("background")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                let bg_suffix = if bg { " (background)" } else { "" };
                format!("Execute command: {} {}{}", cmd, args, bg_suffix)
            }
            "write_file" => {
                let path = map.get("path").and_then(|v| v.as_str()).unwrap_or("");
                let content = map.get("content").and_then(|v| v.as_str()).unwrap_or("");
                format!("Write file: {}\n{}", path, compute_diff(path, content))
            }
            "delete" => {
                let path = map.get("path").and_then(|v| v.as_str()).unwrap_or("");
                format!("Delete path: {}", path)
            }
            "read_file" => {
                let path = map.get("path").and_then(|v| v.as_str()).unwrap_or("");
                format!("Read file: {}", path)
            }
            "grep" => {
                let query = map.get("query").and_then(|v| v.as_str()).unwrap_or("");
                let path = map.get("path").and_then(|v| v.as_str()).unwrap_or(".");
                format!("Search for {:?} in {}", query, path)
            }
            _ => format!("{} {}", name, args_str),
        },
        _ => format!("{} {}", name, args_str),
    }
}

fn format_tool_result(name: &str, result_str: &str) -> String {
    let parsed: Result<serde_json::Value, _> = serde_json::from_str(result_str);
    match parsed {
        Ok(serde_json::Value::Object(map)) => match name {
            "execute_command" => {
                let stdout = map.get("stdout").and_then(|v| v.as_str()).unwrap_or("");
                let stderr = map.get("stderr").and_then(|v| v.as_str()).unwrap_or("");
                let code = map.get("exit_code").and_then(|v| v.as_i64()).unwrap_or(0);
                if code == 0 {
                    format!("Success (exit code 0)\n{}", stdout.trim())
                } else {
                    format!(
                        "Failed (exit code {})\nOutput:\n{}\nError:\n{}",
                        code,
                        stdout.trim(),
                        stderr.trim()
                    )
                }
            }
            "task_write" | "task_check" => {
                if let Some(tasks) = map.get("tasks").and_then(|v| v.as_array()) {
                    let mut lines = vec!["Task list status:".to_string()];
                    for t in tasks {
                        let id = t.get("id").and_then(|v| v.as_str()).unwrap_or("");
                        let content = t.get("content").and_then(|v| v.as_str()).unwrap_or("");
                        let status = t.get("status").and_then(|v| v.as_str()).unwrap_or("");
                        let status_icon = match status {
                            "completed" => "?",
                            "in_progress" => "??",
                            _ => "?",
                        };
                        lines.push(format!(
                            "  {} #{} {} [{}]",
                            status_icon, id, content, status
                        ));
                    }
                    lines.join("\n")
                } else {
                    truncate_result(result_str)
                }
            }
            "task_update" | "task_complete" => {
                if let Some(task) = map.get("task") {
                    let id = task.get("id").and_then(|v| v.as_str()).unwrap_or("");
                    let content = task.get("content").and_then(|v| v.as_str()).unwrap_or("");
                    let status = task.get("status").and_then(|v| v.as_str()).unwrap_or("");
                    format!("Updated task #{} to [{}] - {}", id, status, content)
                } else {
                    truncate_result(result_str)
                }
            }
            _ => truncate_result(result_str),
        },
        _ => truncate_result(result_str),
    }
}

fn truncate_result(result_str: &str) -> String {
    if result_str.len() > 1000 {
        format!(
            "Result (truncated to 1000 of {} characters):\n{}",
            result_str.len(),
            &result_str[..1000]
        )
    } else {
        result_str.to_string()
    }
}
fn compute_diff(path: &str, new_content: &str) -> String {
    use std::fs;
    let old_content = fs::read_to_string(path).unwrap_or_default();

    let diff = similar::TextDiff::from_lines(old_content.as_str(), new_content);
    let diff_output = format!("{}", diff.unified_diff().context_radius(3));

    if diff_output.trim().is_empty() {
        if old_content.is_empty() && !new_content.is_empty() {
            let mut res = String::new();
            for line in new_content.lines() {
                res.push_str(&format!("+{}\n", line));
            }
            res
        } else {
            "No changes made to the file.".to_string()
        }
    } else {
        diff_output
    }
}
