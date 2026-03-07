//! Executable graph construction from AST.
//!
//! Converts a parsed [`ast::Graph`] into a [`petgraph`]-based
//! [`ExecutableGraph`] with cycle detection and topological ordering.

use crate::error::RuntimeError;
use lattice_parser::ast;
use petgraph::algo::{is_cyclic_directed, toposort};
use petgraph::graph::{DiGraph, NodeIndex};
use std::collections::HashMap;

/// An executable graph built from the AST, ready for scheduling.
#[derive(Debug)]
pub struct ExecutableGraph {
    pub name: String,
    pub graph: DiGraph<ExecutableNode, EdgeConfig>,
    pub node_indices: HashMap<String, NodeIndex>,
}

/// A node in the executable graph.
#[derive(Debug)]
pub struct ExecutableNode {
    pub name: String,
    pub node_type: NodeType,
    pub properties: NodeProperties,
}

/// The kind of computation a node performs.
#[derive(Debug)]
pub enum NodeType {
    /// User-defined compute node.
    Compute {
        implementation: Option<NodeImpl>,
    },
    /// Source node (produces data).
    Source {
        data: Vec<serde_json::Value>,
    },
    /// Sink node (collects results).
    Sink {
        results: Vec<serde_json::Value>,
    },
    /// Transform node (map/filter/fold).
    Transform(TransformKind),
    /// Pass-through (identity).
    PassThrough,
}

/// Kinds of built-in transform operations.
#[derive(Debug)]
pub enum TransformKind {
    /// Apply an expression to each element.
    Map(String),
    /// Keep elements matching a predicate.
    Filter(String),
    /// Reduce elements with an accumulator.
    Fold {
        init: serde_json::Value,
        func: String,
    },
}

/// How a compute node is implemented.
#[derive(Debug)]
pub enum NodeImpl {
    /// A built-in (named) implementation.
    Builtin(String),
    /// An expression from the AST.
    Expression(ast::Spanned<ast::Expr>),
}

/// Execution properties for a node.
#[derive(Debug)]
pub struct NodeProperties {
    /// Maximum execution time in milliseconds before timeout.
    pub timeout_ms: Option<u64>,
    /// Number of retry attempts on failure.
    pub retry_count: u32,
    /// Whether repeated execution with the same input yields the same output.
    pub idempotent: bool,
    /// Whether the node's output depends only on its input (no side effects).
    pub deterministic: bool,
}

impl Default for NodeProperties {
    fn default() -> Self {
        Self {
            timeout_ms: None,
            retry_count: 0,
            idempotent: false,
            deterministic: true,
        }
    }
}

/// Configuration for an edge (data channel) between nodes.
#[derive(Debug)]
pub struct EdgeConfig {
    /// Channel buffer size for backpressure control.
    pub buffer_size: usize,
    /// Whether backpressure is enabled on this edge.
    pub backpressure: bool,
}

impl Default for EdgeConfig {
    fn default() -> Self {
        Self {
            buffer_size: 32,
            backpressure: true,
        }
    }
}

impl ExecutableGraph {
    /// Build an executable graph from an AST [`Graph`](ast::Graph).
    pub fn from_ast(ast_graph: &ast::Graph) -> Result<Self, RuntimeError> {
        let mut graph = DiGraph::new();
        let mut node_indices = HashMap::new();

        // First pass: create all nodes
        for member in &ast_graph.members {
            if let ast::GraphMember::Node(node_def) = &member.node {
                let properties = extract_properties(node_def);
                let node_type = infer_node_type(node_def);
                let exec_node = ExecutableNode {
                    name: node_def.name.clone(),
                    node_type,
                    properties,
                };
                let idx = graph.add_node(exec_node);
                node_indices.insert(node_def.name.clone(), idx);
            }
        }

        // Second pass: create all edges
        for member in &ast_graph.members {
            if let ast::GraphMember::Edge(edge_def) = &member.node {
                let from_idx = node_indices.get(&edge_def.from).ok_or_else(|| {
                    RuntimeError::NodeFailed {
                        name: edge_def.from.clone(),
                        cause: format!("Source node '{}' not found in graph", edge_def.from),
                    }
                })?;
                let to_idx = node_indices.get(&edge_def.to).ok_or_else(|| {
                    RuntimeError::NodeFailed {
                        name: edge_def.to.clone(),
                        cause: format!("Target node '{}' not found in graph", edge_def.to),
                    }
                })?;
                let edge_config = extract_edge_config(edge_def);
                graph.add_edge(*from_idx, *to_idx, edge_config);
            }
        }

        Ok(Self {
            name: ast_graph.name.clone(),
            graph,
            node_indices,
        })
    }

