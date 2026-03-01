//! Conversion between AST and BSG representations.
//!
//! Implements the bidirectional mapping:
//! - Surface Syntax (AST) → BSG (for compilation)
//! - BSG → Surface Syntax (AST) (for rendering/debugging)

use crate::bsg;
use lattice_parser::ast;

// ── AST → BSG ──────────────────────────────────────────────

/// Convert a parsed AST program into a BSG Graph.
///
/// Finds the first `Graph` item and converts its nodes/edges.
/// Non-graph items (functions, types, etc.) are skipped.
pub fn ast_to_bsg(program: &[ast::Spanned<ast::Item>]) -> bsg::Graph {
    let mut graph = bsg::Graph::default();

    for item in program {
        if let ast::Item::Graph(g) = &item.node {
            graph.id = g.name.clone();
            graph.version = g.version.clone().unwrap_or_default();

            for member in &g.members {
                match &member.node {
                    ast::GraphMember::Node(n) => graph.nodes.push(convert_node(n)),
                    ast::GraphMember::Edge(e) => graph.edges.push(convert_edge(e)),
                    ast::GraphMember::Solve(_) => {}
                }
            }
            break;
        }
    }

    graph
}

fn convert_node(node: &ast::NodeDef) -> bsg::Node {
    let mut bsg_node = bsg::Node {
        id: node.name.clone(),
        kind: bsg::NodeKind::Compute as i32,
        ..Default::default()
    };

    for field in &node.fields {
        match field {
            ast::NodeField::Input(ty) => {
                bsg_node.inputs.push(bsg::TypedPort {
                    name: "default".into(),
                    r#type: Some(convert_type_expr(&ty.node)),
                });
            }
            ast::NodeField::Output(ty) => {
                bsg_node.outputs.push(bsg::TypedPort {
                    name: "default".into(),
                    r#type: Some(convert_type_expr(&ty.node)),
                });
            }
            ast::NodeField::Properties(props) => {
                bsg_node.properties = Some(convert_properties(props));
            }
            ast::NodeField::Semantic(sem) => {
                bsg_node.semantic = Some(convert_semantic(sem));
            }
            ast::NodeField::ProofObligations(pos) => {
                bsg_node.proof_obligations = pos.iter().map(convert_proof_obligation).collect();
            }
            ast::NodeField::Pre(_) | ast::NodeField::Post(_) | ast::NodeField::Solve(_) => {}
        }
    }

    bsg_node
}

fn convert_edge(edge: &ast::EdgeDef) -> bsg::Edge {
    let properties = if edge.properties.is_empty() {
        None
    } else {
        Some(convert_edge_properties(&edge.properties))
    };

    bsg::Edge {
        source_node: edge.from.clone(),
        source_port: "default".into(),
        target_node: edge.to.clone(),
        target_port: "default".into(),
        properties,
    }
}

