//! Documentation generator for Lattice source files.
//!
//! Produces Markdown documentation from parsed Lattice AST.

use lattice_parser::ast::*;

/// Generate Markdown documentation for a parsed Lattice program.
pub fn generate_docs(program: &Program, filename: &str) -> String {
    let mut out = String::new();

    out.push_str(&format!("# {}\n\n", filename));

    let mut has_types = false;
    let mut has_functions = false;
    let mut has_graphs = false;

    // Check what sections we need
    for item in program {
        match &item.node {
            Item::TypeDef(_) => has_types = true,
            Item::Function(_) => has_functions = true,
            Item::Graph(_) => has_graphs = true,
            _ => {}
        }
    }

    if has_types {
        out.push_str("## Types\n\n");
        for item in program {
            if let Item::TypeDef(td) = &item.node {
                write_type_def(&mut out, td);
            }
        }
    }

    if has_functions {
        out.push_str("## Functions\n\n");
        for item in program {
            if let Item::Function(f) = &item.node {
                write_function(&mut out, f);
            }
        }
    }

    if has_graphs {
        out.push_str("## Graphs\n\n");
        for item in program {
            if let Item::Graph(g) = &item.node {
                write_graph(&mut out, g);
            }
        }
    }

    out
}

fn write_type_def(out: &mut String, td: &TypeDef) {
    out.push_str(&format!("### `{}", td.name));
    if !td.params.is_empty() {
        out.push('<');
        for (i, p) in td.params.iter().enumerate() {
            if i > 0 {
                out.push_str(", ");
            }
            out.push_str(&p.name);
        }
        out.push('>');
    }
    out.push_str("`\n\n");

    match &td.body.node {
        TypeExpr::Sum(variants) => {
            out.push_str("**Variants:**\n\n");
            for v in variants {
                if v.fields.is_empty() {
                    out.push_str(&format!("- `{}`\n", v.name));
                } else {
                    let fields: Vec<String> = v
                        .fields
                        .iter()
                        .map(|(_, ty)| format_type_expr(&ty.node))
                        .collect();
                    out.push_str(&format!("- `{}<{}>`\n", v.name, fields.join(", ")));
                }
            }
            out.push('\n');
        }
        TypeExpr::Refinement {
            var,
            base,
            predicate,
        } => {
            out.push_str(&format!(
                "**Refinement:** `{{ {} in {} | {} }}`\n\n",
                var,
                format_type_expr(&base.node),
                format_expr(&predicate.node),
            ));
        }
        TypeExpr::Record(fields) => {
            out.push_str("**Fields:**\n\n");
            for (name, ty) in fields {
                out.push_str(&format!("- `{}`: `{}`\n", name, format_type_expr(&ty.node)));
            }
            out.push('\n');
        }
        other => {
            out.push_str(&format!("**Definition:** `{}`\n\n", format_type_expr(other)));
        }
    }
}

fn write_function(out: &mut String, f: &Function) {
    // Signature line
    let params: Vec<String> = f
        .params
        .iter()
        .map(|p| format!("{}: {}", p.name, format_type_expr(&p.type_expr.node)))
        .collect();

    let ret = f
        .return_type
        .as_ref()
        .map(|t| format!(" -> {}", format_type_expr(&t.node)))
        .unwrap_or_default();

    out.push_str(&format!("### `{}`\n\n", f.name));
    out.push_str(&format!(
        "```\nfunction {}({}){}\n```\n\n",
        f.name,
        params.join(", "),
        ret,
    ));

    if !f.pre.is_empty() {
        out.push_str("**Preconditions:**\n\n");
        for pre in &f.pre {
            out.push_str(&format!("- `{}`\n", format_expr(&pre.node)));
        }
        out.push('\n');
    }

    if !f.post.is_empty() {
        out.push_str("**Postconditions:**\n\n");
        for post in &f.post {
            out.push_str(&format!("- `{}`\n", format_expr(&post.node)));
        }
        out.push('\n');
    }

    if let FunctionBody::Synthesize(props) = &f.body {
        let strategy = props
            .iter()
            .find(|p| p.key == "strategy")
            .map(|p| format_expr(&p.value.node));
        if let Some(s) = strategy {
            out.push_str(&format!("**Synthesis strategy:** `{}`\n\n", s));
        }
    }
}

