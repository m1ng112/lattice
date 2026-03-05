//! Intent extraction from the Lattice AST.
//!
//! Walks a parsed [`Program`] and finds functions (and graph nodes)
//! whose body is `synthesize(...)`, then builds a [`SynthesisRequest`]
//! for each one.

use lattice_parser::ast::*;

use crate::types::{OptimizationTarget, SynthesisRequest, SynthesisStrategy};

/// Extract all synthesis requests from a parsed program.
pub fn extract_requests(program: &Program) -> Vec<SynthesisRequest> {
    let mut requests = Vec::new();
    for item in program {
        match &item.node {
            Item::Function(func) => {
                if let Some(req) = extract_from_function(func) {
                    requests.push(req);
                }
            }
            Item::Graph(graph) => {
                extract_from_graph(graph, &mut requests);
            }
            _ => {}
        }
    }
    requests
}

/// Extract a synthesis request from a function with `FunctionBody::Synthesize`.
fn extract_from_function(func: &Function) -> Option<SynthesisRequest> {
    let props = match &func.body {
        FunctionBody::Synthesize(props) => props,
        FunctionBody::Block(_) => return None,
    };

    let (strategy, optimize) = parse_synth_properties(props);

    Some(SynthesisRequest {
        function_name: func.name.clone(),
        parameters: func
            .params
            .iter()
            .map(|p| (p.name.clone(), format_type_expr(&p.type_expr.node)))
            .collect(),
        return_type: func
            .return_type
            .as_ref()
            .map(|t| format_type_expr(&t.node))
            .unwrap_or_else(|| "()".to_string()),
        preconditions: func.pre.iter().map(|e| format_expr(&e.node)).collect(),
        postconditions: func.post.iter().map(|e| format_expr(&e.node)).collect(),
        invariants: func.invariants.iter().map(|e| format_expr(&e.node)).collect(),
        strategy,
        optimize,
    })
}

