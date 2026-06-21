use crate::tool::create_tool;
use once_cell::sync::Lazy;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;

static PROCESSES: Lazy<Mutex<HashMap<u32, Child>>> = Lazy::new(|| Mutex::new(HashMap::new()));

// --- Execute Command Tool ---

#[derive(JsonSchema, Deserialize, Debug)]
pub struct ExecuteCommandInput {
    /// The command to execute (e.g. "cargo" or "cmd" on Windows)
    pub command: String,
    /// Arguments to pass to the command
    #[serde(default)]
    pub args: Vec<String>,
    /// If true, execute the command in the background and return the PID immediately
    #[serde(default)]
    pub background: bool,
}

#[derive(JsonSchema, Serialize, Debug)]
pub struct ExecuteCommandOutput {
    pub pid: Option<u32>,
    pub exit_code: Option<i32>,
    pub stdout: Option<String>,
    pub stderr: Option<String>,
}

pub fn execute_command_tool() -> impl crate::Tool {
    create_tool::<ExecuteCommandInput, ExecuteCommandOutput, _, _>(
        "execute_command",
        "Executes a system command synchronously or in the background",
        |args| async move {
            if args.background {
                let child = Command::new(&args.command)
                    .args(&args.args)
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped())
                    .spawn()?;
                let pid = child.id();

                {
                    let mut lock = PROCESSES.lock().unwrap();
                    lock.insert(pid, child);
                }

                Ok(ExecuteCommandOutput {
                    pid: Some(pid),
                    exit_code: None,
                    stdout: None,
                    stderr: None,
                })
            } else {
                let output = Command::new(&args.command).args(&args.args).output()?;

                let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
                let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
                let exit_code = output.status.code();

                Ok(ExecuteCommandOutput {
                    pid: None,
                    exit_code,
                    stdout: Some(stdout),
                    stderr: Some(stderr),
                })
            }
        },
    )
}

// --- Get Process Output Tool ---

#[derive(JsonSchema, Deserialize, Debug)]
pub struct GetProcessOutputInput {
    /// The PID of the background process to inspect
    pub pid: u32,
}

#[derive(JsonSchema, Serialize, Debug)]
pub struct GetProcessOutputOutput {
    pub finished: bool,
    pub exit_code: Option<i32>,
    pub stdout: Option<String>,
    pub stderr: Option<String>,
}

pub fn get_process_output_tool() -> impl crate::Tool {
    create_tool::<GetProcessOutputInput, GetProcessOutputOutput, _, _>(
        "get_process_output",
        "Checks status of a running background process. If finished, retrieves outputs.",
        |args| async move {
            let mut child = {
                let mut lock = PROCESSES.lock().unwrap();
                if lock.contains_key(&args.pid) {
                    lock.remove(&args.pid).unwrap()
                } else {
                    return Err(format!(
                        "Process with PID {} not found or has already been collected",
                        args.pid
                    )
                    .into());
                }
            };

            // Check status without blocking
            match child.try_wait()? {
                Some(status) => {
                    // Process is finished. We can wait for outputs.
                    let output = child.wait_with_output()?;
                    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
                    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
                    Ok(GetProcessOutputOutput {
                        finished: true,
                        exit_code: status.code(),
                        stdout: Some(stdout),
                        stderr: Some(stderr),
                    })
                }
                None => {
                    // Process is still running. Put it back in the map.
                    {
                        let mut lock = PROCESSES.lock().unwrap();
                        lock.insert(args.pid, child);
                    }
                    Ok(GetProcessOutputOutput {
                        finished: false,
                        exit_code: None,
                        stdout: None,
                        stderr: None,
                    })
                }
            }
        },
    )
}

// --- Kill Process Tool ---

#[derive(JsonSchema, Deserialize, Debug)]
pub struct KillProcessInput {
    /// The PID of the process to terminate
    pub pid: u32,
}

#[derive(JsonSchema, Serialize, Debug)]
pub struct KillProcessOutput {
    pub success: bool,
}

pub fn kill_process_tool() -> impl crate::Tool {
    create_tool::<KillProcessInput, KillProcessOutput, _, _>(
        "kill_process",
        "Terminates a background process by PID",
        |args| async move {
            let mut lock = PROCESSES.lock().unwrap();
            if let Some(mut child) = lock.remove(&args.pid) {
                child.kill()?;
                Ok(KillProcessOutput { success: true })
            } else {
                Err(format!("Process with PID {} not found", args.pid).into())
            }
        },
    )
}
