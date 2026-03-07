//! Bridge between the graph runtime and the codegen compiler/interpreter.
//!
//! Provides [`CompiledNodeExecutor`] which compiles a Lattice AST expression
//! and executes it via the stack-based interpreter whenever a graph node runs.

use crate::compiler::Compiler;
use crate::error::CodegenError;
use crate::interpreter::Interpreter;
use crate::ir::Program;
use async_trait::async_trait;
use lattice_parser::ast;
use lattice_runtime::error::RuntimeError;
use lattice_runtime::node::{NodeExecutor, Value};

/// A graph node executor backed by a compiled Lattice expression.
///
/// The expression is compiled once at construction time. On each execution,
/// the node's input value is bound to the variable `input` and the compiled
/// program is evaluated, returning its result.
pub struct CompiledNodeExecutor {
    node_name: String,
    program: Program,
}

impl CompiledNodeExecutor {
    /// Compile an AST expression into a node executor.
    pub fn from_expr(
        node_name: impl Into<String>,
        expr: &ast::Expr,
    ) -> Result<Self, CodegenError> {
        let mut compiler = Compiler::new();
        let program = compiler.compile_expression(expr)?;
        Ok(Self {
            node_name: node_name.into(),
            program,
        })
    }

    /// Create from an already-compiled program.
    pub fn from_program(node_name: impl Into<String>, program: Program) -> Self {
        Self {
            node_name: node_name.into(),
            program,
        }
    }
}

#[async_trait]
impl NodeExecutor for CompiledNodeExecutor {
    async fn execute(&self, input: Value) -> Result<Value, RuntimeError> {
        let mut interp = Interpreter::new();
        interp.register_stdlib();
        interp.set_variable("input", input);

        interp
            .execute(&self.program)
            .map_err(|e| RuntimeError::NodeFailed {
                name: self.node_name.clone(),
                cause: e.to_string(),
            })
    }

