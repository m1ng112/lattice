//! Async graph scheduler for parallel node execution.
//!
//! Executes an [`ExecutableGraph`] by running nodes in topological order,
//! spawning parallel tasks for independent nodes within the same level.

use crate::error::RuntimeError;
use crate::graph::{ExecutableGraph, ExecutableNode, NodeType};
use crate::node::{IdentityNode, NodeExecutor, Value};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::Mutex;

/// An entry in the execution trace log.
#[derive(Debug, Clone, serde::Serialize)]
pub struct TraceEntry {
    pub node: String,
    pub phase: TracePhase,
    pub timestamp_ms: u64,
    pub duration_ms: Option<u64>,
    pub value: Option<Value>,
}

/// Phase of a trace entry.
#[derive(Debug, Clone, serde::Serialize)]
pub enum TracePhase {
    Start,
    Complete,
    Error(String),
}

/// Result of executing a graph.
#[derive(Debug)]
pub struct ExecutionResult {
    /// Output values keyed by sink node name.
    pub outputs: HashMap<String, Value>,
    /// Execution trace (if tracing was enabled).
    pub trace: Vec<TraceEntry>,
    /// Total execution duration in milliseconds.
    pub duration_ms: u64,
}

/// The graph scheduler.
///
/// Executes nodes in parallel groups determined by topological level,
/// propagating values from producers to consumers.
pub struct Scheduler {
    trace_enabled: bool,
    timeout_ms: Option<u64>,
    executors: HashMap<String, Arc<dyn NodeExecutor>>,
}

impl Scheduler {
    /// Create a new scheduler with default settings.
    pub fn new() -> Self {
        Self {
            trace_enabled: false,
            timeout_ms: None,
            executors: HashMap::new(),
        }
    }

    /// Enable execution tracing.
    pub fn with_trace(mut self) -> Self {
        self.trace_enabled = true;
        self
    }

    /// Set a global execution timeout in milliseconds.
    pub fn with_timeout(mut self, ms: u64) -> Self {
        self.timeout_ms = Some(ms);
        self
    }

    /// Register a custom executor for a named node.
    pub fn register_executor(&mut self, name: impl Into<String>, executor: Arc<dyn NodeExecutor>) {
        self.executors.insert(name.into(), executor);
    }

