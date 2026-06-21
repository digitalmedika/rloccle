use crate::tool::{BoxError, create_tool};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
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
        },
    )
}

// --- Read File Range Tool ---

#[derive(JsonSchema, Deserialize, Debug)]
pub struct ReadFileRangeInput {
    /// The path to the file to read
    pub path: String,
    /// The first 1-based line number to include
    pub start_line: usize,
    /// The last 1-based line number to include
    pub end_line: usize,
}

#[derive(JsonSchema, Serialize, Debug)]
pub struct ReadFileRangeOutput {
    pub content: String,
    pub total_lines: usize,
}

pub fn read_file_range_tool() -> impl crate::Tool {
    create_tool::<ReadFileRangeInput, ReadFileRangeOutput, _, _>(
        "read_file_range",
        "Reads a specific inclusive 1-based line range from a file",
        |args| async move {
            if args.start_line == 0 {
                return Err(simple_error("start_line must be greater than 0"));
            }
            if args.end_line < args.start_line {
                return Err(simple_error(
                    "end_line must be greater than or equal to start_line",
                ));
            }

            let content = fs::read_to_string(&args.path)?;
            let lines: Vec<&str> = content.lines().collect();
            let total_lines = lines.len();
            let selected = lines
                .iter()
                .enumerate()
                .filter_map(|(idx, line)| {
                    let line_number = idx + 1;
                    (line_number >= args.start_line && line_number <= args.end_line)
                        .then_some(*line)
                })
                .collect::<Vec<_>>()
                .join("\n");

            Ok(ReadFileRangeOutput {
                content: selected,
                total_lines,
            })
        },
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
        },
    )
}

// --- Replace In File Tool ---

#[derive(JsonSchema, Deserialize, Debug)]
pub struct ReplaceInFileInput {
    /// The path to the file to modify
    pub path: String,
    /// Exact text to search for
    pub old: String,
    /// Replacement text
    pub new: String,
    /// Optional exact number of replacements expected. If supplied and it does not match, the file is not changed.
    pub expected_replacements: Option<usize>,
}

#[derive(JsonSchema, Serialize, Debug)]
pub struct ReplaceInFileOutput {
    pub success: bool,
    pub replacements: usize,
}

pub fn replace_in_file_tool() -> impl crate::Tool {
    create_tool::<ReplaceInFileInput, ReplaceInFileOutput, _, _>(
        "replace_in_file",
        "Replaces exact text in a file, optionally validating the expected replacement count before writing",
        |args| async move {
            if args.old.is_empty() {
                return Err(simple_error("old text must not be empty"));
            }

            let content = fs::read_to_string(&args.path)?;
            let replacements = content.matches(&args.old).count();
            if replacements == 0 {
                return Err(simple_error("old text not found"));
            }
            if let Some(expected) = args.expected_replacements {
                if replacements != expected {
                    return Err(simple_error(format!(
                        "expected {expected} replacements, found {replacements}"
                    )));
                }
            }

            let updated = content.replace(&args.old, &args.new);
            fs::write(&args.path, updated)?;
            Ok(ReplaceInFileOutput {
                success: true,
                replacements,
            })
        },
    )
}

// --- Replace Lines Tool ---

#[derive(JsonSchema, Deserialize, Debug)]
pub struct ReplaceLinesInput {
    /// The path to the file to modify
    pub path: String,
    /// The first 1-based line number to replace
    pub start_line: usize,
    /// The last 1-based line number to replace, inclusive
    pub end_line: usize,
    /// Replacement text for the selected line range
    pub replacement: String,
}

#[derive(JsonSchema, Serialize, Debug)]
pub struct ReplaceLinesOutput {
    pub success: bool,
    pub replaced_lines: usize,
}

