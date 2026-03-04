use lattice_parser::ast;
use std::collections::HashSet;

/// Results from running optimization passes.
#[derive(Debug, Default)]
pub struct OptimizationReport {
    pub dead_nodes_removed: usize,
    pub constants_folded: usize,
}

/// Run all optimization passes on a program and its graphs.
pub fn optimize(
    program: &mut ast::Program,
    graphs: &mut [ast::Graph],
) -> OptimizationReport {
    let mut report = OptimizationReport::default();

    for graph in graphs.iter_mut() {
        report.dead_nodes_removed += eliminate_dead_nodes(graph);
    }
    report.constants_folded = fold_constants(program);

    report
}

/// Remove unreachable nodes: nodes that expect input (have input fields)
/// but have no incoming edges.
pub fn eliminate_dead_nodes(graph: &mut ast::Graph) -> usize {
    let mut has_incoming: HashSet<String> = HashSet::new();
    let mut source_nodes: HashSet<String> = HashSet::new();

    for member in &graph.members {
        match &member.node {
            ast::GraphMember::Node(node) => {
                let has_input = node
                    .fields
                    .iter()
                    .any(|f| matches!(f, ast::NodeField::Input(_)));
                let has_output = node
                    .fields
                    .iter()
                    .any(|f| matches!(f, ast::NodeField::Output(_)));

                // Source = has output but no input
                if has_output && !has_input {
                    source_nodes.insert(node.name.clone());
                }
            }
            ast::GraphMember::Edge(edge) => {
                has_incoming.insert(edge.to.clone());
            }
            _ => {}
        }
    }

    // Collect all node names
    let all_nodes: HashSet<String> = graph
        .members
        .iter()
        .filter_map(|m| {
            if let ast::GraphMember::Node(n) = &m.node {
                Some(n.name.clone())
            } else {
                None
            }
        })
        .collect();

    // Dead = not a source, and has no incoming edges
    let dead: HashSet<String> = all_nodes
        .into_iter()
        .filter(|name| !source_nodes.contains(name) && !has_incoming.contains(name))
        .collect();

    if dead.is_empty() {
        return 0;
    }

    let count = dead.len();

    // Remove dead nodes and edges that reference them
    graph.members.retain(|member| match &member.node {
        ast::GraphMember::Node(node) => !dead.contains(&node.name),
        ast::GraphMember::Edge(edge) => {
            !dead.contains(&edge.from) && !dead.contains(&edge.to)
        }
        _ => true,
    });

    count
}

/// Fold constant expressions in the AST (e.g. `2 + 3` → `5`).
/// Returns the number of folds performed.
pub fn fold_constants(program: &mut ast::Program) -> usize {
    let mut count = 0;
    for item in program.iter_mut() {
        match &mut item.node {
            ast::Item::Function(func) => {
                if let ast::FunctionBody::Block(exprs) = &mut func.body {
                    for expr in exprs.iter_mut() {
                        count += fold_expr(expr);
                    }
                }
            }
            ast::Item::LetBinding(binding) => {
                count += fold_expr(&mut binding.value);
            }
            _ => {}
        }
    }
    count
}