    fn name(&self) -> &str {
        &self.node_name
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lattice_parser::ast::{BinOp, Expr, Span, Spanned};

    fn spanned(expr: Expr) -> Spanned<Expr> {
        Spanned::new(expr, Span::dummy())
    }

    #[tokio::test]
    async fn compiled_executor_identity() {
        let expr = Expr::Ident("input".into());
        let executor = CompiledNodeExecutor::from_expr("test", &expr).unwrap();
        let result = executor.execute(Value::Int(42)).await.unwrap();
        assert_eq!(result, Value::Int(42));
    }

    #[tokio::test]
    async fn compiled_executor_add_ten() {
        let expr = Expr::BinOp {
            op: BinOp::Add,
            left: Box::new(spanned(Expr::Ident("input".into()))),
            right: Box::new(spanned(Expr::IntLit(10))),
        };
        let executor = CompiledNodeExecutor::from_expr("adder", &expr).unwrap();
        let result = executor.execute(Value::Int(5)).await.unwrap();
        assert_eq!(result, Value::Int(15));
    }

    #[tokio::test]
    async fn compiled_executor_string_concat() {
        let expr = Expr::BinOp {
            op: BinOp::Concat,
            left: Box::new(spanned(Expr::Ident("input".into()))),
            right: Box::new(spanned(Expr::StringLit("world".into()))),
        };
        let executor = CompiledNodeExecutor::from_expr("greeter", &expr).unwrap();
        let result = executor
            .execute(Value::String("hello ".into()))
            .await
            .unwrap();
        assert_eq!(result, Value::String("hello world".into()));
    }

    #[tokio::test]
    async fn compiled_executor_multiply() {
        let expr = Expr::BinOp {
            op: BinOp::Mul,
            left: Box::new(spanned(Expr::Ident("input".into()))),
            right: Box::new(spanned(Expr::IntLit(3))),
        };
        let executor = CompiledNodeExecutor::from_expr("mul3", &expr).unwrap();
        let result = executor.execute(Value::Int(7)).await.unwrap();
        assert_eq!(result, Value::Int(21));
    }

    #[tokio::test]
    async fn compiled_executor_conditional() {
        // if input > 0 then input else 0 - input
        let expr = Expr::If {
            cond: Box::new(spanned(Expr::BinOp {
                op: BinOp::Gt,
                left: Box::new(spanned(Expr::Ident("input".into()))),
                right: Box::new(spanned(Expr::IntLit(0))),
            })),
            then_: Box::new(spanned(Expr::Ident("input".into()))),
            else_: Some(Box::new(spanned(Expr::BinOp {
                op: BinOp::Sub,
                left: Box::new(spanned(Expr::IntLit(0))),
                right: Box::new(spanned(Expr::Ident("input".into()))),
            }))),
        };
        let executor = CompiledNodeExecutor::from_expr("abs", &expr).unwrap();

        let pos = executor.execute(Value::Int(5)).await.unwrap();
        assert_eq!(pos, Value::Int(5));

        let neg = executor.execute(Value::Int(-3)).await.unwrap();
        assert_eq!(neg, Value::Int(3));
    }

    #[tokio::test]
    async fn compiled_executor_null_input() {
        let expr = Expr::Ident("input".into());
        let executor = CompiledNodeExecutor::from_expr("null_test", &expr).unwrap();
        let result = executor.execute(Value::Null).await.unwrap();
        assert_eq!(result, Value::Null);
    }

    #[tokio::test]
    async fn graph_with_compiled_nodes() {
        use lattice_runtime::graph::{
            EdgeConfig, ExecutableGraph, ExecutableNode, NodeProperties, NodeType,
        };
        use lattice_runtime::scheduler::Scheduler;
        use std::collections::HashMap;
        use std::sync::Arc;

        // Build graph: source → double → sink
        // double computes: input * 2
        let mut g = ExecutableGraph::new("compiled_pipeline");
        let source = g.add_node(ExecutableNode {
            name: "source".into(),
            node_type: NodeType::Source { data: vec![] },
            properties: NodeProperties::default(),
        });
        let double = g.add_node(ExecutableNode {
            name: "double".into(),
            node_type: NodeType::PassThrough,
            properties: NodeProperties::default(),
        });
        let sink = g.add_node(ExecutableNode {
            name: "sink".into(),
            node_type: NodeType::Sink {
                results: vec![],
            },
            properties: NodeProperties::default(),
        });
        g.add_edge(source, double, EdgeConfig::default());
        g.add_edge(double, sink, EdgeConfig::default());

        // Compile: input * 2
        let expr = Expr::BinOp {
            op: BinOp::Mul,
            left: Box::new(spanned(Expr::Ident("input".into()))),
            right: Box::new(spanned(Expr::IntLit(2))),
        };
        let executor = CompiledNodeExecutor::from_expr("double", &expr).unwrap();

        let mut scheduler = Scheduler::new();
        scheduler.register_executor("double", Arc::new(executor));

        let mut inputs = HashMap::new();
        inputs.insert("source".into(), Value::Int(21));

        let result = scheduler.execute(&g, inputs).await.unwrap();
        assert_eq!(result.outputs.get("sink"), Some(&Value::Int(42)));
    }

    #[tokio::test]
    async fn graph_pipeline_two_compiled_nodes() {
        use lattice_runtime::graph::{
            EdgeConfig, ExecutableGraph, ExecutableNode, NodeProperties, NodeType,
        };
        use lattice_runtime::scheduler::Scheduler;
        use std::collections::HashMap;
        use std::sync::Arc;

        // source → add_ten → mul_three → sink
        // Result: (5 + 10) * 3 = 45
        let mut g = ExecutableGraph::new("two_step");
        let source = g.add_node(ExecutableNode {
            name: "source".into(),
            node_type: NodeType::Source { data: vec![] },
            properties: NodeProperties::default(),
        });
        let add = g.add_node(ExecutableNode {
            name: "add_ten".into(),
            node_type: NodeType::PassThrough,
            properties: NodeProperties::default(),
        });
        let mul = g.add_node(ExecutableNode {
            name: "mul_three".into(),
            node_type: NodeType::PassThrough,
            properties: NodeProperties::default(),
        });
        let sink = g.add_node(ExecutableNode {
            name: "sink".into(),
            node_type: NodeType::Sink {
                results: vec![],
            },
            properties: NodeProperties::default(),
        });
        g.add_edge(source, add, EdgeConfig::default());
        g.add_edge(add, mul, EdgeConfig::default());
        g.add_edge(mul, sink, EdgeConfig::default());

        let add_expr = Expr::BinOp {
            op: BinOp::Add,
            left: Box::new(spanned(Expr::Ident("input".into()))),
            right: Box::new(spanned(Expr::IntLit(10))),
        };
        let mul_expr = Expr::BinOp {
            op: BinOp::Mul,
            left: Box::new(spanned(Expr::Ident("input".into()))),
            right: Box::new(spanned(Expr::IntLit(3))),
        };

        let mut scheduler = Scheduler::new();
        scheduler.register_executor(
            "add_ten",
            Arc::new(CompiledNodeExecutor::from_expr("add_ten", &add_expr).unwrap()),
        );
        scheduler.register_executor(
            "mul_three",
            Arc::new(CompiledNodeExecutor::from_expr("mul_three", &mul_expr).unwrap()),
        );

        let mut inputs = HashMap::new();
        inputs.insert("source".into(), Value::Int(5));

        let result = scheduler.execute(&g, inputs).await.unwrap();
        assert_eq!(result.outputs.get("sink"), Some(&Value::Int(45)));
    }
}
