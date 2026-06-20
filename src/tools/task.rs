use crate::tool::create_tool;
use crate::memory::Memory;
use crate::tool::Tool;
use serde::{Deserialize, Serialize};
use schemars::JsonSchema;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Task {
    pub id: String,
    pub content: String,
    pub status: String,
    #[serde(rename = "activeForm")]
    pub active_form: String,
}

#[derive(Clone)]
pub struct ExecutionContext {
    pub thread_id: Option<String>,
    pub resource_id: Option<String>,
    pub memory: Option<Memory>,
}

tokio::task_local! {
    pub static CURRENT_CONTEXT: ExecutionContext;
}

// --- Task Write Tool ---

#[derive(JsonSchema, Serialize, Deserialize, Debug)]
pub struct TaskInput {
    pub id: Option<String>,
    pub content: String,
    pub status: Option<String>,
    #[serde(rename = "activeForm")]
    pub active_form: Option<String>,
}

#[derive(JsonSchema, Serialize, Deserialize, Debug)]
pub struct TaskWriteInput {
    pub tasks: Vec<TaskInput>,
}

#[derive(JsonSchema, Serialize, Deserialize, Debug)]
pub struct TaskWriteOutput {
    pub success: bool,
    pub error: Option<String>,
    pub tasks: Vec<Task>,
}

pub fn task_write_tool() -> impl Tool {
    create_tool::<TaskWriteInput, TaskWriteOutput, _, _>(
        "task_write",
        "Create or replace a structured task list. Stores the tasks in thread memory.",
        |args| async move {
            let ctx = match CURRENT_CONTEXT.try_with(|c| c.clone()) {
                Ok(c) => c,
                Err(_) => {
                    return Ok(TaskWriteOutput {
                        success: false,
                        error: Some("No active execution context found".to_string()),
                        tasks: vec![],
                    });
                }
            };

            let thread_id = match ctx.thread_id {
                Some(tid) => tid,
                None => {
                    return Ok(TaskWriteOutput {
                        success: false,
                        error: Some("Task tracking requires an active thread_id".to_string()),
                        tasks: vec![],
                    });
                }
            };

            let memory = match ctx.memory {
                Some(mem) => mem,
                None => {
                    return Ok(TaskWriteOutput {
                        success: false,
                        error: Some("Task tracking requires agent memory".to_string()),
                        tasks: vec![],
                    });
                }
            };

            let mut final_tasks = Vec::new();
            for (idx, task_in) in args.tasks.into_iter().enumerate() {
                let id = task_in.id.unwrap_or_else(|| format!("task-{}", idx + 1));
                let status = task_in.status.unwrap_or_else(|| "pending".to_string());
                let active_form = task_in.active_form.unwrap_or_else(|| {
                    format!("Working on: {}", task_in.content)
                });
                final_tasks.push(Task {
                    id,
                    content: task_in.content,
                    status,
                    active_form,
                });
            }

            let value = match serde_json::to_value(&final_tasks) {
                Ok(v) => v,
                Err(e) => {
                    return Ok(TaskWriteOutput {
                        success: false,
                        error: Some(format!("Failed to serialize tasks: {}", e)),
                        tasks: vec![],
                    });
                }
            };

            if let Err(e) = memory.storage().update_thread_state(&thread_id, "tasks".to_string(), value).await {
                return Ok(TaskWriteOutput {
                    success: false,
                    error: Some(format!("Failed to save tasks: {}", e)),
                    tasks: vec![],
                });
            }

            Ok(TaskWriteOutput {
                success: true,
                error: None,
                tasks: final_tasks,
            })
        }
    )
}

// --- Task Update Tool ---

#[derive(JsonSchema, Serialize, Deserialize, Debug)]
pub struct TaskUpdateInput {
    pub id: String,
    pub content: Option<String>,
    pub status: Option<String>,
    #[serde(rename = "activeForm")]
    pub active_form: Option<String>,
}

