#[derive(Debug, thiserror::Error)]
pub enum CodegenError {
    #[error("Unsupported expression: {0}")]
    Unsupported(String),
    #[error("Undefined variable: {0}")]
    UndefinedVariable(String),
    #[error("Type error at runtime: {0}")]
    TypeError(String),
    #[error("Division by zero")]
    DivisionByZero,
    #[error("Stack underflow")]
    StackUnderflow,
}
