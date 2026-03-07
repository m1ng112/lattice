//! Pretty printer for Lattice AST → surface syntax.

use crate::ast::*;

/// Render a Lattice program AST back into surface syntax.
pub fn print_program(program: &Program) -> String {
    let mut p = Printer::new();
    p.print_program(program);
    p.output
}

struct Printer {
    output: String,
    indent: usize,
}

impl Printer {
    fn new() -> Self {
        Self {
            output: String::new(),
            indent: 0,
        }
    }

    fn write(&mut self, s: &str) {
        self.output.push_str(s);
    }

    fn writeln(&mut self, s: &str) {
        self.write_indent();
        self.output.push_str(s);
        self.output.push('\n');
    }

    fn write_indent(&mut self) {
        for _ in 0..self.indent {
            self.output.push_str("  ");
        }
    }

    fn newline(&mut self) {
        self.output.push('\n');
    }

    fn print_program(&mut self, program: &Program) {
        for (i, item) in program.iter().enumerate() {
            if i > 0 {
                self.newline();
            }
            self.print_item(&item.node);
        }
    }

    fn print_item(&mut self, item: &Item) {
        match item {
            Item::Graph(g) => self.print_graph(g),
            Item::TypeDef(td) => self.print_type_def(td),
            Item::Function(f) => self.print_function(f),
            Item::Module(m) => self.print_module(m),
            Item::Model(m) => self.print_model(m),
            Item::Meta(m) => self.print_meta(m),
            Item::LetBinding(lb) => self.print_let_binding(lb),
            Item::Import(imp) => self.print_import(imp),
        }
    }

    fn print_graph(&mut self, g: &Graph) {
        self.write_indent();
        self.write(&format!("graph {} {{\n", g.name));
        self.indent += 1;

        if let Some(v) = &g.version {
            self.writeln(&format!("version: \"{}\"", v));
        }
        if !g.targets.is_empty() {
            self.write_indent();
            self.write("target: [");
            for (i, t) in g.targets.iter().enumerate() {
                if i > 0 {
                    self.write(", ");
                }
                self.write(t);
            }
            self.write("]\n");
        }

        for member in &g.members {
            self.newline();
            match &member.node {
                GraphMember::Node(n) => self.print_node_def(n),
                GraphMember::Edge(e) => self.print_edge_def(e),
                GraphMember::Solve(s) => self.print_solve_block(s),
            }
        }

        self.indent -= 1;
        self.writeln("}");
    }

    fn print_node_def(&mut self, n: &NodeDef) {
        self.write_indent();
        self.write(&format!("node {} {{\n", n.name));
        self.indent += 1;

        for field in &n.fields {
            match field {
                NodeField::Input(ty) => {
                    self.write_indent();
                    self.write("input: ");
                    self.print_type_expr(&ty.node);
                    self.newline();
                }
                NodeField::Output(ty) => {
                    self.write_indent();
                    self.write("output: ");
                    self.print_type_expr(&ty.node);
                    self.newline();
                }
                NodeField::Properties(props) => {
                    self.print_properties_block(props);
                }
                NodeField::Semantic(sem) => {
                    self.print_semantic_block(sem);
                }
                NodeField::ProofObligations(proofs) => {
                    self.writeln("proof_obligations: {");
                    self.indent += 1;
                    for po in proofs {
                        self.write_indent();
                        self.write(&format!("{}: ", po.name));
                        self.print_expr(&po.expr.node);
                        self.newline();
                    }
                    self.indent -= 1;
                    self.writeln("}");
                }
                NodeField::Pre(exprs) => self.print_condition_block("pre", exprs),
                NodeField::Post(exprs) => self.print_condition_block("post", exprs),
                NodeField::Solve(s) => self.print_solve_block(s),
            }
        }

        self.indent -= 1;
        self.writeln("}");
    }