fn convert_type_expr(ty: &ast::TypeExpr) -> bsg::Type {
    let kind = match ty {
        ast::TypeExpr::Named(name) => {
            bsg::r#type::Kind::Primitive(bsg::PrimitiveType { name: name.clone() })
        }
        ast::TypeExpr::Applied { name, args } => {
            bsg::r#type::Kind::Dependent(bsg::DependentType {
                name: name.clone(),
                params: args
                    .iter()
                    .enumerate()
                    .map(|(i, a)| bsg::TypeParam {
                        name: format!("T{i}"),
                        r#type: Some(convert_type_expr(&a.node)),
                    })
                    .collect(),
            })
        }
        ast::TypeExpr::Function { params, ret } => {
            bsg::r#type::Kind::Function(Box::new(bsg::FunctionType {
                params: params.iter().map(|p| convert_type_expr(&p.node)).collect(),
                return_type: Some(Box::new(convert_type_expr(&ret.node))),
            }))
        }
        ast::TypeExpr::Record(fields) => bsg::r#type::Kind::Product(bsg::ProductType {
            fields: fields
                .iter()
                .map(|(name, ty)| bsg::Field {
                    name: name.clone(),
                    r#type: Some(convert_type_expr(&ty.node)),
                })
                .collect(),
        }),
        ast::TypeExpr::Sum(variants) => bsg::r#type::Kind::Sum(bsg::SumType {
            variants: variants
                .iter()
                .map(|v| bsg::Variant {
                    name: v.name.clone(),
                    payload: if v.fields.is_empty() {
                        None
                    } else {
                        Some(bsg::Type {
                            kind: Some(bsg::r#type::Kind::Product(bsg::ProductType {
                                fields: v
                                    .fields
                                    .iter()
                                    .map(|(n, t)| bsg::Field {
                                        name: n.clone(),
                                        r#type: Some(convert_type_expr(&t.node)),
                                    })
                                    .collect(),
                            })),
                        })
                    },
                })
                .collect(),
        }),
        ast::TypeExpr::Refinement {
            base, predicate, ..
        } => bsg::r#type::Kind::Refinement(Box::new(bsg::RefinementType {
            base: Some(Box::new(convert_type_expr(&base.node))),
            predicate: expr_to_string(&predicate.node),
        })),
        ast::TypeExpr::Dependent { name, params } => {
            bsg::r#type::Kind::Dependent(bsg::DependentType {
                name: name.clone(),
                params: params
                    .iter()
                    .map(|(pname, pty)| bsg::TypeParam {
                        name: pname.clone(),
                        r#type: Some(convert_type_expr(&pty.node)),
                    })
                    .collect(),
            })
        }
        ast::TypeExpr::Stream(inner) => {
            bsg::r#type::Kind::Stream(Box::new(bsg::StreamType {
                element: Some(Box::new(convert_type_expr(&inner.node))),
            }))
        }
        ast::TypeExpr::Distribution(inner) => {
            bsg::r#type::Kind::Distribution(Box::new(bsg::DistributionType {
                inner: Some(Box::new(convert_type_expr(&inner.node))),
            }))
        }
        ast::TypeExpr::Where { base, .. } => return convert_type_expr(&base.node),
    };

    bsg::Type { kind: Some(kind) }
}

fn convert_properties(props: &[ast::Property]) -> bsg::Properties {
    let mut bsg_props = bsg::Properties::default();

    for prop in props {
        match prop.key.as_str() {
            "idempotent" => {
                if let ast::Expr::BoolLit(b) = &prop.value.node {
                    bsg_props.idempotent = *b;
                }
            }
            "deterministic" => {
                if let ast::Expr::BoolLit(b) = &prop.value.node {
                    bsg_props.deterministic = *b;
                }
            }
            other => {
                bsg_props
                    .extra
                    .insert(other.to_string(), expr_to_string(&prop.value.node));
            }
        }
    }

    bsg_props
}

fn convert_edge_properties(props: &[ast::Property]) -> bsg::EdgeProperties {
    let mut bsg_props = bsg::EdgeProperties::default();

    for prop in props {
        match prop.key.as_str() {
            "async" => {
                if let ast::Expr::BoolLit(b) = &prop.value.node {
                    bsg_props.r#async = *b;
                }
            }
            "buffered" => {
                if let ast::Expr::BoolLit(b) = &prop.value.node {
                    bsg_props.buffered = *b;
                }
            }
            other => {
                bsg_props
                    .extra
                    .insert(other.to_string(), expr_to_string(&prop.value.node));
            }
        }
    }

    bsg_props
}

fn convert_semantic(sem: &ast::SemanticBlock) -> bsg::SemanticSpec {
    bsg::SemanticSpec {
        natural_language: sem.description.clone().unwrap_or_default(),
        formal_spec: sem
            .formal
            .as_ref()
            .map(|f| expr_to_string(&f.node).into_bytes()),
        examples: sem
            .examples
            .iter()
            .map(|ex| bsg::Example {
                input: expr_to_string(&ex.input.node).into_bytes(),
                expected_output: expr_to_string(&ex.output.node).into_bytes(),
                description: String::new(),
            })
            .collect(),
    }
}

fn convert_proof_obligation(po: &ast::ProofObligation) -> bsg::ProofObligation {
    bsg::ProofObligation {
        name: po.name.clone(),
        expression: expr_to_string(&po.expr.node),
    }
}