#[derive(JsonSchema, Serialize, Deserialize, Debug)]
pub struct TaskUpdateOutput {
    pub success: bool,
    pub error: Option<String>,
    pub task: Option<Task>,
}

pub fn task_update_tool() -> impl Tool {
    create_tool::<TaskUpdateInput, TaskUpdateOutput, _, _>(
        "task_update",
        "Update one tracked task by its ID (e.g., status, content, activeForm).",
        |args| async move {
            let ctx = match CURRENT_CONTEXT.try_with(|c| c.clone()) {
                Ok(c) => c,
                Err(_) => {
                    return Ok(TaskUpdateOutput {
                        success: false,
                        error: Some("No active execution context found".to_string()),
                        task: None,
                    });
                }
            };

            let thread_id = match ctx.thread_id {
                Some(tid) => tid,
                None => {
                    return Ok(TaskUpdateOutput {
                        success: false,
                        error: Some("Task tracking requires an active thread_id".to_string()),
                        task: None,
                    });
                }
            };

            let memory = match ctx.memory {
                Some(mem) => mem,
                None => {
                    return Ok(TaskUpdateOutput {
                        success: false,
                        error: Some("Task tracking requires agent memory".to_string()),
                        task: None,
                    });
                }
            };

            let session = match memory.storage().get_thread(&thread_id).await {
                Ok(Some(s)) => s,
                Ok(None) | Err(_) => {
                    return Ok(TaskUpdateOutput {
                        success: false,
                        error: Some("Thread session not found".to_string()),
                        task: None,
                    });
                }
            };

            let mut tasks: Vec<Task> = session.state.get("tasks")
                .and_then(|v| serde_json::from_value(v.clone()).ok())
                .unwrap_or_default();

            let mut updated_task = None;
            for task in &mut tasks {
                if task.id == args.id {
                    if let Some(content) = &args.content {
                        task.content = content.clone();
                    }
                    if let Some(status) = &args.status {
                        task.status = status.clone();
                    }
                    if let Some(active_form) = &args.active_form {
                        task.active_form = active_form.clone();
                    }
                    updated_task = Some(task.clone());
                    break;
                }
            }

            if updated_task.is_none() {
                return Ok(TaskUpdateOutput {
                    success: false,
                    error: Some(format!("Task with ID '{}' not found", args.id)),
                    task: None,
                });
            }

            let value = serde_json::to_value(&tasks).unwrap();
            if let Err(e) = memory.storage().update_thread_state(&thread_id, "tasks".to_string(), value).await {
                return Ok(TaskUpdateOutput {
                    success: false,
                    error: Some(format!("Failed to save tasks: {}", e)),
                    task: None,
                });
            }

            Ok(TaskUpdateOutput {
                success: true,
                error: None,
                task: updated_task,
            })
        }
    )
}

// --- Task Complete Tool ---

#[derive(JsonSchema, Serialize, Deserialize, Debug)]
pub struct TaskCompleteInput {
    pub id: String,
}

#[derive(JsonSchema, Serialize, Deserialize, Debug)]
pub struct TaskCompleteOutput {
    pub success: bool,
    pub error: Option<String>,
    pub task: Option<Task>,
}