fn fold_expr(expr: &mut ast::Spanned<ast::Expr>) -> usize {
    let mut count = 0;

    // Recursively fold sub-expressions first
    match &mut expr.node {
        ast::Expr::BinOp { left, right, .. } => {
            count += fold_expr(left);
            count += fold_expr(right);
        }
        ast::Expr::UnaryOp { operand, .. } => {
            count += fold_expr(operand);
        }
        ast::Expr::Let { value, .. } => {
            count += fold_expr(value);
        }
        ast::Expr::Call { func, args } => {
            count += fold_expr(func);
            for arg in args.iter_mut() {
                count += fold_expr(arg);
            }
        }
        ast::Expr::If { cond, then_, else_ } => {
            count += fold_expr(cond);
            count += fold_expr(then_);
            if let Some(e) = else_ {
                count += fold_expr(e);
            }
        }
        ast::Expr::Block(exprs) => {
            for e in exprs.iter_mut() {
                count += fold_expr(e);
            }
        }
        ast::Expr::Array(elems) => {
            for e in elems.iter_mut() {
                count += fold_expr(e);
            }
        }
        ast::Expr::Record(fields) => {
            for (_, e) in fields.iter_mut() {
                count += fold_expr(e);
            }
        }
        ast::Expr::Field { expr: inner, .. } => {
            count += fold_expr(inner);
        }
        ast::Expr::Pipeline { left, right } => {
            count += fold_expr(left);
            count += fold_expr(right);
        }
        _ => {}
    }

    // Try to fold this node
    let folded = match &expr.node {
        ast::Expr::BinOp { left, op, right } => match (&left.node, op, &right.node) {
            // Integer arithmetic
            (ast::Expr::IntLit(a), ast::BinOp::Add, ast::Expr::IntLit(b)) => {
                Some(ast::Expr::IntLit(a + b))
            }
            (ast::Expr::IntLit(a), ast::BinOp::Sub, ast::Expr::IntLit(b)) => {
                Some(ast::Expr::IntLit(a - b))
            }
            (ast::Expr::IntLit(a), ast::BinOp::Mul, ast::Expr::IntLit(b)) => {
                Some(ast::Expr::IntLit(a * b))
            }
            (ast::Expr::IntLit(a), ast::BinOp::Div, ast::Expr::IntLit(b)) if *b != 0 => {
                Some(ast::Expr::IntLit(a / b))
            }
            (ast::Expr::IntLit(a), ast::BinOp::Mod, ast::Expr::IntLit(b)) if *b != 0 => {
                Some(ast::Expr::IntLit(a % b))
            }
            // Float arithmetic
            (ast::Expr::FloatLit(a), ast::BinOp::Add, ast::Expr::FloatLit(b)) => {
                Some(ast::Expr::FloatLit(a + b))
            }
            (ast::Expr::FloatLit(a), ast::BinOp::Sub, ast::Expr::FloatLit(b)) => {
                Some(ast::Expr::FloatLit(a - b))
            }
            (ast::Expr::FloatLit(a), ast::BinOp::Mul, ast::Expr::FloatLit(b)) => {
                Some(ast::Expr::FloatLit(a * b))
            }
            // Boolean logic
            (ast::Expr::BoolLit(a), ast::BinOp::And, ast::Expr::BoolLit(b)) => {
                Some(ast::Expr::BoolLit(*a && *b))
            }
            (ast::Expr::BoolLit(a), ast::BinOp::Or, ast::Expr::BoolLit(b)) => {
                Some(ast::Expr::BoolLit(*a || *b))
            }
            // Integer comparison
            (ast::Expr::IntLit(a), ast::BinOp::Eq, ast::Expr::IntLit(b)) => {
                Some(ast::Expr::BoolLit(a == b))
            }
            (ast::Expr::IntLit(a), ast::BinOp::Neq, ast::Expr::IntLit(b)) => {
                Some(ast::Expr::BoolLit(a != b))
            }
            (ast::Expr::IntLit(a), ast::BinOp::Lt, ast::Expr::IntLit(b)) => {
                Some(ast::Expr::BoolLit(a < b))
            }
            (ast::Expr::IntLit(a), ast::BinOp::Gt, ast::Expr::IntLit(b)) => {
                Some(ast::Expr::BoolLit(a > b))
            }
            (ast::Expr::IntLit(a), ast::BinOp::Leq, ast::Expr::IntLit(b)) => {
                Some(ast::Expr::BoolLit(a <= b))
            }
            (ast::Expr::IntLit(a), ast::BinOp::Geq, ast::Expr::IntLit(b)) => {
                Some(ast::Expr::BoolLit(a >= b))
            }
            _ => None,
        },
        ast::Expr::UnaryOp { op, operand } => match (op, &operand.node) {
            (ast::UnaryOp::Neg, ast::Expr::IntLit(n)) => Some(ast::Expr::IntLit(-n)),
            (ast::UnaryOp::Neg, ast::Expr::FloatLit(f)) => Some(ast::Expr::FloatLit(-f)),
            (ast::UnaryOp::Not, ast::Expr::BoolLit(b)) => Some(ast::Expr::BoolLit(!b)),
            _ => None,
        },
        _ => None,
    };

    if let Some(new_node) = folded {
        expr.node = new_node;
        count += 1;
    }

    count
}

#[cfg(test)]
mod tests {
    use super::*;
    use lattice_parser::ast::*;

    fn spanned<T>(node: T) -> Spanned<T> {
        Spanned::new(node, Span::dummy())
    }

    // ── Dead node elimination ───────────────

    #[test]
    fn eliminate_dead_nodes_removes_orphans() {
        let mut graph = Graph {
            name: "test".into(),
            version: None,
            targets: vec![],
            members: vec![
                // Source node (output only) — should survive
                spanned(GraphMember::Node(NodeDef {
                    name: "source".into(),
                    fields: vec![NodeField::Output(spanned(TypeExpr::Named("Int".into())))],
                })),
                // Compute node with input+output, connected — should survive
                spanned(GraphMember::Node(NodeDef {
                    name: "connected".into(),
                    fields: vec![
                        NodeField::Input(spanned(TypeExpr::Named("Int".into()))),
                        NodeField::Output(spanned(TypeExpr::Named("Int".into()))),
                    ],
                })),
                // Orphan node: has input but no incoming edge — dead
                spanned(GraphMember::Node(NodeDef {
                    name: "orphan".into(),
                    fields: vec![
                        NodeField::Input(spanned(TypeExpr::Named("Int".into()))),
                        NodeField::Output(spanned(TypeExpr::Named("Int".into()))),
                    ],
                })),
                spanned(GraphMember::Edge(EdgeDef {
                    from: "source".into(),
                    to: "connected".into(),
                    properties: vec![],
                })),
            ],
        };

        let removed = eliminate_dead_nodes(&mut graph);
        assert_eq!(removed, 1);

        let node_names: Vec<&str> = graph
            .members
            .iter()
            .filter_map(|m| {
                if let GraphMember::Node(n) = &m.node {
                    Some(n.name.as_str())
                } else {
                    None
                }
            })
            .collect();

        assert!(node_names.contains(&"source"));
        assert!(node_names.contains(&"connected"));
        assert!(!node_names.contains(&"orphan"));
    }