// ── BSG → AST ──────────────────────────────────────────────

/// Convert a BSG Graph back to an AST program.
///
/// Produces a single `Graph` item containing the reconstructed nodes and edges.
/// Expressions stored as strings in BSG are recovered as `Expr::Ident`.
pub fn bsg_to_ast(graph: &bsg::Graph) -> Vec<ast::Spanned<ast::Item>> {
    let mut members = Vec::new();

    for node in &graph.nodes {
        members.push(ast::Spanned::dummy(ast::GraphMember::Node(
            bsg_node_to_ast(node),
        )));
    }

    for edge in &graph.edges {
        members.push(ast::Spanned::dummy(ast::GraphMember::Edge(
            bsg_edge_to_ast(edge),
        )));
    }

    let g = ast::Graph {
        name: graph.id.clone(),
        version: if graph.version.is_empty() {
            None
        } else {
            Some(graph.version.clone())
        },
        targets: Vec::new(),
        members,
    };

    vec![ast::Spanned::dummy(ast::Item::Graph(g))]
}

fn bsg_node_to_ast(node: &bsg::Node) -> ast::NodeDef {
    let mut fields = Vec::new();

    for port in &node.inputs {
        if let Some(ty) = &port.r#type {
            fields.push(ast::NodeField::Input(ast::Spanned::dummy(bsg_type_to_ast(
                ty,
            ))));
        }
    }

    for port in &node.outputs {
        if let Some(ty) = &port.r#type {
            fields.push(ast::NodeField::Output(ast::Spanned::dummy(
                bsg_type_to_ast(ty),
            )));
        }
    }

    if let Some(props) = &node.properties {
        let ast_props = bsg_properties_to_ast(props);
        if !ast_props.is_empty() {
            fields.push(ast::NodeField::Properties(ast_props));
        }
    }

    if let Some(sem) = &node.semantic {
        fields.push(ast::NodeField::Semantic(bsg_semantic_to_ast(sem)));
    }

    if !node.proof_obligations.is_empty() {
        fields.push(ast::NodeField::ProofObligations(
            node.proof_obligations
                .iter()
                .map(|po| ast::ProofObligation {
                    name: po.name.clone(),
                    expr: ast::Spanned::dummy(ast::Expr::Ident(po.expression.clone())),
                })
                .collect(),
        ));
    }

    ast::NodeDef {
        name: node.id.clone(),
        fields,
    }
}

fn bsg_edge_to_ast(edge: &bsg::Edge) -> ast::EdgeDef {
    let properties = edge
        .properties
        .as_ref()
        .map(bsg_edge_properties_to_ast)
        .unwrap_or_default();

    ast::EdgeDef {
        from: edge.source_node.clone(),
        to: edge.target_node.clone(),
        properties,
    }
}

