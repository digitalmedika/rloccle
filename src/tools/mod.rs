pub mod fs;
pub mod system;
pub mod task;

use crate::tool::Tool;
use std::sync::Arc;

/// Returns all built-in filesystem tools (read_file, read_file_range, write_file, replace_in_file,
/// replace_lines, list_dir, grep, glob, delete, file_stat, mkdir) as a Vector of Arc<dyn Tool>.
pub fn all_fs_tools() -> Vec<Arc<dyn Tool>> {
    vec![
        Arc::new(fs::read_file_tool()),
        Arc::new(fs::read_file_range_tool()),
        Arc::new(fs::write_file_tool()),
        Arc::new(fs::replace_in_file_tool()),
        Arc::new(fs::replace_lines_tool()),
        Arc::new(fs::list_dir_tool()),
        Arc::new(fs::grep_tool()),
        Arc::new(fs::glob_tool()),
        Arc::new(fs::delete_tool()),
        Arc::new(fs::file_stat_tool()),
        Arc::new(fs::mkdir_tool()),
    ]
}

/// Returns all built-in system tools (execute_command, get_process_output, kill_process)
/// as a Vector of Arc<dyn Tool>.
pub fn all_system_tools() -> Vec<Arc<dyn Tool>> {
    vec![
        Arc::new(system::execute_command_tool()),
        Arc::new(system::get_process_output_tool()),
        Arc::new(system::kill_process_tool()),
    ]
}

/// Returns all built-in task list tools (task_write, task_update, task_complete, task_check)
/// as a Vector of Arc<dyn Tool>.
pub fn all_task_tools() -> Vec<Arc<dyn Tool>> {
    vec![
        Arc::new(task::task_write_tool()),
        Arc::new(task::task_update_tool()),
        Arc::new(task::task_complete_tool()),
        Arc::new(task::task_check_tool()),
    ]
}
