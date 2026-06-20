use crate::tool::create_tool;
use serde::{Deserialize, Serialize};
use schemars::JsonSchema;
use std::fs;
use std::path::Path;

// --- Read File Tool ---

#[derive(JsonSchema, Deserialize, Debug)]
pub struct ReadFileInput {
    /// The path to the file to read
    pub path: String,
}

#[derive(JsonSchema, Serialize, Debug)]
pub struct ReadFileOutput {
    pub content: String,
}

pub fn read_file_tool() -> impl crate::Tool {
    create_tool::<ReadFileInput, ReadFileOutput, _, _>(
        "read_file",
        "Reads the contents of a file from the filesystem",
        |args| async move {
            let content = fs::read_to_string(&args.path)?;
            Ok(ReadFileOutput { content })
        }
    )
}

// --- Write File Tool ---

#[derive(JsonSchema, Deserialize, Debug)]
pub struct WriteFileInput {
    /// The path to the file to write
    pub path: String,
    /// The text content to write to the file
    pub content: String,
}

#[derive(JsonSchema, Serialize, Debug)]
pub struct WriteFileOutput {
    pub success: bool,
}

pub fn write_file_tool() -> impl crate::Tool {
    create_tool::<WriteFileInput, WriteFileOutput, _, _>(
        "write_file",
        "Writes content to a file, creating it if it doesn't exist. Also creates parent directories if needed.",
        |args| async move {
            let path = Path::new(&args.path);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(path, &args.content)?;
            Ok(WriteFileOutput { success: true })
        }
    )
}

// --- List Dir Tool ---

#[derive(JsonSchema, Deserialize, Debug)]
pub struct ListDirInput {
    /// The path to the directory to list (defaults to "." if not specified)
    pub path: Option<String>,
}

#[derive(JsonSchema, Serialize, Debug)]
pub struct ListDirOutput {
    /// Up to the first 4 files/directories in the requested directory.
    pub entries: Vec<String>,
    /// Number of additional files/directories that were not included in `entries`.
    pub remaining_count: usize,
}

pub fn list_dir_tool() -> impl crate::Tool {
    create_tool::<ListDirInput, ListDirOutput, _, _>(
        "list_dir",
        "Lists up to 4 files and directories in a given path, plus the count of remaining entries",
        |args| async move {
            const DISPLAY_LIMIT: usize = 4;

            let target_path = args.path.unwrap_or_else(|| ".".to_string());
            let mut all_entries = Vec::new();
            for entry in fs::read_dir(target_path)? {
                let entry = entry?;
                let path = entry.path();
                let path_str = path.to_string_lossy().into_owned();
                all_entries.push(path_str);
            }

            let total_count = all_entries.len();
            let remaining_count = total_count.saturating_sub(DISPLAY_LIMIT);
            let entries = all_entries.into_iter().take(DISPLAY_LIMIT).collect();

            Ok(ListDirOutput {
                entries,
                remaining_count,
            })
        }
    )
}

// --- Grep Tool ---

#[derive(JsonSchema, Deserialize, Debug)]
pub struct GrepInput {
    /// The search term or literal pattern to find
    pub query: String,
    /// The directory or file path to search within (defaults to "." if not specified)
    pub path: Option<String>,
}

#[derive(JsonSchema, Serialize, Debug, Clone)]
pub struct GrepMatch {
    pub file: String,
    pub line: usize,
    pub content: String,
}

#[derive(JsonSchema, Serialize, Debug)]
pub struct GrepOutput {
    pub matches: Vec<GrepMatch>,
}