fn bsg_type_to_ast(ty: &bsg::Type) -> ast::TypeExpr {
    match &ty.kind {
        Some(bsg::r#type::Kind::Primitive(p)) => ast::TypeExpr::Named(p.name.clone()),
        Some(bsg::r#type::Kind::Function(f)) => ast::TypeExpr::Function {
            params: f
                .params
                .iter()
                .map(|p| ast::Spanned::dummy(bsg_type_to_ast(p)))
                .collect(),
            ret: Box::new(ast::Spanned::dummy(
                f.return_type
                    .as_deref()
                    .map(bsg_type_to_ast)
                    .unwrap_or_else(|| ast::TypeExpr::Named("Unit".into())),
            )),
        },
        Some(bsg::r#type::Kind::Product(p)) => ast::TypeExpr::Record(
            p.fields
                .iter()
                .map(|f| {
                    (
                        f.name.clone(),
                        ast::Spanned::dummy(
                            f.r#type
                                .as_ref()
                                .map(bsg_type_to_ast)
                                .unwrap_or_else(|| ast::TypeExpr::Named("Unknown".into())),
                        ),
                    )
                })
                .collect(),
        ),
        Some(bsg::r#type::Kind::Sum(s)) => ast::TypeExpr::Sum(
            s.variants
                .iter()
                .map(|v| ast::Variant {
                    name: v.name.clone(),
                    fields: v
                        .payload
                        .as_ref()
                        .map(|p| {
                            if let Some(bsg::r#type::Kind::Product(prod)) = &p.kind {
                                prod.fields
                                    .iter()
                                    .map(|f| {
                                        (
                                            f.name.clone(),
                                            ast::Spanned::dummy(
                                                f.r#type
                                                    .as_ref()
                                                    .map(bsg_type_to_ast)
                                                    .unwrap_or_else(|| {
                                                        ast::TypeExpr::Named("Unknown".into())
                                                    }),
                                            ),
                                        )
                                    })
                                    .collect()
                            } else {
                                vec![(
                                    "value".into(),
                                    ast::Spanned::dummy(bsg_type_to_ast(p)),
                                )]
                            }
                        })
                        .unwrap_or_default(),
                })
                .collect(),
        ),
        Some(bsg::r#type::Kind::Refinement(r)) => ast::TypeExpr::Refinement {
            var: "x".into(),
            base: Box::new(ast::Spanned::dummy(
                r.base
                    .as_deref()
                    .map(bsg_type_to_ast)
                    .unwrap_or_else(|| ast::TypeExpr::Named("Unknown".into())),
            )),
            predicate: Box::new(ast::Spanned::dummy(ast::Expr::Ident(
                r.predicate.clone(),
            ))),
        },
        Some(bsg::r#type::Kind::Dependent(d)) => {
            if d.params.is_empty() {
                ast::TypeExpr::Named(d.name.clone())
            } else {
                ast::TypeExpr::Dependent {
                    name: d.name.clone(),
                    params: d
                        .params
                        .iter()
                        .map(|p| {
                            (
                                p.name.clone(),
                                ast::Spanned::dummy(
                                    p.r#type
                                        .as_ref()
                                        .map(bsg_type_to_ast)
                                        .unwrap_or_else(|| ast::TypeExpr::Named("Unknown".into())),
                                ),
                            )
                        })
                        .collect(),
                }
            }
        }
        Some(bsg::r#type::Kind::Stream(s)) => ast::TypeExpr::Stream(Box::new(
            ast::Spanned::dummy(
                s.element
                    .as_deref()
                    .map(bsg_type_to_ast)
                    .unwrap_or_else(|| ast::TypeExpr::Named("Unknown".into())),
            ),
        )),
        Some(bsg::r#type::Kind::Distribution(d)) => ast::TypeExpr::Distribution(Box::new(
            ast::Spanned::dummy(
                d.inner
                    .as_deref()
                    .map(bsg_type_to_ast)
                    .unwrap_or_else(|| ast::TypeExpr::Named("Unknown".into())),
            ),
        )),
        None => ast::TypeExpr::Named("Unknown".into()),
    }
}

fn bsg_properties_to_ast(props: &bsg::Properties) -> Vec<ast::Property> {
    let mut result = Vec::new();

    if props.idempotent {
        result.push(ast::Property {
            key: "idempotent".into(),
            value: ast::Spanned::dummy(ast::Expr::BoolLit(true)),
        });
    }
    if props.deterministic {
        result.push(ast::Property {
            key: "deterministic".into(),
            value: ast::Spanned::dummy(ast::Expr::BoolLit(true)),
        });
    }

    for (k, v) in &props.extra {
        result.push(ast::Property {
            key: k.clone(),
            value: ast::Spanned::dummy(ast::Expr::StringLit(v.clone())),
        });
    }

    result
}

fn bsg_edge_properties_to_ast(props: &bsg::EdgeProperties) -> Vec<ast::Property> {
    let mut result = Vec::new();

    if props.r#async {
        result.push(ast::Property {
            key: "async".into(),
            value: ast::Spanned::dummy(ast::Expr::BoolLit(true)),
        });
    }
    if props.buffered {
        result.push(ast::Property {
            key: "buffered".into(),
            value: ast::Spanned::dummy(ast::Expr::BoolLit(true)),
        });
    }

    for (k, v) in &props.extra {
        result.push(ast::Property {
            key: k.clone(),
            value: ast::Spanned::dummy(ast::Expr::StringLit(v.clone())),
        });
    }

    result
}

