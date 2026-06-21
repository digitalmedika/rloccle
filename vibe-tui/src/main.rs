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
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap},
};
use std::env;
use std::io;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::mpsc;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

const RESULT_PREVIEW_LIMIT: usize = 1200;
const CMD_RESULT_PREVIEW_LIMIT: usize = 400;
const RESULT_LIST_LIMIT: usize = 12;
const TOOL_PROGRESS_WIDTH: usize = 28;
const TOOL_PROGRESS_TICK_MS: u64 = 120;

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
    is_running: bool,
}

impl LogEntry {
    fn new(kind: LogKind, text: String) -> Self {
        Self {
            kind,
            text,
            is_running: false,
        }
    }

    fn new_tool_call(text: String) -> Self {
        Self {
            kind: LogKind::ToolCall,
            text,
            is_running: true,
        }
    }
}

#[derive(Clone, Debug)]
struct ActiveToolProgress {
    name: String,
    started_at: Instant,
    tick: u64,
}

impl ActiveToolProgress {
    fn new(name: String) -> Self {
        Self {
            name,
            started_at: Instant::now(),
            tick: 0,
        }
    }
    fn elapsed_secs(&self) -> u64 {
        self.started_at.elapsed().as_secs()
    }
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

fn display_width(text: &str) -> usize {
    UnicodeWidthStr::width(text)
}

fn truncate_to_width(text: &str, max_width: usize) -> String {
    if display_width(text) <= max_width {
        return text.to_string();
    }

    if max_width <= 3 {
        return ".".repeat(max_width);
    }

    let target_width = max_width - 3;
    let mut output = String::new();
    let mut width = 0;

    for ch in text.chars() {
        let ch_width = UnicodeWidthChar::width(ch).unwrap_or(0);
        if width + ch_width > target_width {
            break;
        }
        output.push(ch);
        width += ch_width;
    }

    output.push_str("...");
    output
}

fn bounded_title(title: &str, area_width: u16) -> String {
    let max_width = area_width.saturating_sub(2) as usize;
    if max_width <= 2 {
        return truncate_to_width(title, max_width);
    }

    format!(" {} ", truncate_to_width(title, max_width - 2))
}

fn friendly_path(path: &str) -> String {
    let path_value = Path::new(path);
    let current_dir = env::current_dir().ok();
    let workspace_dir = current_dir.as_ref().and_then(|cwd| {
        if cwd.file_name().and_then(|name| name.to_str()) == Some("vibe-tui") {
            cwd.parent().map(|parent| parent.to_path_buf())
        } else {
            Some(cwd.to_path_buf())
        }
    });

    if let Some(workspace) = workspace_dir {
        if let Ok(relative) = path_value.strip_prefix(workspace) {
            return relative.to_string_lossy().into_owned();
        }
    }

    path.to_string()
}

fn json_string_array<'a>(
    map: &'a serde_json::Map<String, serde_json::Value>,
    key: &str,
) -> Vec<&'a str> {
    map.get(key)
        .and_then(|v| v.as_array())
        .map(|items| items.iter().filter_map(|item| item.as_str()).collect())
        .unwrap_or_default()
}

fn format_path_list(label: &str, paths: &[&str], extra_count: usize) -> String {
    let mut lines = vec![format!(
        "{} (showing {} of {})",
        label,
        paths.len().min(RESULT_LIST_LIMIT),
        paths.len() + extra_count
    )];

    for path in paths.iter().take(RESULT_LIST_LIMIT) {
        lines.push(format!("  - {}", friendly_path(path)));
    }

    let hidden_count = paths.len().saturating_sub(RESULT_LIST_LIMIT) + extra_count;
    if hidden_count > 0 {
        lines.push(format!("  ... {} more not shown", hidden_count));
    }

    lines.join("\n")
}

fn truncate_plain_text(text: &str, limit: usize) -> String {
    if text.chars().count() <= limit {
        return text.to_string();
    }

    let mut output: String = text.chars().take(limit).collect();
    output.push_str(&format!("\n... truncated after {} characters", limit));
    output
}

