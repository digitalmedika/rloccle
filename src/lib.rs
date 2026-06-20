pub mod agent;
pub mod openai;
pub mod tool;

pub use agent::{Agent, AgentBuilder, AgentConfig};
pub use tool::{Tool, TypedTool, create_tool, BoxError, BoxFuture};

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