    #[test]
    fn eliminate_dead_nodes_no_change_on_healthy_graph() {
        let mut graph = Graph {
            name: "healthy".into(),
            version: None,
            targets: vec![],
            members: vec![
                spanned(GraphMember::Node(NodeDef {
                    name: "src".into(),
                    fields: vec![NodeField::Output(spanned(TypeExpr::Named("Int".into())))],
                })),
                spanned(GraphMember::Node(NodeDef {
                    name: "sink".into(),
                    fields: vec![NodeField::Input(spanned(TypeExpr::Named("Int".into())))],
                })),
                spanned(GraphMember::Edge(EdgeDef {
                    from: "src".into(),
                    to: "sink".into(),
                    properties: vec![],
                })),
            ],
        };

        let removed = eliminate_dead_nodes(&mut graph);
        assert_eq!(removed, 0);
        // 2 nodes + 1 edge
        assert_eq!(graph.members.len(), 3);
    }

    // ── Constant folding ────────────────────

    #[test]
    fn fold_integer_arithmetic() {
        // let result = 2 + 3 * 4
        let mut program: Program = vec![spanned(Item::LetBinding(LetBinding {
            name: "result".into(),
            type_ann: None,
            value: spanned(Expr::BinOp {
                left: Box::new(spanned(Expr::IntLit(2))),
                op: BinOp::Add,
                right: Box::new(spanned(Expr::BinOp {
                    left: Box::new(spanned(Expr::IntLit(3))),
                    op: BinOp::Mul,
                    right: Box::new(spanned(Expr::IntLit(4))),
                })),
            }),
        }))];

        let folded = fold_constants(&mut program);
        assert_eq!(folded, 2); // inner mul + outer add

        if let Item::LetBinding(binding) = &program[0].node {
            assert!(
                matches!(binding.value.node, Expr::IntLit(14)),
                "expected IntLit(14), got {:?}",
                binding.value.node
            );
        } else {
            panic!("expected LetBinding");
        }
    }

    #[test]
    fn fold_boolean_logic() {
        // let flag = true && false
        let mut program: Program = vec![spanned(Item::LetBinding(LetBinding {
            name: "flag".into(),
            type_ann: None,
            value: spanned(Expr::BinOp {
                left: Box::new(spanned(Expr::BoolLit(true))),
                op: BinOp::And,
                right: Box::new(spanned(Expr::BoolLit(false))),
            }),
        }))];

        let folded = fold_constants(&mut program);
        assert_eq!(folded, 1);

        if let Item::LetBinding(binding) = &program[0].node {
            assert!(matches!(binding.value.node, Expr::BoolLit(false)));
        } else {
            panic!("expected LetBinding");
        }
    }

    #[test]
    fn fold_unary_neg() {
        // let x = -42
        let mut program: Program = vec![spanned(Item::LetBinding(LetBinding {
            name: "x".into(),
            type_ann: None,
            value: spanned(Expr::UnaryOp {
                op: UnaryOp::Neg,
                operand: Box::new(spanned(Expr::IntLit(42))),
            }),
        }))];

        let folded = fold_constants(&mut program);
        assert_eq!(folded, 1);

        if let Item::LetBinding(binding) = &program[0].node {
            assert!(matches!(binding.value.node, Expr::IntLit(-42)));
        } else {
            panic!("expected LetBinding");
        }
    }

    #[test]
    fn fold_skips_division_by_zero() {
        // let x = 10 / 0  — should NOT fold
        let mut program: Program = vec![spanned(Item::LetBinding(LetBinding {
            name: "x".into(),
            type_ann: None,
            value: spanned(Expr::BinOp {
                left: Box::new(spanned(Expr::IntLit(10))),
                op: BinOp::Div,
                right: Box::new(spanned(Expr::IntLit(0))),
            }),
        }))];

        let folded = fold_constants(&mut program);
        assert_eq!(folded, 0);

        // Still a BinOp
        if let Item::LetBinding(binding) = &program[0].node {
            assert!(matches!(binding.value.node, Expr::BinOp { .. }));
        }
    }

    #[test]
    fn fold_leaves_variables_alone() {
        // let y = x + 1  — can't fold, x is a variable
        let mut program: Program = vec![spanned(Item::LetBinding(LetBinding {
            name: "y".into(),
            type_ann: None,
            value: spanned(Expr::BinOp {
                left: Box::new(spanned(Expr::Ident("x".into()))),
                op: BinOp::Add,
                right: Box::new(spanned(Expr::IntLit(1))),
            }),
        }))];

        let folded = fold_constants(&mut program);
        assert_eq!(folded, 0);
    }
}