    /// Build an executable graph programmatically (for testing and direct construction).
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            graph: DiGraph::new(),
            node_indices: HashMap::new(),
        }
    }

    /// Add a node to the graph, returning its index.
    pub fn add_node(&mut self, node: ExecutableNode) -> NodeIndex {
        let name = node.name.clone();
        let idx = self.graph.add_node(node);
        self.node_indices.insert(name, idx);
        idx
    }

    /// Add an edge between two nodes.
    pub fn add_edge(&mut self, from: NodeIndex, to: NodeIndex, config: EdgeConfig) {
        self.graph.add_edge(from, to, config);
    }

    /// Check for cycles in the graph (deadlock detection).
    ///
    /// Returns an error with the names of nodes involved in a cycle.
    pub fn check_cycles(&self) -> Result<(), RuntimeError> {
        if is_cyclic_directed(&self.graph) {
            // Collect all node names involved in strongly-connected components
            let sccs = petgraph::algo::kosaraju_scc(&self.graph);
            let cycle_nodes: Vec<String> = sccs
                .into_iter()
                .filter(|scc| scc.len() > 1)
                .flat_map(|scc| {
                    scc.into_iter()
                        .map(|idx| self.graph[idx].name.clone())
                })
                .collect();
            Err(RuntimeError::Deadlock { nodes: cycle_nodes })
        } else {
            Ok(())
        }
    }

    /// Get a topological execution order for the graph.
    ///
    /// Nodes are ordered such that all dependencies of a node come before it.
    pub fn execution_order(&self) -> Result<Vec<NodeIndex>, RuntimeError> {
        toposort(&self.graph, None).map_err(|cycle| {
            let name = self.graph[cycle.node_id()].name.clone();
            RuntimeError::Deadlock {
                nodes: vec![name],
            }
        })
    }

    /// Group nodes into parallel execution levels.
    ///
    /// Nodes within the same group have no dependencies on each other
    /// and can execute concurrently.
    pub fn parallel_groups(&self) -> Result<Vec<Vec<NodeIndex>>, RuntimeError> {
        let order = self.execution_order()?;
        if order.is_empty() {
            return Ok(vec![]);
        }

        // Assign each node a "level" = 1 + max level of predecessors
        let mut levels: HashMap<NodeIndex, usize> = HashMap::new();
        let mut max_level = 0;

        for &idx in &order {
            let level = self
                .graph
                .neighbors_directed(idx, petgraph::Direction::Incoming)
                .filter_map(|pred| levels.get(&pred))
                .max()
                .map(|l| l + 1)
                .unwrap_or(0);
            levels.insert(idx, level);
            max_level = max_level.max(level);
        }

        // Group nodes by level
        let mut groups: Vec<Vec<NodeIndex>> = vec![vec![]; max_level + 1];
        for (&idx, &level) in &levels {
            groups[level].push(idx);
        }

        Ok(groups)
    }
}

/// Extract node properties from AST node fields.
fn extract_properties(node_def: &ast::NodeDef) -> NodeProperties {
    let mut props = NodeProperties::default();
    for field in &node_def.fields {
        if let ast::NodeField::Properties(properties) = field {
            for prop in properties {
                match prop.key.as_str() {
                    "idempotent" => {
                        if let ast::Expr::BoolLit(b) = &prop.value.node {
                            props.idempotent = *b;
                        }
                    }
                    "deterministic" => {
                        if let ast::Expr::BoolLit(b) = &prop.value.node {
                            props.deterministic = *b;
                        }
                    }
                    "timeout" => {
                        if let ast::Expr::IntLit(ms) = &prop.value.node {
                            props.timeout_ms = Some(*ms as u64);
                        }
                    }
                    "retry" => {
                        if let ast::Expr::IntLit(n) = &prop.value.node {
                            props.retry_count = *n as u32;
                        }
                    }
                    _ => {}
                }
            }
        }
    }
    props
}

/// Infer the node type from its AST definition.
///
/// Heuristic: nodes with only outputs are sources, nodes with only inputs
/// are sinks, otherwise they are compute nodes. If a semantic block with
/// a formal expression is present, it becomes the node's implementation.
fn infer_node_type(node_def: &ast::NodeDef) -> NodeType {
    let has_input = node_def
        .fields
        .iter()
        .any(|f| matches!(f, ast::NodeField::Input(_)));
    let has_output = node_def
        .fields
        .iter()
        .any(|f| matches!(f, ast::NodeField::Output(_)));

    // Extract formal expression from semantic block, if any
    let formal_expr = node_def.fields.iter().find_map(|f| {
        if let ast::NodeField::Semantic(sem) = f {
            sem.formal.clone()
        } else {
            None
        }
    });

    let implementation = formal_expr.map(NodeImpl::Expression);

    match (has_input, has_output) {
        (false, true) => NodeType::Source { data: vec![] },
        (true, false) => NodeType::Sink {
            results: vec![],
        },
        _ => NodeType::Compute { implementation },
    }
}

