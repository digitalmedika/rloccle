use crate::openai::ChatMessage;
use crate::tool::{BoxError, BoxFuture};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThreadSession {
    pub thread_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resource_id: Option<String>,
    pub messages: Vec<ChatMessage>,
    #[serde(default)]
    pub state: HashMap<String, serde_json::Value>,
}

pub trait Storage: Send + Sync {
    fn get_messages<'a>(
        &'a self,
        thread_id: &'a str,
    ) -> BoxFuture<'a, Result<Vec<ChatMessage>, BoxError>>;
    fn save_messages<'a>(
        &'a self,
        thread_id: &'a str,
        messages: Vec<ChatMessage>,
    ) -> BoxFuture<'a, Result<(), BoxError>>;
    fn get_thread<'a>(
        &'a self,
        thread_id: &'a str,
    ) -> BoxFuture<'a, Result<Option<ThreadSession>, BoxError>>;
    fn create_thread<'a>(
        &'a self,
        thread_id: &'a str,
        resource_id: Option<String>,
    ) -> BoxFuture<'a, Result<ThreadSession, BoxError>>;
    fn update_thread_state<'a>(
        &'a self,
        thread_id: &'a str,
        key: String,
        value: serde_json::Value,
    ) -> BoxFuture<'a, Result<(), BoxError>>;
}

pub struct InMemoryStorage {
    threads: Mutex<HashMap<String, ThreadSession>>,
}

impl InMemoryStorage {
    pub fn new() -> Self {
        Self {
            threads: Mutex::new(HashMap::new()),
        }
    }
}

impl Default for InMemoryStorage {
    fn default() -> Self {
        Self::new()
    }
}

impl Storage for InMemoryStorage {
    fn get_messages<'a>(
        &'a self,
        thread_id: &'a str,
    ) -> BoxFuture<'a, Result<Vec<ChatMessage>, BoxError>> {
        Box::pin(async move {
            let threads = self.threads.lock().unwrap();
            if let Some(session) = threads.get(thread_id) {
                Ok(session.messages.clone())
            } else {
                Ok(Vec::new())
            }
        })
    }

    fn save_messages<'a>(
        &'a self,
        thread_id: &'a str,
        messages: Vec<ChatMessage>,
    ) -> BoxFuture<'a, Result<(), BoxError>> {
        Box::pin(async move {
            let mut threads = self.threads.lock().unwrap();
            if let Some(session) = threads.get_mut(thread_id) {
                session.messages = messages;
            } else {
                threads.insert(
                    thread_id.to_string(),
                    ThreadSession {
                        thread_id: thread_id.to_string(),
                        resource_id: None,
                        messages,
                        state: HashMap::new(),
                    },
                );
            }
            Ok(())
        })
    }

    fn get_thread<'a>(
        &'a self,
        thread_id: &'a str,
    ) -> BoxFuture<'a, Result<Option<ThreadSession>, BoxError>> {
        Box::pin(async move {
            let threads = self.threads.lock().unwrap();
            Ok(threads.get(thread_id).cloned())
        })
    }

    fn create_thread<'a>(
        &'a self,
        thread_id: &'a str,
        resource_id: Option<String>,
    ) -> BoxFuture<'a, Result<ThreadSession, BoxError>> {
        Box::pin(async move {
            let mut threads = self.threads.lock().unwrap();
            let session = ThreadSession {
                thread_id: thread_id.to_string(),
                resource_id,
                messages: Vec::new(),
                state: HashMap::new(),
            };
            threads.insert(thread_id.to_string(), session.clone());
            Ok(session)
        })
    }

    fn update_thread_state<'a>(
        &'a self,
        thread_id: &'a str,
        key: String,
        value: serde_json::Value,
    ) -> BoxFuture<'a, Result<(), BoxError>> {
        Box::pin(async move {
            let mut threads = self.threads.lock().unwrap();
            if let Some(session) = threads.get_mut(thread_id) {
                session.state.insert(key, value);
            } else {
                let mut state = HashMap::new();
                state.insert(key, value);
                threads.insert(
                    thread_id.to_string(),
                    ThreadSession {
                        thread_id: thread_id.to_string(),
                        resource_id: None,
                        messages: Vec::new(),
                        state,
                    },
                );
            }
            Ok(())
        })
    }
}

pub struct FileStorage {
    dir_path: PathBuf,
}

impl FileStorage {
    pub fn new<P: AsRef<Path>>(dir_path: P) -> Self {
        Self {
            dir_path: dir_path.as_ref().to_path_buf(),
        }
    }
}

impl Storage for FileStorage {
    fn get_messages<'a>(
        &'a self,
        thread_id: &'a str,
    ) -> BoxFuture<'a, Result<Vec<ChatMessage>, BoxError>> {
        Box::pin(async move {
            let path = self.dir_path.join(format!("{}.json", thread_id));
            if !path.exists() {
                return Ok(Vec::new());
            }
            let content = fs::read_to_string(&path)?;
            let session: ThreadSession = serde_json::from_str(&content)?;
            Ok(session.messages)
        })
    }

    fn save_messages<'a>(
        &'a self,
        thread_id: &'a str,
        messages: Vec<ChatMessage>,
    ) -> BoxFuture<'a, Result<(), BoxError>> {
        Box::pin(async move {
            let path = self.dir_path.join(format!("{}.json", thread_id));
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)?;
            }

            let mut session = match self.get_thread(thread_id).await? {
                Some(s) => s,
                None => ThreadSession {
                    thread_id: thread_id.to_string(),
                    resource_id: None,
                    messages: Vec::new(),
                    state: HashMap::new(),
                },
            };
            session.messages = messages;

            let content = serde_json::to_string_pretty(&session)?;
            fs::write(&path, content)?;
            Ok(())
        })
    }

    fn get_thread<'a>(
        &'a self,
        thread_id: &'a str,
    ) -> BoxFuture<'a, Result<Option<ThreadSession>, BoxError>> {
        Box::pin(async move {
            let path = self.dir_path.join(format!("{}.json", thread_id));
            if !path.exists() {
                return Ok(None);
            }
            let content = fs::read_to_string(&path)?;
            let session: ThreadSession = serde_json::from_str(&content)?;
            Ok(Some(session))
        })
    }

    fn create_thread<'a>(
        &'a self,
        thread_id: &'a str,
        resource_id: Option<String>,
    ) -> BoxFuture<'a, Result<ThreadSession, BoxError>> {
        Box::pin(async move {
            let path = self.dir_path.join(format!("{}.json", thread_id));
            let session = ThreadSession {
                thread_id: thread_id.to_string(),
                resource_id,
                messages: Vec::new(),
                state: HashMap::new(),
            };
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)?;
            }
            let content = serde_json::to_string_pretty(&session)?;
            fs::write(&path, content)?;
            Ok(session)
        })
    }

    fn update_thread_state<'a>(
        &'a self,
        thread_id: &'a str,
        key: String,
        value: serde_json::Value,
    ) -> BoxFuture<'a, Result<(), BoxError>> {
        Box::pin(async move {
            let path = self.dir_path.join(format!("{}.json", thread_id));
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)?;
            }
            let mut session = match self.get_thread(thread_id).await? {
                Some(s) => s,
                None => ThreadSession {
                    thread_id: thread_id.to_string(),
                    resource_id: None,
                    messages: Vec::new(),
                    state: HashMap::new(),
                },
            };
            session.state.insert(key, value);
            let content = serde_json::to_string_pretty(&session)?;
            fs::write(&path, content)?;
            Ok(())
        })
    }
}
