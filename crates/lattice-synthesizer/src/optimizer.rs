//! Self-optimization engine.
//!
//! Analyzes profiling data against the program AST to produce
//! actionable optimization suggestions.

use lattice_parser::ast::{GraphMember, Item, Program};
use lattice_runtime::profiler::ProfileReport;
use serde::{Deserialize, Serialize};

/// A suggested optimization for a graph node.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OptimizationSuggestion {
    /// Node that should be optimized.
    pub node_name: String,
    /// Human-readable explanation.
    pub reason: String,
    /// Concrete action to take.
    pub suggested_action: SuggestedAction,
}

/// The kind of optimization to apply.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SuggestedAction {
    /// Independent nodes could run concurrently.
    Parallelize,
    /// Node could benefit from memoization.
    Cache,
    /// Node should be rewritten with the given hint.
    Rewrite(String),
    /// Node should be re-synthesized by the LLM.
    Synthesize,
}

/// Analyze a profile report against a program AST to find optimization opportunities.
pub fn analyze_hotspots(
    profile: &ProfileReport,
    program: &Program,
) -> Vec<OptimizationSuggestion> {
    let mut suggestions = Vec::new();
    let total_ns = profile.total_duration.as_nanos() as f64;
    if total_ns == 0.0 {
        return suggestions;
    }

    // Collect node properties from AST for cross-referencing
    let node_props = collect_node_properties(program);

    // Rule 1: Dominant nodes (>50% of total) → Synthesize or Rewrite
    for entry in &profile.entries {
        let pct = (entry.duration.as_nanos() as f64 / total_ns) * 100.0;
        if pct > 50.0 {
            let action = if node_props
                .get(&entry.node_name)
                .map_or(false, |p| p.has_synthesize)
            {
                SuggestedAction::Synthesize
            } else {
                SuggestedAction::Rewrite(format!(
                    "Node '{}' consumes {:.1}% of runtime — consider a more efficient algorithm",
                    entry.node_name, pct,
                ))
            };
            suggestions.push(OptimizationSuggestion {
                node_name: entry.node_name.clone(),
                reason: format!("Takes {:.1}% of total execution time", pct),
                suggested_action: action,
            });
        }
    }

    // Rule 2: Sequential independent nodes → Parallelize
    // Detect nodes at the same graph level that ran sequentially
    let sequential_groups = find_sequential_independent_nodes(profile, program);
    for group in sequential_groups {
        if group.len() >= 2 {
            for name in &group {
                suggestions.push(OptimizationSuggestion {
                    node_name: name.clone(),
                    reason: format!(
                        "Independent nodes [{}] are executed sequentially",
                        group.join(", "),
                    ),
                    suggested_action: SuggestedAction::Parallelize,
                });
            }
        }
    }

    // Rule 3: Deterministic/idempotent nodes → Cache
    for entry in &profile.entries {
        if let Some(props) = node_props.get(&entry.node_name) {
            if props.deterministic && props.idempotent {
                suggestions.push(OptimizationSuggestion {
                    node_name: entry.node_name.clone(),
                    reason: "Deterministic and idempotent node could benefit from caching".into(),
                    suggested_action: SuggestedAction::Cache,
                });
            }
        }
    }

    suggestions
}

/// Extracted properties for a single AST node.
struct NodeProps {
    deterministic: bool,
    idempotent: bool,
    has_synthesize: bool,
    /// Edges from this node (outgoing).
    edges_to: Vec<String>,
}

