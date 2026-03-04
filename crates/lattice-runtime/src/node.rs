//! Node execution trait and built-in node implementations.

use crate::error::RuntimeError;
use async_trait::async_trait;
use std::collections::HashMap;

/// Value passed between nodes in the dataflow graph.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(untagged)]
pub enum Value {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    String(String),
    Array(Vec<Value>),
    Object(HashMap<String, Value>),
}

impl Value {
    /// Returns `true` if this value is [`Value::Null`].
    pub fn is_null(&self) -> bool {
        matches!(self, Value::Null)
    }

    /// Attempts to extract an `i64` from this value.
    pub fn as_int(&self) -> Option<i64> {
        match self {
            Value::Int(n) => Some(*n),
            _ => None,
        }
    }

    /// Attempts to extract a `f64` from this value.
    pub fn as_float(&self) -> Option<f64> {
        match self {
            Value::Float(f) => Some(*f),
            _ => None,
        }
    }

    /// Attempts to extract a `bool` from this value.
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Value::Bool(b) => Some(*b),
            _ => None,
        }
    }

    /// Attempts to extract a `&str` from this value.
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::String(s) => Some(s),
            _ => None,
        }
    }
}

/// Trait for executable node implementations.
///
/// All node executors must be `Send + Sync` to support parallel execution.
#[async_trait]
pub trait NodeExecutor: Send + Sync {
    /// Execute the node with the given input, producing an output value.
    async fn execute(&self, input: Value) -> Result<Value, RuntimeError>;

    /// The name of this node executor (for tracing/debugging).
    fn name(&self) -> &str;
}

/// Built-in identity node that passes input through unchanged.
pub struct IdentityNode;

#[async_trait]
impl NodeExecutor for IdentityNode {
    async fn execute(&self, input: Value) -> Result<Value, RuntimeError> {
        Ok(input)
    }

    fn name(&self) -> &str {
        "identity"
    }
}

/// Built-in node that evaluates simple expressions on input values.
pub struct ExpressionNode {
    pub node_name: String,
    pub expression: String,
}

#[async_trait]
impl NodeExecutor for ExpressionNode {
    async fn execute(&self, input: Value) -> Result<Value, RuntimeError> {
        // Basic expression evaluation: supports simple transforms
        match self.expression.as_str() {
            "identity" | "" => Ok(input),
            expr if expr.starts_with("add:") => {
                let n: i64 = expr[4..].parse().map_err(|_| RuntimeError::NodeFailed {
                    name: self.node_name.clone(),
                    cause: format!("Invalid add expression: {expr}"),
                })?;
                match input {
                    Value::Int(v) => Ok(Value::Int(v + n)),
                    _ => Err(RuntimeError::NodeFailed {
                        name: self.node_name.clone(),
                        cause: "add requires Int input".to_string(),
                    }),
                }
            }
            expr if expr.starts_with("mul:") => {
                let n: i64 = expr[4..].parse().map_err(|_| RuntimeError::NodeFailed {
                    name: self.node_name.clone(),
                    cause: format!("Invalid mul expression: {expr}"),
                })?;
                match input {
                    Value::Int(v) => Ok(Value::Int(v * n)),
                    _ => Err(RuntimeError::NodeFailed {
                        name: self.node_name.clone(),
                        cause: "mul requires Int input".to_string(),
                    }),
                }
            }
            _ => Err(RuntimeError::NodeFailed {
                name: self.node_name.clone(),
                cause: format!("Unknown expression: {}", self.expression),
            }),
        }
    }

    fn name(&self) -> &str {
        &self.node_name
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn identity_passes_through() {
        let node = IdentityNode;
        let input = Value::Int(42);
        let output = node.execute(input.clone()).await.unwrap();
        assert_eq!(output, input);
    }

    #[tokio::test]
    async fn expression_add() {
        let node = ExpressionNode {
            node_name: "adder".into(),
            expression: "add:10".into(),
        };
        let output = node.execute(Value::Int(5)).await.unwrap();
        assert_eq!(output, Value::Int(15));
    }

    #[tokio::test]
    async fn expression_mul() {
        let node = ExpressionNode {
            node_name: "multiplier".into(),
            expression: "mul:3".into(),
        };
        let output = node.execute(Value::Int(7)).await.unwrap();
        assert_eq!(output, Value::Int(21));
    }

    #[tokio::test]
    async fn expression_unknown_errors() {
        let node = ExpressionNode {
            node_name: "bad".into(),
            expression: "unknown_op".into(),
        };
        let result = node.execute(Value::Int(1)).await;
        assert!(result.is_err());
    }

    #[test]
    fn value_accessors() {
        assert_eq!(Value::Int(42).as_int(), Some(42));
        assert_eq!(Value::Float(3.14).as_float(), Some(3.14));
        assert_eq!(Value::Bool(true).as_bool(), Some(true));
        assert_eq!(Value::String("hi".into()).as_str(), Some("hi"));
        assert!(Value::Null.is_null());
        assert_eq!(Value::Null.as_int(), None);
    }

    #[test]
    fn value_serde_roundtrip() {
        let values = vec![
            Value::Null,
            Value::Bool(true),
            Value::Int(42),
            Value::Float(3.14),
            Value::String("hello".into()),
            Value::Array(vec![Value::Int(1), Value::Int(2)]),
        ];
        for val in values {
            let json = serde_json::to_string(&val).unwrap();
            let back: Value = serde_json::from_str(&json).unwrap();
            assert_eq!(val, back);
        }
    }
}
