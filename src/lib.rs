pub mod agent;
pub mod openai;

pub use agent::{Agent, AgentBuilder, AgentConfig};

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