    /// Execute the graph with the given initial inputs.
    ///
    /// Inputs are keyed by node name. Source nodes without an explicit
    /// input receive [`Value::Null`].
    pub async fn execute(
        &self,
        graph: &ExecutableGraph,
        inputs: HashMap<String, Value>,
    ) -> Result<ExecutionResult, RuntimeError> {
        let start = Instant::now();

        // Check for cycles before executing
        graph.check_cycles()?;

        // Get parallel execution groups
        let groups = graph.parallel_groups()?;

        // Shared state for values flowing between nodes
        let values: Arc<Mutex<HashMap<String, Value>>> = Arc::new(Mutex::new(inputs));
        let trace: Arc<Mutex<Vec<TraceEntry>>> = Arc::new(Mutex::new(Vec::new()));
        let exec_start = Instant::now();

        for group in &groups {
            let mut handles = Vec::new();

            for &node_idx in group {
                let node = &graph.graph[node_idx];
                let node_name = node.name.clone();
                let values = Arc::clone(&values);
                let trace = Arc::clone(&trace);
                let trace_enabled = self.trace_enabled;
                let timeout_ms = node
                    .properties
                    .timeout_ms
                    .or(self.timeout_ms);

                // Determine the input for this node
                let input = {
                    let vals = values.lock().await;
                    vals.get(&node_name).cloned().unwrap_or(Value::Null)
                };

                // Get registered executor or create a default one for this node
                let executor: Arc<dyn NodeExecutor> = self
                    .executors
                    .get(&node_name)
                    .cloned()
                    .unwrap_or_else(|| create_executor(node));

                // Get successor node names for output propagation
                let successors: Vec<String> = graph
                    .graph
                    .neighbors(node_idx)
                    .map(|succ_idx| graph.graph[succ_idx].name.clone())
                    .collect();

                let node_name_clone = node_name.clone();
                let handle = tokio::spawn(async move {
                    let node_start = Instant::now();

                    if trace_enabled {
                        trace.lock().await.push(TraceEntry {
                            node: node_name.clone(),
                            phase: TracePhase::Start,
                            timestamp_ms: exec_start.elapsed().as_millis() as u64,
                            duration_ms: None,
                            value: None,
                        });
                    }

                    // Execute with optional timeout
                    let result = if let Some(timeout) = timeout_ms {
                        match tokio::time::timeout(
                            std::time::Duration::from_millis(timeout),
                            executor.execute(input),
                        )
                        .await
                        {
                            Ok(r) => r,
                            Err(_) => Err(RuntimeError::Timeout {
                                name: node_name.clone(),
                                timeout_ms: timeout,
                            }),
                        }
                    } else {
                        executor.execute(input).await
                    };

                    let duration = node_start.elapsed().as_millis() as u64;

                    match result {
                        Ok(output) => {
                            if trace_enabled {
                                trace.lock().await.push(TraceEntry {
                                    node: node_name.clone(),
                                    phase: TracePhase::Complete,
                                    timestamp_ms: exec_start.elapsed().as_millis() as u64,
                                    duration_ms: Some(duration),
                                    value: Some(output.clone()),
                                });
                            }

                            // Propagate output to successor nodes
                            let mut vals = values.lock().await;
                            for succ in &successors {
                                vals.insert(succ.clone(), output.clone());
                            }
                            // Also store under own name for final output collection
                            vals.insert(node_name, output);

                            Ok(())
                        }
                        Err(e) => {
                            if trace_enabled {
                                trace.lock().await.push(TraceEntry {
                                    node: node_name.clone(),
                                    phase: TracePhase::Error(e.to_string()),
                                    timestamp_ms: exec_start.elapsed().as_millis() as u64,
                                    duration_ms: Some(duration),
                                    value: None,
                                });
                            }
                            Err(e)
                        }
                    }
                });

                handles.push((node_name_clone, handle));
            }

            // Wait for all nodes in this group to complete
            for (name, handle) in handles {
                handle
                    .await
                    .map_err(|e| RuntimeError::NodeFailed {
                        name: name.clone(),
                        cause: format!("Task join error: {e}"),
                    })?
                    .map_err(|e| e)?;
            }
        }

        let duration_ms = start.elapsed().as_millis() as u64;

        // Collect outputs from sink nodes (nodes with no successors)
        let final_values = values.lock().await;
        let mut outputs = HashMap::new();
        for (name, &node_idx) in &graph.node_indices {
            let has_successors = graph
                .graph
                .neighbors(node_idx)
                .next()
                .is_some();
            if !has_successors {
                if let Some(val) = final_values.get(name) {
                    outputs.insert(name.clone(), val.clone());
                }
            }
        }

        let trace = Arc::try_unwrap(trace)
            .map(|mutex| mutex.into_inner())
            .unwrap_or_else(|arc| {
                // Fallback: clone the inner data
                let guard = arc.blocking_lock();
                guard.clone()
            });

        Ok(ExecutionResult {
            outputs,
            trace,
            duration_ms,
        })
    }
}

impl Default for Scheduler {
    fn default() -> Self {
        Self::new()
    }
}