/// Extract edge configuration from AST edge properties.
fn extract_edge_config(edge_def: &ast::EdgeDef) -> EdgeConfig {
    let mut config = EdgeConfig::default();
    for prop in &edge_def.properties {
        match prop.key.as_str() {
            "buffer_size" => {
                if let ast::Expr::IntLit(n) = &prop.value.node {
                    config.buffer_size = *n as usize;
                }
            }
            "backpressure" => {
                if let ast::Expr::BoolLit(b) = &prop.value.node {
                    config.backpressure = *b;
                }
            }
            _ => {}
        }
    }
    config
}

#[cfg(test)]
mod tests {
    use super::*;
    use lattice_parser::ast::{
        EdgeDef, Graph, GraphMember, NodeDef, NodeField, Span, Spanned, TypeExpr,
    };

    fn spanned<T>(node: T) -> Spanned<T> {
        Spanned::new(node, Span::dummy())
    }

    fn make_simple_graph() -> Graph {
        Graph {
            name: "test_graph".into(),
            version: Some("0.1.0".into()),
            targets: vec![],
            members: vec![
                spanned(GraphMember::Node(NodeDef {
                    name: "source".into(),
                    fields: vec![NodeField::Output(spanned(TypeExpr::Named("Int".into())))],
                })),
                spanned(GraphMember::Node(NodeDef {
                    name: "transform".into(),
                    fields: vec![
                        NodeField::Input(spanned(TypeExpr::Named("Int".into()))),
                        NodeField::Output(spanned(TypeExpr::Named("Int".into()))),
                    ],
                })),
                spanned(GraphMember::Node(NodeDef {
                    name: "sink".into(),
                    fields: vec![NodeField::Input(spanned(TypeExpr::Named("Int".into())))],
                })),
                spanned(GraphMember::Edge(EdgeDef {
                    from: "source".into(),
                    to: "transform".into(),
                    properties: vec![],
                })),
                spanned(GraphMember::Edge(EdgeDef {
                    from: "transform".into(),
                    to: "sink".into(),
                    properties: vec![],
                })),
            ],
        }
    }

    #[test]
    fn build_from_ast() {
        let ast_graph = make_simple_graph();
        let exec = ExecutableGraph::from_ast(&ast_graph).unwrap();
        assert_eq!(exec.name, "test_graph");
        assert_eq!(exec.node_indices.len(), 3);
        assert_eq!(exec.graph.node_count(), 3);
        assert_eq!(exec.graph.edge_count(), 2);
    }

    #[test]
    fn no_cycles_in_dag() {
        let ast_graph = make_simple_graph();
        let exec = ExecutableGraph::from_ast(&ast_graph).unwrap();
        assert!(exec.check_cycles().is_ok());
    }

    #[test]
    fn detects_cycle() {
        let cyclic = Graph {
            name: "cyclic".into(),
            version: None,
            targets: vec![],
            members: vec![
                spanned(GraphMember::Node(NodeDef {
                    name: "a".into(),
                    fields: vec![
                        NodeField::Input(spanned(TypeExpr::Named("Int".into()))),
                        NodeField::Output(spanned(TypeExpr::Named("Int".into()))),
                    ],
                })),
                spanned(GraphMember::Node(NodeDef {
                    name: "b".into(),
                    fields: vec![
                        NodeField::Input(spanned(TypeExpr::Named("Int".into()))),
                        NodeField::Output(spanned(TypeExpr::Named("Int".into()))),
                    ],
                })),
                spanned(GraphMember::Edge(EdgeDef {
                    from: "a".into(),
                    to: "b".into(),
                    properties: vec![],
                })),
                spanned(GraphMember::Edge(EdgeDef {
                    from: "b".into(),
                    to: "a".into(),
                    properties: vec![],
                })),
            ],
        };
        let exec = ExecutableGraph::from_ast(&cyclic).unwrap();
        let err = exec.check_cycles().unwrap_err();
        assert!(matches!(err, RuntimeError::Deadlock { .. }));
    }