/// Walk the program and collect relevant node metadata.
fn collect_node_properties(program: &Program) -> std::collections::HashMap<String, NodeProps> {
    use lattice_parser::ast::{Expr, NodeField};
    let mut map = std::collections::HashMap::new();

    for item in program {
        if let Item::Graph(graph) = &item.node {
            // First pass: collect nodes
            for member in &graph.members {
                if let GraphMember::Node(node) = &member.node {
                    let mut deterministic = true;
                    let mut idempotent = false;
                    let mut has_synthesize = false;

                    for field in &node.fields {
                        match field {
                            NodeField::Properties(props) => {
                                for prop in props {
                                    match prop.key.as_str() {
                                        "deterministic" => {
                                            if let Expr::BoolLit(b) = &prop.value.node {
                                                deterministic = *b;
                                            }
                                        }
                                        "idempotent" => {
                                            if let Expr::BoolLit(b) = &prop.value.node {
                                                idempotent = *b;
                                            }
                                        }
                                        _ => {}
                                    }
                                }
                            }
                            NodeField::Solve(solve) => {
                                for constraint in &solve.constraints {
                                    if matches!(&constraint.node, Expr::Synthesize(_)) {
                                        has_synthesize = true;
                                    }
                                }
                            }
                            _ => {}
                        }
                    }

                    map.insert(
                        node.name.clone(),
                        NodeProps {
                            deterministic,
                            idempotent,
                            has_synthesize,
                            edges_to: Vec::new(),
                        },
                    );
                }
            }

            // Second pass: collect edges
            for member in &graph.members {
                if let GraphMember::Edge(edge) = &member.node {
                    if let Some(props) = map.get_mut(&edge.from) {
                        props.edges_to.push(edge.to.clone());
                    }
                }
            }
        }
    }

    map
}

/// Find groups of independent nodes that appear to have run sequentially.
///
/// Two nodes are "independent" if neither depends on the other (no edge
/// path between them). We approximate this by checking the AST edges.
fn find_sequential_independent_nodes(
    profile: &ProfileReport,
    program: &Program,
) -> Vec<Vec<String>> {
    let node_props = collect_node_properties(program);
    let profiled_names: Vec<&str> = profile.entries.iter().map(|e| e.node_name.as_str()).collect();
    let mut groups = Vec::new();

    // Build a simple reachability set per node
    let mut reachable: std::collections::HashMap<&str, std::collections::HashSet<&str>> =
        std::collections::HashMap::new();

    for name in &profiled_names {
        let mut visited = std::collections::HashSet::new();
        let mut stack: Vec<&str> = node_props
            .get(*name)
            .map(|p| p.edges_to.iter().map(|s| s.as_str()).collect())
            .unwrap_or_default();
        while let Some(n) = stack.pop() {
            if visited.insert(n) {
                if let Some(props) = node_props.get(n) {
                    stack.extend(props.edges_to.iter().map(|s| s.as_str()));
                }
            }
        }
        reachable.insert(name, visited);
    }

    // Find pairs of profiled nodes that are mutually unreachable
    let mut independent_group = Vec::new();
    for (i, a) in profiled_names.iter().enumerate() {
        for b in profiled_names.iter().skip(i + 1) {
            let a_reaches_b = reachable.get(a).map_or(false, |s| s.contains(b));
            let b_reaches_a = reachable.get(b).map_or(false, |s| s.contains(a));
            if !a_reaches_b && !b_reaches_a {
                independent_group.push(vec![a.to_string(), b.to_string()]);
            }
        }
    }

    // Deduplicate: merge overlapping pairs into groups
    // For simplicity we return pairs directly
    if !independent_group.is_empty() {
        groups = independent_group;
    }

    groups
}

#[cfg(test)]
mod tests {
    use super::*;
    use lattice_parser::ast::*;
    use lattice_runtime::profiler::{ProfileEntry, ProfileReport};
    use std::time::Duration;

    fn spanned<T>(node: T) -> Spanned<T> {
        Spanned::new(node, Span::dummy())
    }

    fn make_program_with_graph(nodes: Vec<NodeDef>, edges: Vec<EdgeDef>) -> Program {
        let mut members: Vec<Spanned<GraphMember>> = nodes
            .into_iter()
            .map(|n| spanned(GraphMember::Node(n)))
            .collect();
        members.extend(edges.into_iter().map(|e| spanned(GraphMember::Edge(e))));
        vec![spanned(Item::Graph(Graph {
            name: "test".into(),
            version: None,
            targets: vec![],
            members,
        }))]
    }

