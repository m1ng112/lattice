use lattice_codegen::compiler::Compiler;
use lattice_codegen::interpreter::Interpreter;
use lattice_parser::ast;
use lattice_parser::parser;
use lattice_runtime::node::Value;
use lattice_types::checker::TypeChecker;

/// Accumulated REPL context (definitions, loaded files).
pub struct ReplContext {
    pub source: String,
    pub loaded_files: Vec<String>,
}

/// Result of evaluating a single REPL line.
#[derive(Debug, PartialEq)]
pub enum ReplResult {
    Value(String),
    TypeInfo(String),
    ProofResult(String),
    Loaded(String),
    Help(String),
    Empty,
    Quit,
    Error(String),
}

/// The Lattice interactive REPL.
pub struct Repl {
    type_checker: TypeChecker,
    context: ReplContext,
    /// Persistent interpreter that retains variable bindings across lines.
    interpreter: Interpreter,
    /// Pending multi-line input being accumulated.
    pending: Option<String>,
}

impl Default for Repl {
    fn default() -> Self {
        Self::new()
    }
}

impl Repl {
    pub fn new() -> Self {
        let mut interpreter = Interpreter::new();
        interpreter.register_stdlib();
        Self {
            type_checker: TypeChecker::new(),
            context: ReplContext {
                source: String::new(),
                loaded_files: Vec::new(),
            },
            interpreter,
            pending: None,
        }
    }

    /// Returns true if the REPL is currently accumulating a multi-line input.
    pub fn is_multiline(&self) -> bool {
        self.pending.is_some()
    }

    /// Evaluate a single line of user input (or accumulate for multi-line).
    pub fn eval_line(&mut self, input: &str) -> ReplResult {
        let trimmed = input.trim();

        // Handle multi-line continuation
        if let Some(ref mut pending) = self.pending {
            pending.push('\n');
            pending.push_str(input);
            if is_balanced(pending) {
                let full = self.pending.take().unwrap();
                return self.eval_complete(&full);
            }
            return ReplResult::Empty;
        }

        if trimmed.is_empty() {
            return ReplResult::Empty;
        }

        // Check for multi-line start
        if needs_continuation(trimmed) {
            self.pending = Some(input.to_string());
            return ReplResult::Empty;
        }

        self.eval_complete(trimmed)
    }

    fn eval_complete(&mut self, input: &str) -> ReplResult {
        let trimmed = input.trim();

        // Commands
        if let Some(rest) = trimmed.strip_prefix(":quit").or_else(|| trimmed.strip_prefix(":q")) {
            if rest.is_empty() || rest.starts_with(' ') {
                return ReplResult::Quit;
            }
        }

        if trimmed == ":help" || trimmed == ":h" {
            return ReplResult::Help(help_text());
        }

        if let Some(rest) = trimmed.strip_prefix(":type ") {
            return self.cmd_type(rest.trim());
        }

        if let Some(rest) = trimmed.strip_prefix(":prove ") {
            return self.cmd_prove(rest.trim());
        }

        if let Some(rest) = trimmed.strip_prefix(":load ") {
            return self.cmd_load(rest.trim());
        }

        // Try parsing as a let binding first, then as an expression
        self.eval_input(trimmed)
    }

    fn eval_input(&mut self, input: &str) -> ReplResult {
        // Try as a program item (let binding, function, etc.)
        if input.starts_with("let ") || input.starts_with("function ") {
            let combined = format!("{}\n{}", self.context.source, input);
            match parser::parse(&combined) {
                Ok(_program) => {
                    self.context.source = combined.clone();
                    // Compile just this line and execute persistently
                    match parser::parse(input) {
                        Ok(program) => {
                            let mut compiler = Compiler::new();
                            match compiler.compile_program(&program) {
                                Ok(ir) => match self.interpreter.execute_persistent(&ir) {
                                    Ok(val) if val != Value::Null => {
                                        return ReplResult::Value(format_value(&val));
                                    }
                                    Ok(_) => {
                                        // For let bindings, show the bound name
                                        if let Some(item) = program.last() {
                                            if let ast::Item::LetBinding(lb) = &item.node {
                                                if let Some(val) = self.interpreter.globals().get(&lb.name) {
                                                    return ReplResult::Value(format_value(val));
                                                }
                                            }
                                        }
                                        return ReplResult::Empty;
                                    }
                                    Err(e) => return ReplResult::Error(e.to_string()),
                                },
                                Err(e) => return ReplResult::Error(e.to_string()),
                            }
                        }
                        Err(errors) => return ReplResult::Error(format_parse_errors(&errors)),
                    }
                }
                Err(errors) => {
                    return ReplResult::Error(format_parse_errors(&errors));
                }
            }
        }

        // Try as an expression
        match parser::parse_expression(input) {
            Ok(expr) => {
                let mut compiler = Compiler::new();
                match compiler.compile_expression(&expr) {
                    Ok(ir) => match self.interpreter.execute_persistent(&ir) {
                        Ok(val) => ReplResult::Value(format_value(&val)),
                        Err(e) => ReplResult::Error(e.to_string()),
                    },
                    Err(e) => ReplResult::Error(e.to_string()),
                }
            }
            Err(errors) => ReplResult::Error(format_parse_errors(&errors)),
        }
    }