/// Create a default executor based on node type.
fn create_executor(node: &ExecutableNode) -> Arc<dyn NodeExecutor> {
    match &node.node_type {
        NodeType::PassThrough => Arc::new(IdentityNode),
        NodeType::Source { .. } => Arc::new(IdentityNode),
        NodeType::Sink { .. } => Arc::new(IdentityNode),
        NodeType::Compute { .. } => Arc::new(IdentityNode),
        NodeType::Transform(_) => Arc::new(IdentityNode),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::{EdgeConfig, ExecutableNode, NodeProperties, NodeType};

    fn make_linear_graph() -> ExecutableGraph {
        let mut g = ExecutableGraph::new("linear");
        let source = g.add_node(ExecutableNode {
            name: "source".into(),
            node_type: NodeType::Source { data: vec![] },
            properties: NodeProperties::default(),
        });
        let pass = g.add_node(ExecutableNode {
            name: "pass".into(),
            node_type: NodeType::PassThrough,
            properties: NodeProperties::default(),
        });
        let sink = g.add_node(ExecutableNode {
            name: "sink".into(),
            node_type: NodeType::Sink { results: vec![] },
            properties: NodeProperties::default(),
        });
        g.add_edge(source, pass, EdgeConfig::default());
        g.add_edge(pass, sink, EdgeConfig::default());
        g
    }

    #[tokio::test]
    async fn execute_linear_graph() {
        let graph = make_linear_graph();
        let mut inputs = HashMap::new();
        inputs.insert("source".into(), Value::Int(42));

        let scheduler = Scheduler::new();
        let result = scheduler.execute(&graph, inputs).await.unwrap();

        // Value should propagate through: source → pass → sink
        assert_eq!(result.outputs.get("sink"), Some(&Value::Int(42)));
    }

    #[tokio::test]
    async fn execute_with_trace() {
        let graph = make_linear_graph();
        let mut inputs = HashMap::new();
        inputs.insert("source".into(), Value::Int(99));

        let scheduler = Scheduler::new().with_trace();
        let result = scheduler.execute(&graph, inputs).await.unwrap();

        // Should have Start + Complete for each of 3 nodes = 6 trace entries
        assert_eq!(result.trace.len(), 6);

        // Verify we got Start and Complete for each node
        let starts: Vec<_> = result
            .trace
            .iter()
            .filter(|t| matches!(t.phase, TracePhase::Start))
            .collect();
        let completes: Vec<_> = result
            .trace
            .iter()
            .filter(|t| matches!(t.phase, TracePhase::Complete))
            .collect();
        assert_eq!(starts.len(), 3);
        assert_eq!(completes.len(), 3);
    }

    #[tokio::test]
    async fn execute_with_timeout() {
        struct SlowNode;

        #[async_trait::async_trait]
        impl crate::node::NodeExecutor for SlowNode {
            async fn execute(&self, _input: Value) -> Result<Value, RuntimeError> {
                tokio::time::sleep(std::time::Duration::from_secs(10)).await;
                Ok(Value::Null)
            }
            fn name(&self) -> &str {
                "slow"
            }
        }

        let mut g = ExecutableGraph::new("timeout_test");
        g.add_node(ExecutableNode {
            name: "slow".into(),
            node_type: NodeType::PassThrough,
            properties: NodeProperties {
                timeout_ms: Some(50),
                ..NodeProperties::default()
            },
        });

        let mut scheduler = Scheduler::new();
        scheduler.register_executor("slow", Arc::new(SlowNode));
        let result = scheduler.execute(&g, HashMap::new()).await;
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            RuntimeError::Timeout { .. }
        ));
    }

    #[tokio::test]
    async fn execute_diamond_graph() {
        // A → B, A → C, B → D, C → D
        let mut g = ExecutableGraph::new("diamond");
        let a = g.add_node(ExecutableNode {
            name: "A".into(),
            node_type: NodeType::Source { data: vec![] },
            properties: NodeProperties::default(),
        });
        let b = g.add_node(ExecutableNode {
            name: "B".into(),
            node_type: NodeType::PassThrough,
            properties: NodeProperties::default(),
        });
        let c = g.add_node(ExecutableNode {
            name: "C".into(),
            node_type: NodeType::PassThrough,
            properties: NodeProperties::default(),
        });
        let d = g.add_node(ExecutableNode {
            name: "D".into(),
            node_type: NodeType::Sink { results: vec![] },
            properties: NodeProperties::default(),
        });
        g.add_edge(a, b, EdgeConfig::default());
        g.add_edge(a, c, EdgeConfig::default());
        g.add_edge(b, d, EdgeConfig::default());
        g.add_edge(c, d, EdgeConfig::default());

        let mut inputs = HashMap::new();
        inputs.insert("A".into(), Value::Int(7));

        let scheduler = Scheduler::new();
        let result = scheduler.execute(&g, inputs).await.unwrap();
        // D should receive value propagated through B or C
        assert_eq!(result.outputs.get("D"), Some(&Value::Int(7)));
    }

    #[tokio::test]
    async fn execute_empty_graph() {
        let g = ExecutableGraph::new("empty");
        let scheduler = Scheduler::new();
        let result = scheduler.execute(&g, HashMap::new()).await.unwrap();
        assert!(result.outputs.is_empty());
    }

    #[tokio::test]
    async fn rejects_cyclic_graph() {
        let mut g = ExecutableGraph::new("cyclic");
        let a = g.add_node(ExecutableNode {
            name: "a".into(),
            node_type: NodeType::PassThrough,
            properties: NodeProperties::default(),
        });
        let b = g.add_node(ExecutableNode {
            name: "b".into(),
            node_type: NodeType::PassThrough,
            properties: NodeProperties::default(),
        });
        g.add_edge(a, b, EdgeConfig::default());
        g.add_edge(b, a, EdgeConfig::default());

        let scheduler = Scheduler::new();
        let result = scheduler.execute(&g, HashMap::new()).await;
        assert!(matches!(
            result.unwrap_err(),
            RuntimeError::Deadlock { .. }
        ));
    }
}
