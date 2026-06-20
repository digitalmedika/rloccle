use std::future::Future;
use std::pin::Pin;
use std::marker::PhantomData;
use std::sync::Arc;

pub type BoxError = Box<dyn std::error::Error + Send + Sync>;
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

pub trait Tool: Send + Sync {
    fn id(&self) -> &str;
    fn description(&self) -> &str;
    fn input_schema(&self) -> serde_json::Value;
    fn output_schema(&self) -> Option<serde_json::Value>;
    fn execute(&self, args: serde_json::Value) -> BoxFuture<'static, Result<serde_json::Value, BoxError>>;
}

pub struct TypedTool<I, O, F> {
    id: String,
    description: String,
    execute_fn: Arc<F>,
    _marker: PhantomData<fn(I) -> O>,
}

impl<I, O, F, Fut> Tool for TypedTool<I, O, F>
where
    I: schemars::JsonSchema + serde::de::DeserializeOwned + Send + 'static,
    O: schemars::JsonSchema + serde::Serialize + Send + 'static,
    F: Fn(I) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<O, BoxError>> + Send + 'static,
{
    fn id(&self) -> &str {
        &self.id
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::to_value(schemars::schema_for!(I)).unwrap_or_default()
    }

    fn output_schema(&self) -> Option<serde_json::Value> {
        Some(serde_json::to_value(schemars::schema_for!(O)).unwrap_or_default())
    }

    fn execute(&self, args: serde_json::Value) -> BoxFuture<'static, Result<serde_json::Value, BoxError>> {
        let exec = self.execute_fn.clone();
        Box::pin(async move {
            let input: I = serde_json::from_value(args)?;
            let output: O = exec(input).await?;
            let output_val = serde_json::to_value(output)?;
            Ok(output_val)
        })
    }
}

pub fn create_tool<I, O, F, Fut>(
    id: impl Into<String>,
    description: impl Into<String>,
    execute_fn: F,
) -> TypedTool<I, O, F>
where
    I: schemars::JsonSchema + serde::de::DeserializeOwned + Send + 'static,
    O: schemars::JsonSchema + serde::Serialize + Send + 'static,
    F: Fn(I) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<O, BoxError>> + Send + 'static,
{
    TypedTool {
        id: id.into(),
        description: description.into(),
        execute_fn: Arc::new(execute_fn),
        _marker: PhantomData,
    }
}