    #[test]
    fn topological_order() {
        let ast_graph = make_simple_graph();
        let exec = ExecutableGraph::from_ast(&ast_graph).unwrap();
        let order = exec.execution_order().unwrap();
        assert_eq!(order.len(), 3);

        // Source must come before transform, transform before sink
        let source_idx = exec.node_indices["source"];
        let transform_idx = exec.node_indices["transform"];
        let sink_idx = exec.node_indices["sink"];

        let pos = |idx: NodeIndex| order.iter().position(|&i| i == idx).unwrap();
        assert!(pos(source_idx) < pos(transform_idx));
        assert!(pos(transform_idx) < pos(sink_idx));
    }

    #[test]
    fn parallel_groups_linear() {
        let ast_graph = make_simple_graph();
        let exec = ExecutableGraph::from_ast(&ast_graph).unwrap();
        let groups = exec.parallel_groups().unwrap();
        // Linear chain: each level has exactly 1 node
        assert_eq!(groups.len(), 3);
        assert_eq!(groups[0].len(), 1);
        assert_eq!(groups[1].len(), 1);
        assert_eq!(groups[2].len(), 1);
    }

    #[test]
    fn parallel_groups_diamond() {
        // Diamond: A → B, A → C, B → D, C → D
        let diamond = Graph {
            name: "diamond".into(),
            version: None,
            targets: vec![],
            members: vec![
                spanned(GraphMember::Node(NodeDef {
                    name: "A".into(),
                    fields: vec![NodeField::Output(spanned(TypeExpr::Named("Int".into())))],
                })),
                spanned(GraphMember::Node(NodeDef {
                    name: "B".into(),
                    fields: vec![
                        NodeField::Input(spanned(TypeExpr::Named("Int".into()))),
                        NodeField::Output(spanned(TypeExpr::Named("Int".into()))),
                    ],
                })),
                spanned(GraphMember::Node(NodeDef {
                    name: "C".into(),
                    fields: vec![
                        NodeField::Input(spanned(TypeExpr::Named("Int".into()))),
                        NodeField::Output(spanned(TypeExpr::Named("Int".into()))),
                    ],
                })),
                spanned(GraphMember::Node(NodeDef {
                    name: "D".into(),
                    fields: vec![NodeField::Input(spanned(TypeExpr::Named("Int".into())))],
                })),
                spanned(GraphMember::Edge(EdgeDef {
                    from: "A".into(),
                    to: "B".into(),
                    properties: vec![],
                })),
                spanned(GraphMember::Edge(EdgeDef {
                    from: "A".into(),
                    to: "C".into(),
                    properties: vec![],
                })),
                spanned(GraphMember::Edge(EdgeDef {
                    from: "B".into(),
                    to: "D".into(),
                    properties: vec![],
                })),
                spanned(GraphMember::Edge(EdgeDef {
                    from: "C".into(),
                    to: "D".into(),
                    properties: vec![],
                })),
            ],
        };

        let exec = ExecutableGraph::from_ast(&diamond).unwrap();
        let groups = exec.parallel_groups().unwrap();
        assert_eq!(groups.len(), 3);
        // Level 0: A (source)
        assert_eq!(groups[0].len(), 1);
        // Level 1: B and C (can run in parallel)
        assert_eq!(groups[1].len(), 2);
        // Level 2: D (sink)
        assert_eq!(groups[2].len(), 1);
    }

    #[test]
    fn missing_node_in_edge() {
        let bad = Graph {
            name: "bad".into(),
            version: None,
            targets: vec![],
            members: vec![
                spanned(GraphMember::Node(NodeDef {
                    name: "a".into(),
                    fields: vec![],
                })),
                spanned(GraphMember::Edge(EdgeDef {
                    from: "a".into(),
                    to: "nonexistent".into(),
                    properties: vec![],
                })),
            ],
        };
        let err = ExecutableGraph::from_ast(&bad).unwrap_err();
        assert!(matches!(err, RuntimeError::NodeFailed { .. }));
    }

    #[test]
    fn empty_graph() {
        let empty = Graph {
            name: "empty".into(),
            version: None,
            targets: vec![],
            members: vec![],
        };
        let exec = ExecutableGraph::from_ast(&empty).unwrap();
        assert!(exec.parallel_groups().unwrap().is_empty());
        assert!(exec.execution_order().unwrap().is_empty());
    }

    #[test]
    fn programmatic_construction() {
        let mut g = ExecutableGraph::new("manual");
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

        assert_eq!(g.graph.node_count(), 2);
        assert_eq!(g.graph.edge_count(), 1);
        assert!(g.check_cycles().is_ok());
    }
}