    fn print_edge_def(&mut self, e: &EdgeDef) {
        self.write_indent();
        self.write(&format!("edge {} -> {}", e.from, e.to));
        if !e.properties.is_empty() {
            self.write(" {\n");
            self.indent += 1;
            for prop in &e.properties {
                self.write_indent();
                self.write(&format!("{}: ", prop.key));
                self.print_expr(&prop.value.node);
                self.newline();
            }
            self.indent -= 1;
            self.writeln("}");
        } else {
            self.newline();
        }
    }

    fn print_solve_block(&mut self, s: &SolveBlock) {
        self.writeln("solve {");
        self.indent += 1;
        if let Some(goal) = &s.goal {
            self.write_indent();
            self.write("goal: ");
            self.print_expr(&goal.node);
            self.newline();
        }
        for c in &s.constraints {
            self.write_indent();
            self.write("constraint: ");
            self.print_expr(&c.node);
            self.newline();
        }
        for inv in &s.invariants {
            self.write_indent();
            self.write("invariant: ");
            self.print_expr(&inv.node);
            self.newline();
        }
        if let Some(d) = &s.domain {
            self.write_indent();
            self.write(&format!("domain: {}", d.kind));
            if !d.config.is_empty() {
                self.write("(");
                for (i, c) in d.config.iter().enumerate() {
                    if i > 0 {
                        self.write(", ");
                    }
                    self.write(&format!("{}: ", c.key));
                    self.print_expr(&c.value.node);
                }
                self.write(")");
            }
            self.newline();
        }
        if let Some(st) = &s.strategy {
            self.write_indent();
            self.write("strategy: ");
            self.print_expr(&st.node);
            self.newline();
        }
        self.indent -= 1;
        self.writeln("}");
    }

    fn print_function(&mut self, f: &Function) {
        self.write_indent();
        self.write(&format!("function {}(", f.name));
        for (i, p) in f.params.iter().enumerate() {
            if i > 0 {
                self.write(", ");
            }
            self.write(&format!("{}: ", p.name));
            self.print_type_expr(&p.type_expr.node);
        }
        self.write(")");
        if let Some(rt) = &f.return_type {
            self.write(" -> ");
            self.print_type_expr(&rt.node);
        }
        self.write(" {\n");
        self.indent += 1;

        if !f.pre.is_empty() {
            self.print_condition_block("pre", &f.pre);
        }
        if !f.post.is_empty() {
            self.print_condition_block("post", &f.post);
        }
        if !f.invariants.is_empty() {
            self.print_condition_block("invariant", &f.invariants);
        }

        match &f.body {
            FunctionBody::Block(exprs) => {
                for expr in exprs {
                    self.write_indent();
                    self.print_expr(&expr.node);
                    self.newline();
                }
            }
            FunctionBody::Synthesize(args) => {
                self.write_indent();
                self.write("synthesize(");
                for (i, arg) in args.iter().enumerate() {
                    if i > 0 {
                        self.write(", ");
                    }
                    self.write(&format!("{}: ", arg.key));
                    self.print_expr(&arg.value.node);
                }
                self.write(")\n");
            }
        }

        self.indent -= 1;
        self.writeln("}");
    }

    fn print_type_def(&mut self, td: &TypeDef) {
        self.write_indent();
        self.write(&format!("type {}", td.name));
        if !td.params.is_empty() {
            self.write("<");
            for (i, tp) in td.params.iter().enumerate() {
                if i > 0 {
                    self.write(", ");
                }
                self.write(&tp.name);
                if let Some(b) = &tp.bound {
                    self.write(": ");
                    self.print_type_expr(&b.node);
                }
            }
            self.write(">");
        }
        self.write(" = ");
        self.print_type_expr(&td.body.node);
        self.newline();
    }

    fn print_module(&mut self, m: &Module) {
        self.write_indent();
        self.write(&format!("module {} {{\n", m.name));
        self.indent += 1;
        for item in &m.items {
            self.print_item(&item.node);
        }
        self.indent -= 1;
        self.writeln("}");
    }