fn write_graph(out: &mut String, g: &Graph) {
    out.push_str(&format!("### `{}`\n\n", g.name));

    if let Some(v) = &g.version {
        out.push_str(&format!("**Version:** {}\n\n", v));
    }

    let nodes: Vec<&NodeDef> = g
        .members
        .iter()
        .filter_map(|m| match &m.node {
            GraphMember::Node(n) => Some(n),
            _ => None,
        })
        .collect();

    let edges: Vec<&EdgeDef> = g
        .members
        .iter()
        .filter_map(|m| match &m.node {
            GraphMember::Edge(e) => Some(e),
            _ => None,
        })
        .collect();

    let solves: Vec<&SolveBlock> = g
        .members
        .iter()
        .filter_map(|m| match &m.node {
            GraphMember::Solve(s) => Some(s),
            _ => None,
        })
        .collect();

    if !nodes.is_empty() {
        out.push_str("**Nodes:**\n\n");
        for n in &nodes {
            out.push_str(&format!("- `{}`\n", n.name));
        }
        out.push('\n');
    }

    if !edges.is_empty() {
        out.push_str("**Edges:**\n\n");
        for e in &edges {
            out.push_str(&format!("- `{}` -> `{}`\n", e.from, e.to));
        }
        out.push('\n');
    }

    if !solves.is_empty() {
        out.push_str("**Solve blocks:**\n\n");
        for s in &solves {
            if let Some(goal) = &s.goal {
                out.push_str(&format!("- Goal: `{}`\n", format_expr(&goal.node)));
            }
            for c in &s.constraints {
                out.push_str(&format!("- Constraint: `{}`\n", format_expr(&c.node)));
            }
        }
        out.push('\n');
    }
}

/// Format a type expression to a string.
fn format_type_expr(te: &TypeExpr) -> String {
    match te {
        TypeExpr::Named(name) => name.clone(),
        TypeExpr::Applied { name, args } => {
            let args_str: Vec<String> = args.iter().map(|a| format_type_expr(&a.node)).collect();
            format!("{}<{}>", name, args_str.join(", "))
        }
        TypeExpr::Function { params, ret } => {
            let params_str: Vec<String> =
                params.iter().map(|p| format_type_expr(&p.node)).collect();
            format!("{} -> {}", params_str.join(", "), format_type_expr(&ret.node))
        }
        TypeExpr::Record(fields) => {
            let fields_str: Vec<String> = fields
                .iter()
                .map(|(name, ty)| format!("{}: {}", name, format_type_expr(&ty.node)))
                .collect();
            format!("{{ {} }}", fields_str.join(", "))
        }
        TypeExpr::Sum(variants) => {
            let vs: Vec<String> = variants
                .iter()
                .map(|v| {
                    if v.fields.is_empty() {
                        v.name.clone()
                    } else {
                        let fs: Vec<String> = v
                            .fields
                            .iter()
                            .map(|(_, ty)| format_type_expr(&ty.node))
                            .collect();
                        format!("{}<{}>", v.name, fs.join(", "))
                    }
                })
                .collect();
            vs.join(" | ")
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
                format_expr(&predicate.node),
            )
        }
        TypeExpr::Dependent { name, params } => {
            let ps: Vec<String> = params
                .iter()
                .map(|(n, ty)| format!("{}: {}", n, format_type_expr(&ty.node)))
                .collect();
            format!("{}({})", name, ps.join(", "))
        }
        TypeExpr::Stream(inner) => format!("Stream<{}>", format_type_expr(&inner.node)),
        TypeExpr::Distribution(inner) => {
            format!("Distribution<{}>", format_type_expr(&inner.node))
        }
        TypeExpr::Where { base, constraint } => {
            format!(
                "{} where {}",
                format_type_expr(&base.node),
                format_expr(&constraint.node),
            )
        }
    }
}

