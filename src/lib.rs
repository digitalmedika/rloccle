pub mod agent;
pub mod openai;
pub mod tool;
pub mod tools;
pub mod storage;
pub mod memory;

pub use agent::{Agent, AgentBuilder, AgentConfig, AgentStream, AgentStreamEvent, GenerateOptions};
pub use tool::{Tool, TypedTool, create_tool, BoxError, BoxFuture};
pub use storage::{Storage, InMemoryStorage, FileStorage, ThreadSession};
pub use memory::{Memory, MemoryConfig};

#[macro_export]
macro_rules! agent {
    ($($key:ident : $val:expr),* $(,)?) => {
        {
            let mut builder = $crate::Agent::builder();
            $(
                builder = builder.$key($val);
            )*
            builder.build().expect("Failed to build agent")
        }
    };
}