    fn print_model(&mut self, m: &Model) {
        self.write_indent();
        self.write(&format!("model {} {{\n", m.name));
        self.indent += 1;
        for stmt in &m.statements {
            match &stmt.node {
                ModelStatement::Prior { name, distribution } => {
                    self.write_indent();
                    self.write(&format!("prior {}: ", name));
                    self.print_expr(&distribution.node);
                    self.newline();
                }
                ModelStatement::Observe { name, distribution } => {
                    self.write_indent();
                    self.write(&format!("observe {} ~ ", name));
                    self.print_expr(&distribution.node);
                    self.newline();
                }
                ModelStatement::Posterior(expr) => {
                    self.write_indent();
                    self.write("posterior = ");
                    self.print_expr(&expr.node);
                    self.newline();
                }
            }
        }
        self.indent -= 1;
        self.writeln("}");
    }

    fn print_meta(&mut self, m: &Meta) {
        self.write_indent();
        self.write(&format!("meta {}(", m.name));
        self.print_expr(&m.target.node);
        self.write(") {\n");
        self.indent += 1;
        for field in &m.body {
            self.write_indent();
            self.write(&format!("{}: ", field.key));
            self.print_expr(&field.value.node);
            self.newline();
        }
        self.indent -= 1;
        self.writeln("}");
    }

    fn print_import(&mut self, imp: &Import) {
        self.write_indent();
        self.write("import ");
        self.write(&imp.path.join("."));
        if let Some(names) = &imp.names {
            self.write(".{");
            for (i, n) in names.iter().enumerate() {
                if i > 0 {
                    self.write(", ");
                }
                self.write(&n.name);
                if let Some(alias) = &n.alias {
                    self.write(&format!(" as {}", alias));
                }
            }
            self.write("}");
        }
        self.newline();
    }

    fn print_let_binding(&mut self, lb: &LetBinding) {
        self.write_indent();
        self.write(&format!("let {}", lb.name));
        if let Some(ty) = &lb.type_ann {
            self.write(": ");
            self.print_type_expr(&ty.node);
        }
        self.write(" = ");
        self.print_expr(&lb.value.node);
        self.newline();
    }

    fn print_properties_block(&mut self, props: &[Property]) {
        self.writeln("properties: {");
        self.indent += 1;
        for prop in props {
            self.write_indent();
            self.write(&format!("{}: ", prop.key));
            self.print_expr(&prop.value.node);
            self.newline();
        }
        self.indent -= 1;
        self.writeln("}");
    }

    fn print_semantic_block(&mut self, sem: &SemanticBlock) {
        self.writeln("semantic: {");
        self.indent += 1;
        if let Some(desc) = &sem.description {
            self.writeln(&format!("description: \"{}\"", desc));
        }
        if let Some(formal) = &sem.formal {
            self.write_indent();
            self.write("formal: ");
            self.print_expr(&formal.node);
            self.newline();
        }
        self.indent -= 1;
        self.writeln("}");
    }

    fn print_condition_block(&mut self, label: &str, exprs: &[Spanned<Expr>]) {
        self.writeln(&format!("{}: {{", label));
        self.indent += 1;
        for e in exprs {
            self.write_indent();
            self.print_expr(&e.node);
            self.newline();
        }
        self.indent -= 1;
        self.writeln("}");
    }