pub fn grep_tool() -> impl crate::Tool {
    create_tool::<GrepInput, GrepOutput, _, _>(
        "grep",
        "Searches for a text pattern recursively in files inside a directory or a specific file",
        |args| async move {
            let start_path = args.path.unwrap_or_else(|| ".".to_string());
            let query = args.query.to_lowercase();
            let mut matches = Vec::new();

            for entry in walkdir::WalkDir::new(start_path)
                .into_iter()
                .filter_map(|e| e.ok())
            {
                let path = entry.path();
                if path.is_file() {
                    if let Ok(content) = fs::read_to_string(path) {
                        for (idx, line) in content.lines().enumerate() {
                            if line.to_lowercase().contains(&query) {
                                matches.push(GrepMatch {
                                    file: path.to_string_lossy().into_owned(),
                                    line: idx + 1,
                                    content: line.trim().to_string(),
                                });
                            }
                        }
                    }
                }
            }
            Ok(GrepOutput { matches })
        }
    )
}

// --- Glob Tool ---

#[derive(JsonSchema, Deserialize, Debug)]
pub struct GlobInput {
    /// The glob pattern (e.g. "**/*.rs", "src/**/*.rs")
    pub pattern: String,
}

#[derive(JsonSchema, Serialize, Debug)]
pub struct GlobOutput {
    pub paths: Vec<String>,
}

pub fn glob_tool() -> impl crate::Tool {
    create_tool::<GlobInput, GlobOutput, _, _>(
        "glob",
        "Finds file paths matching a given glob pattern",
        |args| async move {
            let mut paths = Vec::new();
            for entry in glob::glob(&args.pattern)? {
                let path = entry?;
                paths.push(path.to_string_lossy().into_owned());
            }
            Ok(GlobOutput { paths })
        }
    )
}

// --- Delete Tool ---

#[derive(JsonSchema, Deserialize, Debug)]
pub struct DeleteInput {
    /// The path to the file or directory to delete
    pub path: String,
    /// If true and the path is a directory, recursively delete all its contents
    #[serde(default)]
    pub recursive: bool,
}

#[derive(JsonSchema, Serialize, Debug)]
pub struct DeleteOutput {
    pub success: bool,
}

pub fn delete_tool() -> impl crate::Tool {
    create_tool::<DeleteInput, DeleteOutput, _, _>(
        "delete",
        "Deletes a file or directory from the filesystem",
        |args| async move {
            let path = Path::new(&args.path);
            if path.is_dir() {
                if args.recursive {
                    fs::remove_dir_all(path)?;
                } else {
                    fs::remove_dir(path)?;
                }
            } else {
                fs::remove_file(path)?;
            }
            Ok(DeleteOutput { success: true })
        }
    )
}

// --- File Stat Tool ---

#[derive(JsonSchema, Deserialize, Debug)]
pub struct FileStatInput {
    /// The path to the file or directory to get stats for
    pub path: String,
}

#[derive(JsonSchema, Serialize, Debug)]
pub struct FileStatOutput {
    pub size_bytes: u64,
    pub is_dir: bool,
    pub is_file: bool,
    pub modified_time: String,
}

pub fn file_stat_tool() -> impl crate::Tool {
    create_tool::<FileStatInput, FileStatOutput, _, _>(
        "file_stat",
        "Retrieves size, type, and modified time for a file or directory",
        |args| async move {
            let path = Path::new(&args.path);
            let metadata = fs::metadata(path)?;
            let size_bytes = metadata.len();
            let is_dir = metadata.is_dir();
            let is_file = metadata.is_file();
            
            let modified_time = match metadata.modified() {
                Ok(time) => format!("{:?}", time),
                Err(_) => "unknown".to_string(),
            };
            
            Ok(FileStatOutput {
                size_bytes,
                is_dir,
                is_file,
                modified_time,
            })
        }
    )
}

// --- Mkdir Tool ---

#[derive(JsonSchema, Deserialize, Debug)]
pub struct MkdirInput {
    /// The path of the directory to create
    pub path: String,
}

#[derive(JsonSchema, Serialize, Debug)]
pub struct MkdirOutput {
    pub success: bool,
}

pub fn mkdir_tool() -> impl crate::Tool {
    create_tool::<MkdirInput, MkdirOutput, _, _>(
        "mkdir",
        "Creates a new directory (and intermediate parent directories if missing)",
        |args| async move {
            fs::create_dir_all(&args.path)?;
            Ok(MkdirOutput { success: true })
        }
    )
}
