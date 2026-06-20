use crate::storage::{InMemoryStorage, Storage};
use std::sync::Arc;

#[derive(Debug, Clone, Default)]
pub struct MemoryConfig {
    pub last_messages: Option<usize>,
}

#[derive(Clone)]
pub struct Memory {
    storage: Arc<dyn Storage>,
    config: MemoryConfig,
}

impl Memory {
    pub fn new(storage: Arc<dyn Storage>, config: MemoryConfig) -> Self {
        Self { storage, config }
    }

    pub fn with_in_memory(config: MemoryConfig) -> Self {
        Self {
            storage: Arc::new(InMemoryStorage::new()),
            config,
        }
    }

    pub fn storage(&self) -> &Arc<dyn Storage> {
        &self.storage
    }

    pub fn config(&self) -> &MemoryConfig {
        &self.config
    }
}

impl Default for Memory {
    fn default() -> Self {
        Self {
            storage: Arc::new(InMemoryStorage::new()),
            config: MemoryConfig::default(),
        }
    }
}