    fn cmd_type(&mut self, input: &str) -> ReplResult {
        match parser::parse_expression(input) {
            Ok(expr) => match convert_expr_for_types(&expr) {
                Some(tc_expr) => match self.type_checker.synthesize(&tc_expr) {
                    Ok(ty) => ReplResult::TypeInfo(ty.to_string()),
                    Err(e) => ReplResult::Error(format!("Type error: {e}")),
                },
                None => ReplResult::Error("Expression not supported for type inference".into()),
            },
            Err(errors) => ReplResult::Error(format_parse_errors(&errors)),
        }
    }

    fn cmd_prove(&mut self, name: &str) -> ReplResult {
        if self.context.source.is_empty() {
            return ReplResult::Error("No definitions loaded. Use :load to load a file.".into());
        }

        match parser::parse(&self.context.source) {
            Ok(program) => {
                let obligations =
                    lattice_proof::obligation::extract_obligations(&program);
                let relevant: Vec<_> = obligations
                    .iter()
                    .filter(|ob| ob.source.item_name == name)
                    .cloned()
                    .collect();

                if relevant.is_empty() {
                    return ReplResult::ProofResult(format!(
                        "No proof obligations found for '{name}'"
                    ));
                }

                let mut checker = lattice_proof::checker::ProofChecker::new();
                checker.add_backend(Box::new(
                    lattice_proof::arithmetic_backend::ArithmeticBackend,
                ));
                let results = checker.check_all(&relevant);

                let mut output = String::new();
                for (ob, result) in &results {
                    let status = match &result.status {
                        lattice_proof::status::ProofStatus::Verified => "VERIFIED",
                        lattice_proof::status::ProofStatus::Failed { .. } => "FAILED",
                        lattice_proof::status::ProofStatus::Unverified => "UNVERIFIED",
                        lattice_proof::status::ProofStatus::Skipped => "SKIPPED",
                        lattice_proof::status::ProofStatus::Timeout => "TIMEOUT",
                    };
                    output.push_str(&format!("{}: {} ({}ms)\n", ob.name, status, result.duration_ms));
                    if let Some(msg) = &result.message {
                        output.push_str(&format!("  {msg}\n"));
                    }
                }
                ReplResult::ProofResult(output.trim_end().to_string())
            }
            Err(errors) => ReplResult::Error(format_parse_errors(&errors)),
        }
    }

    fn cmd_load(&mut self, path: &str) -> ReplResult {
        match std::fs::read_to_string(path) {
            Ok(source) => match parser::parse(&source) {
                Ok(_) => {
                    if !self.context.source.is_empty() {
                        self.context.source.push('\n');
                    }
                    self.context.source.push_str(&source);
                    self.context.loaded_files.push(path.to_string());
                    ReplResult::Loaded(format!("Loaded {path}"))
                }
                Err(errors) => ReplResult::Error(format_parse_errors(&errors)),
            },
            Err(e) => ReplResult::Error(format!("Cannot read '{path}': {e}")),
        }
    }
}