fn summarize_json_value(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::Null => "empty".to_string(),
        serde_json::Value::Bool(v) => v.to_string(),
        serde_json::Value::Number(v) => v.to_string(),
        serde_json::Value::String(v) => {
            if v.contains('\\') || v.contains('/') {
                friendly_path(v)
            } else {
                truncate_plain_text(v, 120)
            }
        }
        serde_json::Value::Array(items) => {
            if items.is_empty() {
                "0 items".to_string()
            } else {
                format!("{} items", items.len())
            }
        }
        serde_json::Value::Object(map) => format!("{} fields", map.len()),
    }
}

fn format_tool_fields(name: &str, map: &serde_json::Map<String, serde_json::Value>) -> String {
    let mut lines = vec![format!("{} completed:", name)];

    for (key, value) in map {
        lines.push(format!("  - {}: {}", key, summarize_json_value(value)));
    }

    lines.join("\n")
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

fn task_status_style(status: &str) -> (&'static str, Style) {
    match status.to_ascii_lowercase().as_str() {
        "completed" | "done" => ("x", Style::default().fg(Color::Green)),
        "in_progress" | "running" | "active" => (
            ">",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ),
        "failed" | "error" => ("!", Style::default().fg(Color::Red)),
        _ => ("-", Style::default().fg(Color::Gray)),
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
    let title = bounded_title(
        &format!("Task List ({}/{})", completed, tasks.len()),
        area.width,
    );

    if tasks.is_empty() {
        let empty = Paragraph::new(
            wrap_text(
                "No task list yet. Agent will call task_write first.",
                area.width.saturating_sub(2) as usize,
            )
            .join("\n"),
        )
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

    let task_width = area.width.saturating_sub(2) as usize;
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
            let prefix = truncate_to_width(
                &format!("{} {} ", icon, task.id),
                task_width.saturating_sub(1).max(1),
            );
            let prefix_width = display_width(&prefix);
            let text_width = task_width.saturating_sub(prefix_width).max(1);
            let wrapped = wrap_text(display_text, text_width);
            let mut lines = Vec::new();

            for (idx, part) in wrapped.into_iter().enumerate() {
                if idx == 0 {
                    lines.push(Line::from(vec![
                        Span::styled(prefix.clone(), Style::default().fg(Color::DarkGray)),
                        Span::styled(part, style),
                    ]));
                } else {
                    lines.push(Line::from(vec![
                        Span::raw(" ".repeat(prefix_width)),
                        Span::styled(part, style),
                    ]));
                }
            }

            ListItem::new(lines)
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

fn animated_progress_line(tool: &ActiveToolProgress) -> String {
    let pos = (tool.tick as usize) % TOOL_PROGRESS_WIDTH;
    let mut bar = String::with_capacity(TOOL_PROGRESS_WIDTH);
    for idx in 0..TOOL_PROGRESS_WIDTH {
        let distance = idx.abs_diff(pos);
        bar.push(match distance {
            0 => '█',
            1 => '▓',
            2 => '▒',
            _ => '░',
        });
    }
    let frames = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
    let spinner = frames[(tool.tick as usize) % frames.len()];
    format!(
        "{} [{}] running {} ({}s)",
        spinner,
        bar,
        tool.name,
        tool.elapsed_secs()
    )
}


fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(r);
    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}

fn render_sessions_overlay(
    f: &mut ratatui::Frame,
    area: Rect,
    sessions: &[String],
    selected_session: usize,
    active_thread_id: &str,
) {
    let popup = centered_rect(62, 60, area);
    f.render_widget(Clear, popup);

    let items: Vec<ListItem> = sessions
        .iter()
        .enumerate()
        .map(|(idx, session)| {
            let marker = if session == active_thread_id {
                "*"
            } else {
                " "
            };
            let text = format!("{} {}", marker, session);
            let style = if idx == selected_session {
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Cyan)
                    .add_modifier(Modifier::BOLD)
            } else if session == active_thread_id {
                Style::default().fg(Color::Green)
            } else {
                Style::default().fg(Color::White)
            };
            ListItem::new(Line::from(Span::styled(text, style)))
        })
        .collect();

    let mut state = ListState::default();
    if !sessions.is_empty() {
        state.select(Some(selected_session.min(sessions.len() - 1)));
    }

    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Sessions (Up/Down select, Enter load, Esc close) ")
                .border_style(Style::default().fg(Color::Cyan)),
        )
        .highlight_style(
            Style::default()
                .fg(Color::Black)
                .bg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        );
    f.render_stateful_widget(list, popup, &mut state);
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
    Tick,
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

    let mut session_counter: u64 = 1;
    let mut thread_id = format!("tui-thread-session-{}", session_counter);
    let mut sessions: Vec<String> = vec![thread_id.clone()];
    let mut sessions_overlay = false;
    let mut selected_session: usize = 0;
    storage
        .create_thread(&thread_id, Some("tui-user".to_string()))
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

    // Spawn UI ticker so active tool progress bars animate even when no input arrives.
    let tx_tick = tx.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_millis(TOOL_PROGRESS_TICK_MS));
        loop {
            interval.tick().await;
            if tx_tick.send(UiEvent::Tick).await.is_err() {
                break;
            }
        }
    });



    // App state
    let mut input_buffer = String::new();
    let mut responses: Vec<LogEntry> = vec![
        LogEntry::new(
            LogKind::Info,
            "Welcome to Vibe-TUI Agent Checklist! Enter your request below to plan and execute coding tasks. Type /new to start a fresh session, /sessions to browse sessions.".to_string(),
        )
    ];
    let mut scroll_offset: u16 = 0;
    let mut auto_scroll = true;
    let mut is_streaming = false;
    let mut current_action = String::from("Idle. Waiting for prompt...");
    let mut task_list: Vec<TuiTask> = Vec::new();
    let mut last_tool_call_args: Option<(String, String)> = None;
    let mut active_tool_progress: Option<ActiveToolProgress> = None;

    loop {
        // Draw the terminal
        terminal.draw(|f| {
            let size = f.size();

            let input_inner_width = size.width.saturating_sub(2) as usize;
            let input_line_count = wrap_text(&input_buffer, input_inner_width)
                .len()
                .max(1) as u16;
            let input_height = input_line_count.saturating_add(2).min(size.height.saturating_sub(8).max(3));

            // Layout: main content area (scroll view) + input area at the bottom
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints(vec![Constraint::Min(8), Constraint::Length(input_height)])
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
                        let prefix_text = "User: ";
                        let prefix_width = display_width(prefix_text);
                        let wrap_width = scroll_view_width.saturating_sub(prefix_width).max(1);
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
                        let prefix_text = "Agent: ";
                        let prefix_width = display_width(prefix_text);
                        let wrap_width = scroll_view_width.saturating_sub(prefix_width).max(1);
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
                        let prefix_text = "[Thinking] ";
                        let prefix_width = display_width(prefix_text);
                        let wrap_width = scroll_view_width.saturating_sub(prefix_width).max(1);
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
                        let is_running = entry.is_running;
                        let prefix_text = if is_running {
                            let frames = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
                            let spinner = active_tool_progress
                                .as_ref()
                                .map(|p| frames[(p.tick as usize) % frames.len()])
                                .unwrap_or("⠋");
                            format!("{} ", spinner)
                        } else {
                            "✓ ".to_string()
                        };
                        let prefix_width = display_width(&prefix_text);
                        let wrap_width = scroll_view_width.saturating_sub(prefix_width).max(1);
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
                                let prefix_style = if is_running {
                                    Style::default()
                                        .fg(Color::Yellow)
                                        .add_modifier(Modifier::BOLD)
                                } else {
                                    Style::default()
                                        .fg(Color::Green)
                                        .add_modifier(Modifier::BOLD)
                                };
                                lines.push(Line::from(vec![
                                    Span::styled(prefix_text.clone(), prefix_style),
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
                        let prefix_text = "[Tool Result] ";
                        let prefix_width = display_width(prefix_text);
                        let wrap_width = scroll_view_width.saturating_sub(prefix_width).max(1);
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
                        let prefix_text = "[Error] ";
                        let prefix_width = display_width(prefix_text);
                        let wrap_width = scroll_view_width.saturating_sub(prefix_width).max(1);
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
                        let prefix_text = "[Info] ";
                        let prefix_width = display_width(prefix_text);
                        let wrap_width = scroll_view_width.saturating_sub(prefix_width).max(1);
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
            let scroll_title = bounded_title(
                &format!(
                    "Streaming Response Logs & Task Signal Lane | Action: {}",
                    current_action
                ),
                content_chunks[0].width,
            );
            let scroll_view = Paragraph::new(text)
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .title(scroll_title)
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
                " Prompt Input (Enter to Send, /new, /sessions, Esc to Quit) "
            };

            let input_paragraph = Paragraph::new(input_buffer.as_str())
                .style(input_style)
                .wrap(Wrap { trim: false })
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



            if sessions_overlay {
                render_sessions_overlay(f, size, &sessions, selected_session, &thread_id);
            }
        })?;

        // Process incoming events
        if let Some(event) = rx.recv().await {
            match event {
                UiEvent::Tick => {
                    if let Some(tool_progress) = active_tool_progress.as_mut() {
                        tool_progress.tick = tool_progress.tick.wrapping_add(1);
                        current_action = animated_progress_line(tool_progress);
                    }
                }

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
                    if sessions_overlay {
                        match key.code {
                            KeyCode::Esc => sessions_overlay = false,
                            KeyCode::Up => selected_session = selected_session.saturating_sub(1),
                            KeyCode::Down => {
                                if selected_session + 1 < sessions.len() {
                                    selected_session += 1;
                                }
                            }
                            KeyCode::Enter => {
                                if let Some(selected) = sessions.get(selected_session).cloned() {
                                    thread_id = selected.clone();
                                    input_buffer.clear();
                                    responses.clear();
                                    task_list.clear();
                                    scroll_offset = 0;
                                    auto_scroll = true;
                                    current_action = format!("Loaded session {}", thread_id);
                                    responses.push(LogEntry::new(LogKind::Info, format!("Loaded session {}. New prompts will continue this conversation context.", thread_id)));
                                }
                                sessions_overlay = false;
                            }
                            _ => {}
                        }
                        continue;
                    }
                    match key.code {
                        KeyCode::Esc => {
                            break;
                        }
                        KeyCode::Enter => {
                            if !is_streaming && !input_buffer.trim().is_empty() {
                                let prompt = input_buffer.trim().to_string();

                                if prompt == "/sessions" {
                                    selected_session =
                                        sessions.iter().position(|s| s == &thread_id).unwrap_or(0);
                                    sessions_overlay = true;
                                    input_buffer.clear();
                                    current_action = "Browsing sessions".to_string();
                                    continue;
                                }

                                if prompt == "/new" {
                                    session_counter += 1;
                                    thread_id = format!("tui-thread-session-{}", session_counter);
                                    storage
                                        .create_thread(&thread_id, Some("tui-user".to_string()))
                                        .await?;
                                    sessions.push(thread_id.clone());
                                    selected_session = sessions.len().saturating_sub(1);

                                    input_buffer.clear();
                                    responses.clear();
                                    task_list.clear();
                                    scroll_offset = 0;
                                    auto_scroll = true;
                                    current_action = "Started a new session".to_string();
                                    responses.push(LogEntry::new(
                                        LogKind::Info,
                                        format!(
                                            "Started a fresh session ({}). Previous conversation and task list were cleared.",
                                            thread_id
                                        ),
                                    ));
                                    continue;
                                }

                                responses.push(LogEntry::new(
                                    LogKind::User,
                                    prompt.clone(),
                                ));

                                input_buffer.clear();
                                is_streaming = true;
                                auto_scroll = true;
                                current_action = "Initiating agent stream...".to_string();

                                // Spawn the async streaming task
                                let agent = test_agent.clone();
                                let tx_agent = tx.clone();
                                let active_thread_id = thread_id.clone();
                                tokio::spawn(async move {
                                    let options = GenerateOptions::new()
                                        .thread_id(active_thread_id)
                                        .resource_id("tui-user")
                                        .max_steps(40);

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
                            responses.push(LogEntry::new(LogKind::AgentText, t));
                        }
                    } else {
                        responses.push(LogEntry::new(LogKind::AgentText, t));
                    }
                }
                UiEvent::AgentReasoning(r) => {
                    // Append to last AgentReasoning block or create new one
                    if let Some(last) = responses.last_mut() {
                        if matches!(last.kind, LogKind::AgentReasoning) {
                            last.text.push_str(&r);
                        } else {
                            responses.push(LogEntry::new(LogKind::AgentReasoning, r));
                        }
                    } else {
                        responses.push(LogEntry::new(LogKind::AgentReasoning, r));
                    }
                }
                UiEvent::AgentToolCall { name, args } => {
                    last_tool_call_args = Some((name.clone(), args.clone()));
                    active_tool_progress = Some(ActiveToolProgress::new(name.clone()));
                    current_action = active_tool_progress
                        .as_ref()
                        .map(animated_progress_line)
                        .unwrap_or_else(|| format!("Invoking tool: {}", name));
                    responses.push(LogEntry::new_tool_call(format_tool_call(&name, &args)));
                }
                UiEvent::AgentToolResult { name, result } => {
                    active_tool_progress = None;
                    current_action = format!("Tool {} complete", name);
                    update_task_list_from_tool_result(&mut task_list, &name, &result);

                    // Mark the last running ToolCall log entry as completed
                    if let Some(last_tool_call) = responses
                        .iter_mut()
                        .rev()
                        .find(|entry| matches!(entry.kind, LogKind::ToolCall) && entry.is_running)
                    {
                        last_tool_call.is_running = false;
                    }

                    if !matches!(
                        name.as_str(),
                        "task_write" | "task_update" | "task_complete" | "task_check"
                    ) {
                        let call_args =
                            if let Some((ref last_name, ref last_args)) = last_tool_call_args {
                                if last_name == &name {
                                    Some(last_args.as_str())
                                } else {
                                    None
                                }
                            } else {
                                None
                            };
                        responses.push(LogEntry::new(
                            LogKind::ToolResult,
                            format_tool_result(&name, &result, call_args),
                        ));
                    }
                }
                UiEvent::AgentFinished => {
                    active_tool_progress = None;
                    is_streaming = false;
                    current_action = "Idle. Waiting for prompt...".to_string();
                    responses.push(LogEntry::new(
                        LogKind::Info,
                        "Agent completed execution run successfully.".to_string(),
                    ));
                }
                UiEvent::AgentError(err_msg) => {
                    active_tool_progress = None;
                    is_streaming = false;
                    current_action = "Error encountered".to_string();
                    responses.push(LogEntry::new(LogKind::Error, err_msg));
                }
            }
        }
    }

    Ok(())
}

fn wrap_text(text: &str, max_width: usize) -> Vec<String> {
    let max_width = max_width.max(1);
    let mut lines = Vec::new();

    for line in text.split('\n') {
        if line.is_empty() {
            lines.push(String::new());
            continue;
        }

        let mut current_line = String::new();
        let mut current_width = 0;

        for word in line.split_whitespace() {
            let word_width = display_width(word);

            if word_width > max_width {
                if !current_line.is_empty() {
                    lines.push(current_line);
                    current_line = String::new();
                    current_width = 0;
                }

                lines.extend(split_word_to_width(word, max_width));
                continue;
            }

            if current_line.is_empty() {
                current_line.push_str(word);
                current_width = word_width;
            } else if current_width + 1 + word_width <= max_width {
                current_line.push(' ');
                current_line.push_str(word);
                current_width += 1 + word_width;
            } else {
                lines.push(current_line);
                current_line = word.to_string();
                current_width = word_width;
            }
        }

        if !current_line.is_empty() {
            lines.push(current_line);
        }
    }

    lines
}

fn split_word_to_width(word: &str, max_width: usize) -> Vec<String> {
    let mut chunks = Vec::new();
    let mut current = String::new();
    let mut current_width = 0;

    for ch in word.chars() {
        let ch_width = UnicodeWidthChar::width(ch).unwrap_or(0);
        if !current.is_empty() && current_width + ch_width > max_width {
            chunks.push(current);
            current = String::new();
            current_width = 0;
        }

        current.push(ch);
        current_width += ch_width;

        if current_width >= max_width {
            chunks.push(current);
            current = String::new();
            current_width = 0;
        }
    }

    if !current.is_empty() {
        chunks.push(current);
    }

    chunks
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wrap_text_breaks_long_tokens_to_fit_width() {
        assert_eq!(wrap_text("abcdefgh", 3), vec!["abc", "def", "gh"]);
    }

    #[test]
    fn wrap_text_uses_display_width_for_unicode() {
        let wrapped = wrap_text("Ã°Å¸Â¤â€“Ã°Å¸Â¤â€“Ã°Å¸Â¤â€“ done", 4);
        assert!(wrapped.iter().all(|line| display_width(line) <= 4));
    }

    #[test]
    fn truncate_to_width_keeps_result_within_limit() {
        let truncated = truncate_to_width("Streaming Response Logs", 12);
        assert!(display_width(&truncated) <= 12);
        assert!(truncated.ends_with("..."));
    }

    #[test]
    fn glob_call_and_result_are_human_readable() {
        let call = format_tool_call("glob", r#"{"pattern":"D:\\project\\rloccle\\**\\*.rs"}"#);
        let result = format_tool_result(
            "glob",
            r#"{"paths":["D:\\project\\rloccle\\src\\agent.rs","D:\\project\\rloccle\\src\\tool.rs"]}"#,
            None,
        );

        assert!(call.starts_with("Find files matching:"));
        assert!(result.starts_with("Matched files"));
        assert!(!call.contains("{\""));
        assert!(!result.contains("{\""));
    }

    #[test]
    fn list_dir_result_summarizes_entries_without_json() {
        let result = format_tool_result(
            "list_dir",
            r#"{"entries":["D:\\project\\rloccle\\.agents","D:\\project\\rloccle\\.env"],"remaining_count":10}"#,
            None,
        );

        assert!(result.starts_with("Directory entries"));
        assert!(result.contains("10 more"));
        assert!(!result.contains("\"entries\""));
    }
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
                let full_cmd = format!("{} {}", cmd, args);
                let max_len = 160;
                let truncated_cmd = if full_cmd.chars().count() > max_len {
                    let mut truncated: String = full_cmd.chars().take(max_len).collect();
                    truncated.push_str("... (truncated)");
                    truncated
                } else {
                    full_cmd
                };
                format!("Execute command: {}{}", truncated_cmd, bg_suffix)
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
            "glob" => {
                let pattern = map.get("pattern").and_then(|v| v.as_str()).unwrap_or("");
                format!("Find files matching: {}", friendly_path(pattern))
            }
            "list_dir" => {
                let path = map.get("path").and_then(|v| v.as_str()).unwrap_or(".");
                format!("List directory: {}", friendly_path(path))
            }
            "file_stat" => {
                let path = map.get("path").and_then(|v| v.as_str()).unwrap_or("");
                format!("Inspect file: {}", friendly_path(path))
            }
            "mkdir" => {
                let path = map.get("path").and_then(|v| v.as_str()).unwrap_or("");
                format!("Create directory: {}", friendly_path(path))
            }
            "get_process_output" => {
                let pid = map.get("pid").and_then(|v| v.as_u64()).unwrap_or(0);
                format!("Check background process: PID {}", pid)
            }
            "kill_process" => {
                let pid = map.get("pid").and_then(|v| v.as_u64()).unwrap_or(0);
                format!("Stop background process: PID {}", pid)
            }
            _ => format_tool_fields(name, &map),
        },
        _ => format!("Run tool: {}\n{}", name, truncate_plain_text(args_str, 240)),
    }
}

fn format_tool_result(name: &str, result_str: &str, call_args_str: Option<&str>) -> String {
    let parsed: Result<serde_json::Value, _> = serde_json::from_str(result_str);
    match parsed {
        Ok(serde_json::Value::Object(map)) => match name {
            "execute_command" => {
                let stdout = map.get("stdout").and_then(|v| v.as_str()).unwrap_or("");
                let stderr = map.get("stderr").and_then(|v| v.as_str()).unwrap_or("");
                let pid = map.get("pid").and_then(|v| v.as_u64());
                let code = map.get("exit_code").and_then(|v| v.as_i64());

                if let Some(pid) = pid {
                    format!("Started background process\n  - PID: {}", pid)
                } else if code == Some(0) {
                    let output = stdout.trim();
                    if output.is_empty() {
                        "Command succeeded (exit code 0)".to_string()
                    } else {
                        format!(
                            "Command succeeded (exit code 0)\n{}",
                            truncate_plain_text(output, CMD_RESULT_PREVIEW_LIMIT)
                        )
                    }
                } else {
                    format!(
                        "Failed (exit code {})\nOutput:\n{}\nError:\n{}",
                        code.map(|v| v.to_string())
                            .unwrap_or_else(|| "unknown".to_string()),
                        truncate_plain_text(stdout.trim(), CMD_RESULT_PREVIEW_LIMIT),
                        truncate_plain_text(stderr.trim(), CMD_RESULT_PREVIEW_LIMIT)
                    )
                }
            }
            "get_process_output" => {
                let finished = map
                    .get("finished")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                let stdout = map.get("stdout").and_then(|v| v.as_str()).unwrap_or("");
                let stderr = map.get("stderr").and_then(|v| v.as_str()).unwrap_or("");
                let code = map.get("exit_code").and_then(|v| v.as_i64());

                if finished {
                    let mut lines = vec![format!(
                        "Background process finished{}",
                        code.map(|v| format!(" (exit code {})", v))
                            .unwrap_or_default()
                    )];
                    if !stdout.trim().is_empty() {
                        lines.push(format!(
                            "Output:\n{}",
                            truncate_plain_text(stdout.trim(), CMD_RESULT_PREVIEW_LIMIT)
                        ));
                    }
                    if !stderr.trim().is_empty() {
                        lines.push(format!(
                            "Error:\n{}",
                            truncate_plain_text(stderr.trim(), CMD_RESULT_PREVIEW_LIMIT)
                        ));
                    }
                    lines.join("\n")
                } else {
                    "Background process is still running".to_string()
                }
            }
            "read_file" => {
                let content = map.get("content").and_then(|v| v.as_str()).unwrap_or("");
                let line_count = content.lines().count();
                let char_count = content.chars().count();
                let path_str = call_args_str
                    .and_then(|args| serde_json::from_str::<serde_json::Value>(args).ok())
                    .and_then(|v| {
                        v.get("path")
                            .and_then(|p| p.as_str())
                            .map(|s| s.to_string())
                    })
                    .unwrap_or_else(|| "Unknown".to_string());

                format!(
                    "Read file successfully\n  - Path: {}\n  - Lines: {}\n  - Characters: {}",
                    friendly_path(&path_str),
                    line_count,
                    char_count
                )
            }
            "write_file" => {
                if map
                    .get("success")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false)
                {
                    "File written successfully".to_string()
                } else {
                    "File write finished with unknown status".to_string()
                }
            }
            "delete" => {
                if map
                    .get("success")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false)
                {
                    "Deleted successfully".to_string()
                } else {
                    "Delete finished with unknown status".to_string()
                }
            }
            "mkdir" => {
                if map
                    .get("success")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false)
                {
                    "Directory created successfully".to_string()
                } else {
                    "Directory creation finished with unknown status".to_string()
                }
            }
            "kill_process" => {
                if map
                    .get("success")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false)
                {
                    "Process stopped successfully".to_string()
                } else {
                    "Process stop finished with unknown status".to_string()
                }
            }
            "list_dir" => {
                let entries = json_string_array(&map, "entries");
                let remaining_count = map
                    .get("remaining_count")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0) as usize;
                format_path_list("Directory entries", &entries, remaining_count)
            }
            "glob" => {
                let paths = json_string_array(&map, "paths");
                if paths.is_empty() {
                    "No files matched the pattern".to_string()
                } else {
                    format_path_list("Matched files", &paths, 0)
                }
            }
            "grep" => {
                let matches = map
                    .get("matches")
                    .and_then(|v| v.as_array())
                    .cloned()
                    .unwrap_or_default();

                if matches.is_empty() {
                    "No matching lines found".to_string()
                } else {
                    let mut lines = vec![format!(
                        "Found {} matching line{}",
                        matches.len(),
                        if matches.len() == 1 { "" } else { "s" }
                    )];
                    for item in matches.iter().take(RESULT_LIST_LIMIT) {
                        let file = item.get("file").and_then(|v| v.as_str()).unwrap_or("");
                        let line = item.get("line").and_then(|v| v.as_u64()).unwrap_or(0);
                        let content = item.get("content").and_then(|v| v.as_str()).unwrap_or("");
                        lines.push(format!(
                            "  - {}:{} {}",
                            friendly_path(file),
                            line,
                            truncate_plain_text(content, 160)
                        ));
                    }
                    if matches.len() > RESULT_LIST_LIMIT {
                        lines.push(format!(
                            "  ... {} more not shown",
                            matches.len() - RESULT_LIST_LIMIT
                        ));
                    }
                    lines.join("\n")
                }
            }
            "file_stat" => {
                let size = map.get("size_bytes").and_then(|v| v.as_u64()).unwrap_or(0);
                let is_dir = map.get("is_dir").and_then(|v| v.as_bool()).unwrap_or(false);
                let is_file = map
                    .get("is_file")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                let modified = map
                    .get("modified_time")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown");
                let kind = if is_dir {
                    "directory"
                } else if is_file {
                    "file"
                } else {
                    "path"
                };
                format!(
                    "File info\n  - Type: {}\n  - Size: {} bytes\n  - Modified: {}",
                    kind, size, modified
                )
            }
            "task_write" | "task_check" => {
                if let Some(tasks) = map.get("tasks").and_then(|v| v.as_array()) {
                    let mut lines = vec!["Task list status:".to_string()];
                    for t in tasks {
                        let id = t.get("id").and_then(|v| v.as_str()).unwrap_or("");
                        let content = t.get("content").and_then(|v| v.as_str()).unwrap_or("");
                        let status = t.get("status").and_then(|v| v.as_str()).unwrap_or("");
                        let status_icon = match status {
                            "completed" => "[x]",
                            "in_progress" => "[>]",
                            "failed" | "error" => "[!]",
                            _ => "[-]",
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
                    format_tool_fields(name, &map)
                }
            }
            _ => format_tool_fields(name, &map),
        },
        _ => truncate_result(result_str),
    }
}

fn truncate_result(result_str: &str) -> String {
    if result_str.chars().count() > RESULT_PREVIEW_LIMIT {
        format!(
            "Result preview (truncated to {} of {} characters):\n{}",
            RESULT_PREVIEW_LIMIT,
            result_str.chars().count(),
            truncate_plain_text(result_str, RESULT_PREVIEW_LIMIT)
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