    fn print_type_expr(&mut self, te: &TypeExpr) {
        match te {
            TypeExpr::Named(name) => self.write(name),
            TypeExpr::Applied { name, args } => {
                self.write(name);
                self.write("<");
                for (i, arg) in args.iter().enumerate() {
                    if i > 0 {
                        self.write(", ");
                    }
                    self.print_type_expr(&arg.node);
                }
                self.write(">");
            }
            TypeExpr::Function { params, ret } => {
                for (i, p) in params.iter().enumerate() {
                    if i > 0 {
                        self.write(", ");
                    }
                    self.print_type_expr(&p.node);
                }
                self.write(" -> ");
                self.print_type_expr(&ret.node);
            }
            TypeExpr::Record(fields) => {
                self.write("{ ");
                for (i, (name, ty)) in fields.iter().enumerate() {
                    if i > 0 {
                        self.write(", ");
                    }
                    self.write(&format!("{}: ", name));
                    self.print_type_expr(&ty.node);
                }
                self.write(" }");
            }
            TypeExpr::Sum(variants) => {
                for (i, v) in variants.iter().enumerate() {
                    if i > 0 {
                        self.write(" | ");
                    }
                    self.write(&v.name);
                    if !v.fields.is_empty() {
                        self.write("(");
                        for (j, (fname, fty)) in v.fields.iter().enumerate() {
                            if j > 0 {
                                self.write(", ");
                            }
                            self.write(&format!("{}: ", fname));
                            self.print_type_expr(&fty.node);
                        }
                        self.write(")");
                    }
                }
            }
            TypeExpr::Refinement {
                var,
                base,
                predicate,
            } => {
                self.write(&format!("{{ {} in ", var));
                self.print_type_expr(&base.node);
                self.write(" | ");
                self.print_expr(&predicate.node);
                self.write(" }");
            }
            TypeExpr::Dependent { name, params } => {
                self.write(name);
                self.write("(");
                for (i, (pname, pty)) in params.iter().enumerate() {
                    if i > 0 {
                        self.write(", ");
                    }
                    self.write(&format!("{}: ", pname));
                    self.print_type_expr(&pty.node);
                }
                self.write(")");
            }
            TypeExpr::Stream(inner) => {
                self.write("Stream<");
                self.print_type_expr(&inner.node);
                self.write(">");
            }
            TypeExpr::Distribution(inner) => {
                self.write("Distribution<");
                self.print_type_expr(&inner.node);
                self.write(">");
            }
            TypeExpr::Where { base, constraint } => {
                self.print_type_expr(&base.node);
                self.write(" where ");
                self.print_expr(&constraint.node);
            }
        }
    }