fn format_value(val: &Value) -> String {
    match val {
        Value::Null => "null".to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Int(n) => n.to_string(),
        Value::Float(f) => format!("{f}"),
        Value::String(s) => format!("\"{s}\""),
        Value::Array(items) => {
            let inner: Vec<String> = items.iter().map(format_value).collect();
            format!("[{}]", inner.join(", "))
        }
        Value::Constructor { name, fields } => {
            if fields.is_empty() {
                name.clone()
            } else {
                let inner: Vec<String> = fields.iter().map(format_value).collect();
                format!("{name}({})", inner.join(", "))
            }
        }
        Value::Object(map) => {
            let fields: Vec<String> = map
                .iter()
                .map(|(k, v)| format!("{k}: {}", format_value(v)))
                .collect();
            format!("{{ {} }}", fields.join(", "))
        }
    }
}

fn format_parse_errors(errors: &[parser::ParseError]) -> String {
    errors
        .iter()
        .map(|e| e.to_string())
        .collect::<Vec<_>>()
        .join("\n")
}

fn help_text() -> String {
    "\
Commands:
  :type <expr>     Infer and display the type of an expression
  :prove <name>    Check proof obligations for a function
  :load <file>     Load a .lattice file into context
  :help            Show this help message
  :quit / :q       Exit the REPL

Enter expressions to evaluate them (e.g. 1 + 2, \"hello\")
Multi-line input: end a line with { or \\ to continue"
        .to_string()
}

/// Check if input needs continuation (unbalanced braces or trailing backslash).
fn needs_continuation(input: &str) -> bool {
    if input.ends_with('\\') {
        return true;
    }
    let mut depth: i32 = 0;
    for ch in input.chars() {
        match ch {
            '{' => depth += 1,
            '}' => depth -= 1,
            _ => {}
        }
    }
    depth > 0
}

/// Check if accumulated multi-line input is balanced.
fn is_balanced(input: &str) -> bool {
    // If last line ends with \, still continuing
    if let Some(last_line) = input.lines().last() {
        if last_line.trim_end().ends_with('\\') {
            return false;
        }
    }
    let mut depth: i32 = 0;
    for ch in input.chars() {
        match ch {
            '{' => depth += 1,
            '}' => depth -= 1,
            _ => {}
        }
    }
    depth <= 0
}

/// Convert a parser AST expression to the type checker's AST expression.
fn convert_expr_for_types(expr: &ast::Expr) -> Option<lattice_types::ast::Expr> {
    use lattice_types::ast as tc;
    let span = tc::Span::dummy();
    match expr {
        ast::Expr::IntLit(n) => Some(tc::Expr::IntLit { value: *n, span }),
        ast::Expr::FloatLit(f) => Some(tc::Expr::FloatLit { value: *f, span }),
        ast::Expr::StringLit(s) => Some(tc::Expr::StringLit {
            value: s.clone(),
            span,
        }),
        ast::Expr::BoolLit(b) => Some(tc::Expr::BoolLit { value: *b, span }),
        ast::Expr::Ident(name) => Some(tc::Expr::Var {
            name: name.clone(),
            span,
        }),
        ast::Expr::BinOp { left, op, right } => {
            let tc_op = match op {
                ast::BinOp::Add => tc::BinOp::Add,
                ast::BinOp::Sub => tc::BinOp::Sub,
                ast::BinOp::Mul => tc::BinOp::Mul,
                ast::BinOp::Div => tc::BinOp::Div,
                ast::BinOp::Mod => tc::BinOp::Mod,
                ast::BinOp::Eq => tc::BinOp::Eq,
                ast::BinOp::Neq => tc::BinOp::Ne,
                ast::BinOp::Lt => tc::BinOp::Lt,
                ast::BinOp::Gt => tc::BinOp::Gt,
                ast::BinOp::Leq => tc::BinOp::Le,
                ast::BinOp::Geq => tc::BinOp::Ge,
                ast::BinOp::And => tc::BinOp::And,
                ast::BinOp::Or => tc::BinOp::Or,
                _ => return None,
            };
            let lhs = convert_expr_for_types(&left.node)?;
            let rhs = convert_expr_for_types(&right.node)?;
            Some(tc::Expr::BinOp {
                op: tc_op,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
                span,
            })
        }
        ast::Expr::If { cond, then_, else_ } => {
            let c = convert_expr_for_types(&cond.node)?;
            let t = convert_expr_for_types(&then_.node)?;
            let e = convert_expr_for_types(&else_.as_ref()?.node)?;
            Some(tc::Expr::If {
                cond: Box::new(c),
                then_branch: Box::new(t),
                else_branch: Box::new(e),
                span,
            })
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn eval_simple_expression() {
        let mut repl = Repl::new();
        let result = repl.eval_line("1 + 2");
        assert_eq!(result, ReplResult::Value("3".to_string()));
    }

    #[test]
    fn eval_let_binding_then_use() {
        let mut repl = Repl::new();
        let r1 = repl.eval_line("let x = 5");
        assert!(matches!(r1, ReplResult::Value(_)));
        // Variable persists across eval_line calls
        let r2 = repl.eval_line("x + 3");
        assert_eq!(r2, ReplResult::Value("8".to_string()));
    }

    #[test]
    fn variable_shadowing() {
        let mut repl = Repl::new();
        repl.eval_line("let x = 10");
        repl.eval_line("let x = 20");
        let r = repl.eval_line("x");
        assert_eq!(r, ReplResult::Value("20".to_string()));
    }

    #[test]
    fn type_command() {
        let mut repl = Repl::new();
        let result = repl.eval_line(":type 42");
        assert!(matches!(result, ReplResult::TypeInfo(ref s) if s.contains("Int")));
    }

    #[test]
    fn help_command() {
        let mut repl = Repl::new();
        let result = repl.eval_line(":help");
        assert!(matches!(result, ReplResult::Help(ref s) if s.contains(":type")));
    }

    #[test]
    fn quit_command() {
        let mut repl = Repl::new();
        assert_eq!(repl.eval_line(":quit"), ReplResult::Quit);
        assert_eq!(repl.eval_line(":q"), ReplResult::Quit);
    }

    #[test]
    fn error_on_invalid_expression() {
        let mut repl = Repl::new();
        let result = repl.eval_line("@@@");
        assert!(matches!(result, ReplResult::Error(_)));
    }

    #[test]
    fn load_nonexistent_file() {
        let mut repl = Repl::new();
        let result = repl.eval_line(":load /nonexistent/file.lattice");
        assert!(matches!(result, ReplResult::Error(ref s) if s.contains("Cannot read")));
    }

    #[test]
    fn multiline_detection() {
        assert!(needs_continuation("function foo() {"));
        assert!(needs_continuation("let x = \\"));
        assert!(!needs_continuation("1 + 2"));
        assert!(!needs_continuation("}"));
    }

    #[test]
    fn empty_line() {
        let mut repl = Repl::new();
        assert_eq!(repl.eval_line(""), ReplResult::Empty);
        assert_eq!(repl.eval_line("   "), ReplResult::Empty);
    }

    #[test]
    fn eval_boolean_expression() {
        let mut repl = Repl::new();
        assert_eq!(
            repl.eval_line("true and false"),
            ReplResult::Value("false".to_string())
        );
    }

    #[test]
    fn eval_string_expression() {
        let mut repl = Repl::new();
        let result = repl.eval_line("\"hello\"");
        assert_eq!(result, ReplResult::Value("\"hello\"".to_string()));
    }

    #[test]
    fn prove_with_no_context() {
        let mut repl = Repl::new();
        let result = repl.eval_line(":prove foo");
        assert!(matches!(result, ReplResult::Error(ref s) if s.contains("No definitions")));
    }

    #[test]
    fn function_persistence_across_lines() {
        let mut repl = Repl::new();
        let r1 = repl.eval_line("function double(x: Int) -> Int { x * 2 }");
        assert!(matches!(r1, ReplResult::Empty | ReplResult::Value(_)));
        let r2 = repl.eval_line("double(5)");
        assert_eq!(r2, ReplResult::Value("10".to_string()));
    }

    #[test]
    fn function_can_use_persistent_variables() {
        let mut repl = Repl::new();
        repl.eval_line("let factor = 3");
        let r1 = repl.eval_line("function triple(x: Int) -> Int { x * factor }");
        assert!(matches!(r1, ReplResult::Empty | ReplResult::Value(_)));
        let r2 = repl.eval_line("triple(7)");
        assert_eq!(r2, ReplResult::Value("21".to_string()));
    }

    #[test]
    fn format_value_array() {
        let val = Value::Array(vec![Value::Int(1), Value::Int(2), Value::Int(3)]);
        assert_eq!(format_value(&val), "[1, 2, 3]");
    }
}