pub fn replace_lines_tool() -> impl crate::Tool {
    create_tool::<ReplaceLinesInput, ReplaceLinesOutput, _, _>(
        "replace_lines",
        "Replaces an inclusive 1-based line range in a file with the provided text",
        |args| async move {
            if args.start_line == 0 {
                return Err(simple_error("start_line must be greater than 0"));
            }
            if args.end_line < args.start_line {
                return Err(simple_error(
                    "end_line must be greater than or equal to start_line",
                ));
            }

            let content = fs::read_to_string(&args.path)?;
            let had_trailing_newline = content.ends_with('\n');
            let mut lines: Vec<String> = content.lines().map(ToString::to_string).collect();
            if args.end_line > lines.len() {
                return Err(simple_error(format!(
                    "end_line {} is greater than total lines {}",
                    args.end_line,
                    lines.len()
                )));
            }

            let replacement_lines: Vec<String> = if args.replacement.is_empty() {
                Vec::new()
            } else {
                args.replacement.lines().map(ToString::to_string).collect()
            };
            let replaced_lines = args.end_line - args.start_line + 1;
            lines.splice((args.start_line - 1)..args.end_line, replacement_lines);

            let mut updated = lines.join("\n");
            if had_trailing_newline && !updated.ends_with('\n') {
                updated.push('\n');
            }
            fs::write(&args.path, updated)?;
            Ok(ReplaceLinesOutput {
                success: true,
                replaced_lines,
            })
        },
    )
}

// --- List Dir Tool ---

#[derive(JsonSchema, Deserialize, Debug)]
pub struct ListDirInput {
    /// The path to the directory to list (defaults to "." if not specified)
    pub path: Option<String>,
    /// Maximum number of entries to return (defaults to 4)
    pub limit: Option<usize>,
}

#[derive(JsonSchema, Serialize, Debug)]
pub struct ListDirOutput {
    /// Files/directories in the requested directory, capped by the requested limit.
    pub entries: Vec<String>,
    /// Number of additional files/directories that were not included in `entries`.
    pub remaining_count: usize,
}

pub fn list_dir_tool() -> impl crate::Tool {
    create_tool::<ListDirInput, ListDirOutput, _, _>(
        "list_dir",
        "Lists files and directories in a given path, capped by an optional limit, plus the count of remaining entries",
        |args| async move {
            const DEFAULT_DISPLAY_LIMIT: usize = 4;
            let display_limit = args.limit.unwrap_or(DEFAULT_DISPLAY_LIMIT);

            let target_path = args.path.unwrap_or_else(|| ".".to_string());
            let mut all_entries = Vec::new();
            for entry in fs::read_dir(target_path)? {
                let entry = entry?;
                let path = entry.path();
                let path_str = path.to_string_lossy().into_owned();
                all_entries.push(path_str);
            }
            all_entries.sort();

            let total_count = all_entries.len();
            let remaining_count = total_count.saturating_sub(display_limit);
            let entries = all_entries.into_iter().take(display_limit).collect();

            Ok(ListDirOutput {
                entries,
                remaining_count,
            })
        },
    )
}

// --- Grep Tool ---

#[derive(JsonSchema, Deserialize, Debug)]
pub struct GrepInput {
    /// The search term or literal pattern to find
    pub query: String,
    /// The directory or file path to search within (defaults to "." if not specified)
    pub path: Option<String>,
    /// Maximum number of matches to return (defaults to 1000)
    pub max_matches: Option<usize>,
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
        "Searches for a text pattern recursively in files inside a directory or a specific file, ignoring common build/dependency directories",
        |args| async move {
            const DEFAULT_MAX_MATCHES: usize = 1000;
            let start_path = args.path.unwrap_or_else(|| ".".to_string());
            let query = args.query.to_lowercase();
            let max_matches = args.max_matches.unwrap_or(DEFAULT_MAX_MATCHES);
            let mut matches = Vec::new();

            for entry in walkdir::WalkDir::new(start_path)
                .into_iter()
                .filter_entry(|entry| !is_ignored_dir(entry.path()))
                .filter_map(|e| e.ok())
            {
                if matches.len() >= max_matches {
                    break;
                }

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

                                if matches.len() >= max_matches {
                                    break;
                                }
                            }
                        }
                    }
                }
            }
            Ok(GrepOutput { matches })
        },
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
        },
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
        },
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
        },
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
        },
    )
}

fn simple_error(message: impl Into<String>) -> BoxError {
    Box::new(std::io::Error::new(
        std::io::ErrorKind::InvalidInput,
        message.into(),
    ))
}

fn is_ignored_dir(path: &Path) -> bool {
    if !path.is_dir() {
        return false;
    }

    matches!(
        path.file_name().and_then(|name| name.to_str()),
        Some(".git" | "target" | "node_modules" | ".next" | "dist" | "build")
    )
}