fn bsg_semantic_to_ast(sem: &bsg::SemanticSpec) -> ast::SemanticBlock {
    ast::SemanticBlock {
        description: if sem.natural_language.is_empty() {
            None
        } else {
            Some(sem.natural_language.clone())
        },
        formal: sem.formal_spec.as_ref().and_then(|b| {
            String::from_utf8(b.clone())
                .ok()
                .map(|s| ast::Spanned::dummy(ast::Expr::Ident(s)))
        }),
        examples: sem
            .examples
            .iter()
            .filter_map(|ex| {
                let input = String::from_utf8(ex.input.clone()).ok()?;
                let output = String::from_utf8(ex.expected_output.clone()).ok()?;
                Some(ast::Example {
                    input: ast::Spanned::dummy(ast::Expr::Ident(input)),
                    output: ast::Spanned::dummy(ast::Expr::Ident(output)),
                })
            })
            .collect(),
    }
}

// ── Public test helpers ────────────────────────────────────

#[doc(hidden)]
pub fn convert_type_expr_pub(ty: &ast::TypeExpr) -> bsg::Type {
    convert_type_expr(ty)
}

#[doc(hidden)]
pub fn bsg_type_to_ast_pub(ty: &bsg::Type) -> ast::TypeExpr {
    bsg_type_to_ast(ty)
}

// ── Expr → String helpers ──────────────────────────────────

fn expr_to_string(expr: &ast::Expr) -> String {
    match expr {
        ast::Expr::IntLit(n) => n.to_string(),
        ast::Expr::FloatLit(n) => n.to_string(),
        ast::Expr::StringLit(s) => format!("\"{s}\""),
        ast::Expr::BoolLit(b) => b.to_string(),
        ast::Expr::Ident(s) => s.clone(),
        ast::Expr::BinOp { left, op, right } => {
            format!(
                "{} {} {}",
                expr_to_string(&left.node),
                binop_str(*op),
                expr_to_string(&right.node)
            )
        }
        ast::Expr::UnaryOp { op, operand } => {
            let prefix = match op {
                ast::UnaryOp::Neg => "-",
                ast::UnaryOp::Not => "not ",
            };
            format!("{prefix}{}", expr_to_string(&operand.node))
        }
        ast::Expr::Call { func, args } => {
            let args_str: Vec<_> = args.iter().map(|a| expr_to_string(&a.node)).collect();
            format!("{}({})", expr_to_string(&func.node), args_str.join(", "))
        }
        ast::Expr::CallNamed { func, args } => {
            let args_str: Vec<_> = args
                .iter()
                .map(|(k, v)| format!("{k}: {}", expr_to_string(&v.node)))
                .collect();
            format!("{}({})", expr_to_string(&func.node), args_str.join(", "))
        }
        ast::Expr::Field { expr, name } => {
            format!("{}.{name}", expr_to_string(&expr.node))
        }
        ast::Expr::WithUnit { value, unit } => {
            format!("{}.{unit}", expr_to_string(&value.node))
        }
        _ => format!("{expr:?}"),
    }
}

fn binop_str(op: ast::BinOp) -> &'static str {
    match op {
        ast::BinOp::Add => "+",
        ast::BinOp::Sub => "-",
        ast::BinOp::Mul => "*",
        ast::BinOp::Div => "/",
        ast::BinOp::Mod => "%",
        ast::BinOp::Eq => "=",
        ast::BinOp::Neq => "!=",
        ast::BinOp::Lt => "<",
        ast::BinOp::Gt => ">",
        ast::BinOp::Leq => "<=",
        ast::BinOp::Geq => ">=",
        ast::BinOp::And => "and",
        ast::BinOp::Or => "or",
        ast::BinOp::Implies => "implies",
        ast::BinOp::In => "in",
        ast::BinOp::NotIn => "not_in",
        ast::BinOp::Assign => "=",
        ast::BinOp::Concat => "++",
    }
}
