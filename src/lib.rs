pub mod agent;
pub mod memory;
pub mod openai;
pub mod storage;
pub mod tool;
pub mod tools;

pub use agent::{
    Agent, AgentBuilder, AgentConfig, AgentStream, AgentStreamEvent, GenerateOptions,
    TaskSignalProvider,
};
pub use memory::{Memory, MemoryConfig};
pub use storage::{FileStorage, InMemoryStorage, Storage, ThreadSession};
pub use tool::{BoxError, BoxFuture, Tool, TypedTool, create_tool};
pub use tools::task::{
    CURRENT_CONTEXT, ExecutionContext, Task, TaskCheckOutput, TaskCompleteOutput, TaskInput,
    TaskUpdateOutput, TaskWriteOutput,
};

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