/// Walk graph nodes looking for `synthesize(...)` expressions or solve blocks.
fn extract_from_graph(graph: &Graph, requests: &mut Vec<SynthesisRequest>) {
    for member in &graph.members {
        if let GraphMember::Node(node) = &member.node {
            let mut pre = Vec::new();
            let mut post = Vec::new();
            let mut has_synthesize = false;
            let mut strategy = None;
            let mut optimize = None;

            for field in &node.fields {
                match field {
                    NodeField::Pre(pres) => {
                        pre.extend(pres.iter().map(|e| format_expr(&e.node)));
                    }
                    NodeField::Post(posts) => {
                        post.extend(posts.iter().map(|e| format_expr(&e.node)));
                    }
                    NodeField::Solve(solve) => {
                        if let Some(strat_expr) = &solve.strategy {
                            if let Expr::Ident(s) = &strat_expr.node {
                                strategy = Some(SynthesisStrategy::from_ident(s));
                            }
                        }
                        // Check for synthesize expressions in constraints
                        for constraint in &solve.constraints {
                            if let Expr::Synthesize(props) = &constraint.node {
                                has_synthesize = true;
                                let (s, o) = parse_synth_properties(props);
                                if s.is_some() {
                                    strategy = s;
                                }
                                if o.is_some() {
                                    optimize = o;
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }

            if has_synthesize {
                requests.push(SynthesisRequest {
                    function_name: format!("{}::{}", graph.name, node.name),
                    parameters: vec![],
                    return_type: "()".to_string(),
                    preconditions: pre,
                    postconditions: post,
                    invariants: vec![],
                    strategy,
                    optimize,
                });
            }
        }
    }
}

/// Parse `strategy` and `optimize` from synthesize properties.
fn parse_synth_properties(
    props: &[Property],
) -> (Option<SynthesisStrategy>, Option<OptimizationTarget>) {
    let mut strategy = None;
    let mut optimize = None;

    for prop in props {
        match prop.key.as_str() {
            "strategy" => {
                if let Expr::Ident(s) = &prop.value.node {
                    strategy = Some(SynthesisStrategy::from_ident(s));
                }
            }
            "optimize" => {
                if let Expr::Ident(s) = &prop.value.node {
                    optimize = Some(OptimizationTarget::from_ident(s));
                }
            }
            _ => {}
        }
    }

    (strategy, optimize)
}

// ── Formatting helpers ──────────────────────────

/// Format a [`TypeExpr`] as a human-readable string.
pub(crate) fn format_type_expr(ty: &TypeExpr) -> String {
    match ty {
        TypeExpr::Named(name) => name.clone(),
        TypeExpr::Applied { name, args } => {
            let args_str: Vec<String> = args.iter().map(|a| format_type_expr(&a.node)).collect();
            format!("{}<{}>", name, args_str.join(", "))
        }
        TypeExpr::Function { params, ret } => {
            let params_str: Vec<String> =
                params.iter().map(|p| format_type_expr(&p.node)).collect();
            format!("({}) -> {}", params_str.join(", "), format_type_expr(&ret.node))
        }
        TypeExpr::Record(fields) => {
            let fields_str: Vec<String> = fields
                .iter()
                .map(|(name, ty)| format!("{}: {}", name, format_type_expr(&ty.node)))
                .collect();
            format!("{{ {} }}", fields_str.join(", "))
        }
        TypeExpr::Sum(variants) => {
            let variants_str: Vec<String> = variants
                .iter()
                .map(|v| {
                    if v.fields.is_empty() {
                        v.name.clone()
                    } else {
                        let fields: Vec<String> = v
                            .fields
                            .iter()
                            .map(|(n, t)| format!("{}: {}", n, format_type_expr(&t.node)))
                            .collect();
                        format!("{}({})", v.name, fields.join(", "))
                    }
                })
                .collect();
            variants_str.join(" | ")
        }
        TypeExpr::Refinement {
            var,
            base,
            predicate,
        } => {
            format!(
                "{{ {} in {} | {} }}",
                var,
                format_type_expr(&base.node),
                format_expr(&predicate.node)
            )
        }
        TypeExpr::Dependent { name, params } => {
            let params_str: Vec<String> = params
                .iter()
                .map(|(n, t)| format!("{}: {}", n, format_type_expr(&t.node)))
                .collect();
            format!("{}({})", name, params_str.join(", "))
        }
        TypeExpr::Stream(inner) => format!("Stream<{}>", format_type_expr(&inner.node)),
        TypeExpr::Distribution(inner) => {
            format!("Distribution<{}>", format_type_expr(&inner.node))
        }
        TypeExpr::Where { base, constraint } => {
            format!(
                "{} where {}",
                format_type_expr(&base.node),
                format_expr(&constraint.node)
            )
        }
    }
}

/// Format an [`Expr`] as a human-readable string.
pub(crate) fn format_expr(expr: &Expr) -> String {
    match expr {
        Expr::IntLit(n) => n.to_string(),
        Expr::FloatLit(f) => f.to_string(),
        Expr::StringLit(s) => format!("\"{}\"", s),
        Expr::BoolLit(b) => b.to_string(),
        Expr::Ident(name) => name.clone(),
        Expr::BinOp { left, op, right } => {
            format!(
                "{} {} {}",
                format_expr(&left.node),
                format_binop(op),
                format_expr(&right.node)
            )
        }
        Expr::UnaryOp { op, operand } => {
            let op_str = match op {
                UnaryOp::Neg => "-",
                UnaryOp::Not => "not ",
            };
            format!("{}{}", op_str, format_expr(&operand.node))
        }
        Expr::Call { func, args } => {
            let args_str: Vec<String> = args.iter().map(|a| format_expr(&a.node)).collect();
            format!("{}({})", format_expr(&func.node), args_str.join(", "))
        }
        Expr::CallNamed { func, args } => {
            let args_str: Vec<String> = args
                .iter()
                .map(|(k, v)| format!("{}: {}", k, format_expr(&v.node)))
                .collect();
            format!("{}({})", format_expr(&func.node), args_str.join(", "))
        }
        Expr::Field { expr, name } => {
            format!("{}.{}", format_expr(&expr.node), name)
        }
        Expr::Index { expr, index } => {
            format!("{}[{}]", format_expr(&expr.node), format_expr(&index.node))
        }
        Expr::Pipeline { left, right } => {
            format!("{} |> {}", format_expr(&left.node), format_expr(&right.node))
        }
        Expr::ForAll { var, domain, body } => {
            format!(
                "forall {} in {} . {}",
                var,
                format_expr(&domain.node),
                format_expr(&body.node)
            )
        }
        Expr::Exists { var, domain, body } => {
            format!(
                "exists {} in {} . {}",
                var,
                format_expr(&domain.node),
                format_expr(&body.node)
            )
        }
        Expr::If { cond, then_, else_ } => {
            let base = format!("if {} then {}", format_expr(&cond.node), format_expr(&then_.node));
            match else_ {
                Some(e) => format!("{} else {}", base, format_expr(&e.node)),
                None => base,
            }
        }
        Expr::Record(fields) => {
            let fields_str: Vec<String> = fields
                .iter()
                .map(|(k, v)| format!("{}: {}", k, format_expr(&v.node)))
                .collect();
            format!("{{ {} }}", fields_str.join(", "))
        }
        Expr::Array(elems) => {
            let elems_str: Vec<String> = elems.iter().map(|e| format_expr(&e.node)).collect();
            format!("[{}]", elems_str.join(", "))
        }
        Expr::WithUnit { value, unit } => {
            format!("{}.{}", format_expr(&value.node), unit)
        }
        Expr::Range { start, end } => {
            format!("{}..{}", format_expr(&start.node), format_expr(&end.node))
        }
        Expr::Ascription { expr, type_expr } => {
            format!("{}: {}", format_expr(&expr.node), format_type_expr(&type_expr.node))
        }
        Expr::Try(inner) => format!("{}?", format_expr(&inner.node)),
        // Fallback for complex expressions
        _ => "<expr>".to_string(),
    }
}

fn format_binop(op: &BinOp) -> &'static str {
    match op {
        BinOp::Add => "+",
        BinOp::Sub => "-",
        BinOp::Mul => "*",
        BinOp::Div => "/",
        BinOp::Mod => "%",
        BinOp::Eq => "=",
        BinOp::Neq => "!=",
        BinOp::Lt => "<",
        BinOp::Gt => ">",
        BinOp::Leq => "<=",
        BinOp::Geq => ">=",
        BinOp::And => "&&",
        BinOp::Or => "||",
        BinOp::Implies => "=>",
        BinOp::In => "in",
        BinOp::NotIn => "not in",
        BinOp::Assign => "=",
        BinOp::Concat => "++",
    }
}