pub fn task_complete_tool() -> impl Tool {
    create_tool::<TaskCompleteInput, TaskCompleteOutput, _, _>(
        "task_complete",
        "Mark one tracked task as completed by its ID.",
        |args| async move {
            let ctx = match CURRENT_CONTEXT.try_with(|c| c.clone()) {
                Ok(c) => c,
                Err(_) => {
                    return Ok(TaskCompleteOutput {
                        success: false,
                        error: Some("No active execution context found".to_string()),
                        task: None,
                    });
                }
            };

            let thread_id = match ctx.thread_id {
                Some(tid) => tid,
                None => {
                    return Ok(TaskCompleteOutput {
                        success: false,
                        error: Some("Task tracking requires an active thread_id".to_string()),
                        task: None,
                    });
                }
            };

            let memory = match ctx.memory {
                Some(mem) => mem,
                None => {
                    return Ok(TaskCompleteOutput {
                        success: false,
                        error: Some("Task tracking requires agent memory".to_string()),
                        task: None,
                    });
                }
            };

            let session = match memory.storage().get_thread(&thread_id).await {
                Ok(Some(s)) => s,
                Ok(None) | Err(_) => {
                    return Ok(TaskCompleteOutput {
                        success: false,
                        error: Some("Thread session not found".to_string()),
                        task: None,
                    });
                }
            };

            let mut tasks: Vec<Task> = session.state.get("tasks")
                .and_then(|v| serde_json::from_value(v.clone()).ok())
                .unwrap_or_default();

            let mut updated_task = None;
            for task in &mut tasks {
                if task.id == args.id {
                    task.status = "completed".to_string();
                    updated_task = Some(task.clone());
                    break;
                }
            }

            if updated_task.is_none() {
                return Ok(TaskCompleteOutput {
                    success: false,
                    error: Some(format!("Task with ID '{}' not found", args.id)),
                    task: None,
                });
            }

            let value = serde_json::to_value(&tasks).unwrap();
            if let Err(e) = memory.storage().update_thread_state(&thread_id, "tasks".to_string(), value).await {
                return Ok(TaskCompleteOutput {
                    success: false,
                    error: Some(format!("Failed to save tasks: {}", e)),
                    task: None,
                });
            }

            Ok(TaskCompleteOutput {
                success: true,
                error: None,
                task: updated_task,
            })
        }
    )
}

// --- Task Check Tool ---

#[derive(JsonSchema, Serialize, Deserialize, Debug)]
pub struct TaskCheckInput {}

#[derive(JsonSchema, Serialize, Deserialize, Debug)]
pub struct TaskCheckOutput {
    pub success: bool,
    pub error: Option<String>,
    pub tasks: Vec<Task>,
    pub all_completed: bool,
}

pub fn task_check_tool() -> impl Tool {
    create_tool::<TaskCheckInput, TaskCheckOutput, _, _>(
        "task_check",
        "Check task list and overall completion status.",
        |_args| async move {
            let ctx = match CURRENT_CONTEXT.try_with(|c| c.clone()) {
                Ok(c) => c,
                Err(_) => {
                    return Ok(TaskCheckOutput {
                        success: false,
                        error: Some("No active execution context found".to_string()),
                        tasks: vec![],
                        all_completed: false,
                    });
                }
            };

            let thread_id = match ctx.thread_id {
                Some(tid) => tid,
                None => {
                    return Ok(TaskCheckOutput {
                        success: false,
                        error: Some("Task tracking requires an active thread_id".to_string()),
                        tasks: vec![],
                        all_completed: false,
                    });
                }
            };

            let memory = match ctx.memory {
                Some(mem) => mem,
                None => {
                    return Ok(TaskCheckOutput {
                        success: false,
                        error: Some("Task tracking requires agent memory".to_string()),
                        tasks: vec![],
                        all_completed: false,
                    });
                }
            };

            let session = match memory.storage().get_thread(&thread_id).await {
                Ok(Some(s)) => s,
                Ok(None) | Err(_) => {
                    return Ok(TaskCheckOutput {
                        success: false,
                        error: Some("Thread session not found".to_string()),
                        tasks: vec![],
                        all_completed: false,
                    });
                }
            };

            let tasks: Vec<Task> = session.state.get("tasks")
                .and_then(|v| serde_json::from_value(v.clone()).ok())
                .unwrap_or_default();

            let all_completed = !tasks.is_empty() && tasks.iter().all(|t| t.status == "completed");

            Ok(TaskCheckOutput {
                success: true,
                error: None,
                tasks,
                all_completed,
            })
        }
    )
}