/// Format an expression to a string (simplified).
fn format_expr(expr: &Expr) -> String {
    match expr {
        Expr::IntLit(n) => n.to_string(),
        Expr::FloatLit(n) => {
            let s = n.to_string();
            if s.contains('.') {
                s
            } else {
                format!("{}.0", s)
            }
        }
        Expr::StringLit(s) => format!("\"{}\"", s),
        Expr::BoolLit(b) => b.to_string(),
        Expr::Ident(name) => name.clone(),
        Expr::BinOp { left, op, right } => {
            let op_str = match op {
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
                BinOp::And => "and",
                BinOp::Or => "or",
                BinOp::Implies => "implies",
                BinOp::In => "in",
                BinOp::NotIn => "not_in",
                BinOp::Assign => "=",
                BinOp::Concat => "++",
            };
            format!(
                "{} {} {}",
                format_expr(&left.node),
                op_str,
                format_expr(&right.node),
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
        Expr::Field { expr, name } => {
            format!("{}.{}", format_expr(&expr.node), name)
        }
        _ => "...".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lattice_parser::parser;

    #[test]
    fn test_parse_stdlib_core() {
        let src = include_str!("../../../stdlib/core.lattice");
        let program = parser::parse(src).expect("core.lattice should parse");
        assert!(program.len() >= 3, "core.lattice should have at least 3 items");
    }

    #[test]
    fn test_parse_stdlib_stream() {
        let src = include_str!("../../../stdlib/stream.lattice");
        let program = parser::parse(src).expect("stream.lattice should parse");
        assert!(program.len() >= 3, "stream.lattice should have at least 3 items");
    }

    #[test]
    fn test_parse_stdlib_math() {
        let src = include_str!("../../../stdlib/math.lattice");
        let program = parser::parse(src).expect("math.lattice should parse");
        assert!(program.len() >= 4, "math.lattice should have at least 4 items");
    }

    #[test]
    fn test_parse_stdlib_probability() {
        let src = include_str!("../../../stdlib/probability.lattice");
        let program = parser::parse(src).expect("probability.lattice should parse");
        assert!(program.len() >= 2, "probability.lattice should have at least 2 items");
    }

    #[test]
    fn test_docgen_functions_with_pre_post() {
        let src = r#"
function gcd(a: Int, b: Int) -> Int {
  pre: {
    a > 0
    b > 0
  }
  post: { result > 0 }
  synthesize(strategy: recursive)
}
"#;
        let program = parser::parse(src).unwrap();
        let doc = generate_docs(&program, "math.lattice");

        assert!(doc.contains("# math.lattice"), "should have file header");
        assert!(doc.contains("## Functions"), "should have functions section");
        assert!(doc.contains("### `gcd`"), "should have function name");
        assert!(doc.contains("a: Int"), "should have param types");
        assert!(doc.contains("-> Int"), "should have return type");
        assert!(doc.contains("**Preconditions:**"), "should have preconditions");
        assert!(doc.contains("a > 0"), "should have pre content");
        assert!(doc.contains("**Postconditions:**"), "should have postconditions");
        assert!(doc.contains("result > 0"), "should have post content");
        assert!(doc.contains("**Synthesis strategy:** `recursive`"), "should have strategy");
    }

    #[test]
    fn test_docgen_type_definitions() {
        let src = r#"
type Option<T> = Some<T> | None

type NonEmpty<T> = { xs ∈ List<T> | length(xs) > 0 }
"#;
        let program = parser::parse(src).unwrap();
        let doc = generate_docs(&program, "core.lattice");

        assert!(doc.contains("## Types"), "should have types section");
        assert!(doc.contains("### `Option<T>`"), "should have Option type");
        assert!(doc.contains("**Variants:**"), "should list variants");
        assert!(doc.contains("`None`"), "should have None variant");
        assert!(doc.contains("`Some"), "should have Some variant");
        assert!(doc.contains("### `NonEmpty<T>`"), "should have NonEmpty type");
        assert!(doc.contains("**Refinement:**"), "should have refinement");
    }

    #[test]
    fn test_docgen_graph() {
        let src = r#"
graph Calculator {
  version: "1.0.0"

  node Input {
    output: Int
  }

  node Double {
    input: Int
    output: Int
  }

  edge Input -> Double {}
}
"#;
        let program = parser::parse(src).unwrap();
        let doc = generate_docs(&program, "calculator.lattice");

        assert!(doc.contains("## Graphs"), "should have graphs section");
        assert!(doc.contains("### `Calculator`"), "should have graph name");
        assert!(doc.contains("**Version:** 1.0.0"), "should have version");
        assert!(doc.contains("**Nodes:**"), "should list nodes");
        assert!(doc.contains("`Input`"), "should have Input node");
        assert!(doc.contains("`Double`"), "should have Double node");
        assert!(doc.contains("**Edges:**"), "should list edges");
        assert!(doc.contains("`Input` -> `Double`"), "should have edge");
    }

    #[test]
    fn test_docgen_full_stdlib_core() {
        let src = include_str!("../../../stdlib/core.lattice");
        let program = parser::parse(src).unwrap();
        let doc = generate_docs(&program, "core.lattice");

        assert!(doc.contains("## Types"), "should have types section");
        assert!(doc.contains("## Functions"), "should have functions section");
        assert!(doc.contains("### `Option<T>`"), "should document Option");
        assert!(doc.contains("### `Result<T, E>`"), "should document Result");
        assert!(doc.contains("### `unwrap_or`"), "should document unwrap_or");
        assert!(doc.contains("### `map_option`"), "should document map_option");
    }
}