    fn print_expr(&mut self, expr: &Expr) {
        match expr {
            Expr::IntLit(n) => self.write(&n.to_string()),
            Expr::FloatLit(n) => {
                if *n == (*n as i64) as f64 && !n.to_string().contains('.') {
                    self.write(&format!("{}.0", n));
                } else {
                    self.write(&n.to_string());
                }
            }
            Expr::StringLit(s) => self.write(&format!("\"{}\"", s)),
            Expr::BoolLit(b) => self.write(if *b { "true" } else { "false" }),
            Expr::Ident(s) => self.write(s),
            Expr::BinOp { left, op, right } => {
                self.print_expr(&left.node);
                self.write(&format!(" {} ", op_str(*op)));
                self.print_expr(&right.node);
            }
            Expr::UnaryOp { op, operand } => {
                self.write(match op {
                    UnaryOp::Neg => "-",
                    UnaryOp::Not => "not ",
                });
                self.print_expr(&operand.node);
            }
            Expr::Call { func, args } => {
                self.print_expr(&func.node);
                self.write("(");
                for (i, arg) in args.iter().enumerate() {
                    if i > 0 {
                        self.write(", ");
                    }
                    self.print_expr(&arg.node);
                }
                self.write(")");
            }
            Expr::CallNamed { func, args } => {
                self.print_expr(&func.node);
                self.write("(");
                for (i, (name, val)) in args.iter().enumerate() {
                    if i > 0 {
                        self.write(", ");
                    }
                    self.write(&format!("{}: ", name));
                    self.print_expr(&val.node);
                }
                self.write(")");
            }
            Expr::Field { expr, name } => {
                self.print_expr(&expr.node);
                self.write(&format!(".{}", name));
            }
            Expr::Index { expr, index } => {
                self.print_expr(&expr.node);
                self.write("[");
                self.print_expr(&index.node);
                self.write("]");
            }
            Expr::Slice { expr, start, end } => {
                self.print_expr(&expr.node);
                self.write("[");
                if let Some(s) = start {
                    self.print_expr(&s.node);
                }
                self.write(":");
                if let Some(e) = end {
                    self.print_expr(&e.node);
                }
                self.write("]");
            }
            Expr::Pipeline { left, right } => {
                self.print_expr(&left.node);
                self.newline();
                self.write_indent();
                self.write("|> ");
                self.print_expr(&right.node);
            }
            Expr::Lambda { params, body } => {
                if params.len() == 1 {
                    self.write(&format!("fn {}", params[0].name));
                    self.write(": ");
                    self.print_type_expr(&params[0].type_expr.node);
                    self.write(" -> ");
                } else {
                    self.write("fn(");
                    for (i, p) in params.iter().enumerate() {
                        if i > 0 {
                            self.write(", ");
                        }
                        self.write(&format!("{}: ", p.name));
                        self.print_type_expr(&p.type_expr.node);
                    }
                    self.write(") -> ");
                }
                self.print_expr(&body.node);
            }
            Expr::Let { name, type_ann, value } => {
                self.write(&format!("let {}", name));
                if let Some(ty) = type_ann {
                    self.write(": ");
                    self.print_type_expr(&ty.node);
                }
                self.write(" = ");
                self.print_expr(&value.node);
            }
            Expr::Record(pairs) => {
                self.write("{");
                for (i, (key, val)) in pairs.iter().enumerate() {
                    if i > 0 {
                        self.write(", ");
                    }
                    self.write(&format!("{}: ", key));
                    self.print_expr(&val.node);
                }
                self.write("}");
            }
            Expr::Array(items) => {
                self.write("[");
                for (i, item) in items.iter().enumerate() {
                    if i > 0 {
                        self.write(", ");
                    }
                    self.print_expr(&item.node);
                }
                self.write("]");
            }
            Expr::WithUnit { value, unit } => {
                self.print_expr(&value.node);
                self.write(&format!(".{}", unit));
            }
            Expr::Select { predicate, relation } => {
                self.write("sigma(");
                self.print_expr(&predicate.node);
                self.write(")(");
                self.print_expr(&relation.node);
                self.write(")");
            }
            Expr::Project { fields, relation } => {
                self.write("project[");
                self.write(&fields.join(", "));
                self.write("](");
                self.print_expr(&relation.node);
                self.write(")");
            }
            Expr::Join {
                left,
                condition,
                right,
            } => {
                self.print_expr(&left.node);
                self.write(" join(");
                self.print_expr(&condition.node);
                self.write(") ");
                self.print_expr(&right.node);
            }
            Expr::GroupBy {
                keys,
                aggregates,
                relation,
            } => {
                self.write("group_by[");
                self.write(&keys.join(", "));
                if !aggregates.is_empty() {
                    self.write("; ");
                    for (i, a) in aggregates.iter().enumerate() {
                        if i > 0 {
                            self.write(", ");
                        }
                        self.print_expr(&a.node);
                    }
                }
                self.write("](");
                self.print_expr(&relation.node);
                self.write(")");
            }
            Expr::DoBlock(stmts) => {
                self.write("do {\n");
                self.indent += 1;
                for stmt in stmts {
                    self.write_indent();
                    match &stmt.node {
                        DoStatement::Bind { name, expr } => {
                            self.write(&format!("{} <- ", name));
                            self.print_expr(&expr.node);
                        }
                        DoStatement::Let { name, expr } => {
                            self.write(&format!("let {} = ", name));
                            self.print_expr(&expr.node);
                        }
                        DoStatement::Yield(e) => {
                            self.write("yield ");
                            self.print_expr(&e.node);
                        }
                        DoStatement::Expr(e) => {
                            self.print_expr(&e.node);
                        }
                    }
                    self.newline();
                }
                self.indent -= 1;
                self.write_indent();
                self.write("}");
            }
            Expr::ForAll { var, domain, body } => {
                self.write(&format!("forall {} in ", var));
                self.print_expr(&domain.node);
                self.write(" -> ");
                self.print_expr(&body.node);
            }
            Expr::Exists { var, domain, body } => {
                self.write(&format!("exists {} in ", var));
                self.print_expr(&domain.node);
                self.write(" -> ");
                self.print_expr(&body.node);
            }
            Expr::Branch { expr, arms } => {
                self.write("branch ");
                self.print_expr(&expr.node);
                self.write(" {\n");
                self.indent += 1;
                for arm in arms {
                    self.write_indent();
                    self.print_pattern(&arm.pattern.node);
                    if let Some(g) = &arm.guard {
                        self.write("(");
                        self.print_expr(&g.node);
                        self.write(")");
                    }
                    self.write(" -> ");
                    self.print_expr(&arm.body.node);
                    self.newline();
                }
                self.indent -= 1;
                self.write_indent();
                self.write("}");
            }
            Expr::Match { expr, arms } => {
                self.write("match ");
                self.print_expr(&expr.node);
                self.write(" {\n");
                self.indent += 1;
                for arm in arms {
                    self.write_indent();
                    self.print_pattern(&arm.pattern.node);
                    if let Some(g) = &arm.guard {
                        self.write("(");
                        self.print_expr(&g.node);
                        self.write(")");
                    }
                    self.write(" -> ");
                    self.print_expr(&arm.body.node);
                    self.newline();
                }
                self.indent -= 1;
                self.write_indent();
                self.write("}");
            }
            Expr::If { cond, then_, else_ } => {
                self.write("if ");
                self.print_expr(&cond.node);
                self.write(" then ");
                self.print_expr(&then_.node);
                if let Some(e) = else_ {
                    self.write(" else ");
                    self.print_expr(&e.node);
                }
            }
            Expr::Block(exprs) => {
                self.write("{\n");
                self.indent += 1;
                for e in exprs {
                    self.write_indent();
                    self.print_expr(&e.node);
                    self.newline();
                }
                self.indent -= 1;
                self.write_indent();
                self.write("}");
            }
            Expr::Synthesize(args) => {
                self.write("synthesize(");
                for (i, arg) in args.iter().enumerate() {
                    if i > 0 {
                        self.write(", ");
                    }
                    self.write(&format!("{}: ", arg.key));
                    self.print_expr(&arg.value.node);
                }
                self.write(")");
            }
            Expr::Ascription { expr, type_expr } => {
                self.print_expr(&expr.node);
                self.write(": ");
                self.print_type_expr(&type_expr.node);
            }
            Expr::Range { start, end } => {
                self.print_expr(&start.node);
                self.write("..");
                self.print_expr(&end.node);
            }
            Expr::Try(inner) => {
                self.print_expr(&inner.node);
                self.write("?");
            }
            Expr::Yield(inner) => {
                self.write("yield ");
                self.print_expr(&inner.node);
            }
        }
    }

    fn print_pattern(&mut self, pat: &Pattern) {
        match pat {
            Pattern::Wildcard => self.write("_"),
            Pattern::Ident(s) => self.write(s),
            Pattern::Constructor(name, fields) => {
                self.write(name);
                if !fields.is_empty() {
                    self.write("(");
                    for (i, f) in fields.iter().enumerate() {
                        if i > 0 {
                            self.write(", ");
                        }
                        self.print_pattern(&f.node);
                    }
                    self.write(")");
                }
            }
            Pattern::Literal(e) => self.print_expr(&e.node),
            Pattern::Record(fields) => {
                self.write("{ ");
                for (i, (name, pat)) in fields.iter().enumerate() {
                    if i > 0 {
                        self.write(", ");
                    }
                    self.write(&format!("{}: ", name));
                    self.print_pattern(&pat.node);
                }
                self.write(" }");
            }
        }
    }
}

fn op_str(op: BinOp) -> &'static str {
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
        BinOp::And => "and",
        BinOp::Or => "or",
        BinOp::Implies => "implies",
        BinOp::In => "in",
        BinOp::NotIn => "not_in",
        BinOp::Assign => "=",
        BinOp::Concat => "++",
    }
}