    #[test]
    fn dominant_node_suggests_rewrite() {
        let profile = ProfileReport {
            entries: vec![
                ProfileEntry {
                    node_name: "fast".into(),
                    duration: Duration::from_millis(10),
                    input_size: None,
                    output_size: None,
                },
                ProfileEntry {
                    node_name: "slow".into(),
                    duration: Duration::from_millis(90),
                    input_size: None,
                    output_size: None,
                },
            ],
            total_duration: Duration::from_millis(100),
        };

        let program = make_program_with_graph(
            vec![
                NodeDef {
                    name: "fast".into(),
                    fields: vec![],
                },
                NodeDef {
                    name: "slow".into(),
                    fields: vec![],
                },
            ],
            vec![],
        );

        let suggestions = analyze_hotspots(&profile, &program);
        assert!(
            suggestions.iter().any(|s| s.node_name == "slow"
                && matches!(s.suggested_action, SuggestedAction::Rewrite(_))),
            "should suggest rewrite for dominant node: {suggestions:?}"
        );
    }

    #[test]
    fn independent_nodes_suggest_parallelize() {
        let profile = ProfileReport {
            entries: vec![
                ProfileEntry {
                    node_name: "A".into(),
                    duration: Duration::from_millis(30),
                    input_size: None,
                    output_size: None,
                },
                ProfileEntry {
                    node_name: "B".into(),
                    duration: Duration::from_millis(30),
                    input_size: None,
                    output_size: None,
                },
            ],
            total_duration: Duration::from_millis(60),
        };

        // A and B are independent (no edges between them)
        let program = make_program_with_graph(
            vec![
                NodeDef {
                    name: "A".into(),
                    fields: vec![
                        NodeField::Input(spanned(TypeExpr::Named("Int".into()))),
                        NodeField::Output(spanned(TypeExpr::Named("Int".into()))),
                    ],
                },
                NodeDef {
                    name: "B".into(),
                    fields: vec![
                        NodeField::Input(spanned(TypeExpr::Named("Int".into()))),
                        NodeField::Output(spanned(TypeExpr::Named("Int".into()))),
                    ],
                },
            ],
            vec![], // No edges — they are independent
        );

        let suggestions = analyze_hotspots(&profile, &program);
        assert!(
            suggestions
                .iter()
                .any(|s| matches!(s.suggested_action, SuggestedAction::Parallelize)),
            "should suggest parallelization: {suggestions:?}"
        );
    }

    #[test]
    fn idempotent_deterministic_suggests_cache() {
        let profile = ProfileReport {
            entries: vec![ProfileEntry {
                node_name: "lookup".into(),
                duration: Duration::from_millis(20),
                input_size: None,
                output_size: None,
            }],
            total_duration: Duration::from_millis(100),
        };

        let program = make_program_with_graph(
            vec![NodeDef {
                name: "lookup".into(),
                fields: vec![NodeField::Properties(vec![
                    Property {
                        key: "deterministic".into(),
                        value: spanned(Expr::BoolLit(true)),
                    },
                    Property {
                        key: "idempotent".into(),
                        value: spanned(Expr::BoolLit(true)),
                    },
                ])],
            }],
            vec![],
        );

        let suggestions = analyze_hotspots(&profile, &program);
        assert!(
            suggestions
                .iter()
                .any(|s| s.node_name == "lookup"
                    && matches!(s.suggested_action, SuggestedAction::Cache)),
            "should suggest caching: {suggestions:?}"
        );
    }

    #[test]
    fn empty_profile_produces_no_suggestions() {
        let profile = ProfileReport {
            entries: vec![],
            total_duration: Duration::from_millis(0),
        };
        let program: Program = vec![];
        let suggestions = analyze_hotspots(&profile, &program);
        assert!(suggestions.is_empty());
    }
}
