use crate::error::CodegenError;
use crate::ir::*;
use lattice_parser::ast;
use lattice_parser::ast::Spanned;

pub struct Compiler {
    functions: Vec<Function>,
}

impl Default for Compiler {
    fn default() -> Self {
        Self::new()
    }
}

impl Compiler {
    pub fn new() -> Self {
        Self {
            functions: Vec::new(),
        }
    }

    /// Compile a full program (list of top-level items).
    pub fn compile_program(&mut self, program: &ast::Program) -> Result<Program, CodegenError> {
        let mut main_instructions = Vec::new();

        for item in program {
            match &item.node {
                ast::Item::Function(func) => {
                    let ir_func = self.compile_function(func)?;
                    self.functions.push(ir_func);
                }
                ast::Item::LetBinding(binding) => {
                    self.compile_expr(&binding.value.node, &mut main_instructions)?;
                    main_instructions.push(Instruction::StoreVar(binding.name.clone()));
                    main_instructions.push(Instruction::PushNull);
                }
                ast::Item::TypeDef(td) => {
                    self.compile_type_def(td);
                }
                _ => {} // Skip graphs, modules, models, meta for now
            }
        }

        if main_instructions.is_empty() {
            main_instructions.push(Instruction::PushNull);
        }
        main_instructions.push(Instruction::Return);

        let entry = self.functions.len();
        self.functions.push(Function {
            name: "__main__".into(),
            params: vec![],
            instructions: main_instructions,
        });

        Ok(Program {
            functions: std::mem::take(&mut self.functions),
            entry,
        })
    }

    /// Compile a single expression into a self-contained program (for REPL / testing).
    pub fn compile_expression(&mut self, expr: &ast::Expr) -> Result<Program, CodegenError> {
        let mut instructions = Vec::new();
        self.compile_expr(expr, &mut instructions)?;
        instructions.push(Instruction::Return);

        let entry = self.functions.len();
        self.functions.push(Function {
            name: "__expr__".into(),
            params: vec![],
            instructions,
        });

        Ok(Program {
            functions: std::mem::take(&mut self.functions),
            entry,
        })
    }

    fn compile_function(&mut self, func: &ast::Function) -> Result<Function, CodegenError> {
        let mut instructions = Vec::new();

        match &func.body {
            ast::FunctionBody::Block(exprs) => {
                for (i, expr) in exprs.iter().enumerate() {
                    self.compile_expr(&expr.node, &mut instructions)?;
                    if i < exprs.len() - 1 {
                        instructions.push(Instruction::Pop);
                    }
                }
                if exprs.is_empty() {
                    instructions.push(Instruction::PushNull);
                }
            }
            ast::FunctionBody::Synthesize(_) => {
                return Err(CodegenError::Unsupported(
                    "synthesize blocks".to_string(),
                ));
            }
        }

        instructions.push(Instruction::Return);

        let params = func.params.iter().map(|p| p.name.clone()).collect();

        Ok(Function {
            name: func.name.clone(),
            params,
            instructions,
        })
    }

    fn compile_expr(
        &mut self,
        expr: &ast::Expr,
        instructions: &mut Vec<Instruction>,
    ) -> Result<(), CodegenError> {
        match expr {
            ast::Expr::IntLit(n) => {
                instructions.push(Instruction::PushInt(*n));
            }
            ast::Expr::FloatLit(f) => {
                instructions.push(Instruction::PushFloat(*f));
            }
            ast::Expr::BoolLit(b) => {
                instructions.push(Instruction::PushBool(*b));
            }
            ast::Expr::StringLit(s) => {
                instructions.push(Instruction::PushString(s.clone()));
            }
            ast::Expr::Ident(name) => {
                instructions.push(Instruction::LoadVar(name.clone()));
            }

            ast::Expr::BinOp { left, op, right } => {
                self.compile_expr(&left.node, instructions)?;
                self.compile_expr(&right.node, instructions)?;
                let instr = match op {
                    ast::BinOp::Add => Instruction::Add,
                    ast::BinOp::Sub => Instruction::Sub,
                    ast::BinOp::Mul => Instruction::Mul,
                    ast::BinOp::Div => Instruction::Div,
                    ast::BinOp::Mod => Instruction::Mod,
                    ast::BinOp::Eq => Instruction::Eq,
                    ast::BinOp::Neq => Instruction::Neq,
                    ast::BinOp::Lt => Instruction::Lt,
                    ast::BinOp::Gt => Instruction::Gt,
                    ast::BinOp::Leq => Instruction::Leq,
                    ast::BinOp::Geq => Instruction::Geq,
                    ast::BinOp::And => Instruction::And,
                    ast::BinOp::Or => Instruction::Or,
                    ast::BinOp::Concat => Instruction::Concat,
                    other => {
                        return Err(CodegenError::Unsupported(format!("{other:?}")));
                    }
                };
                instructions.push(instr);
            }

            ast::Expr::UnaryOp { op, operand } => {
                self.compile_expr(&operand.node, instructions)?;
                match op {
                    ast::UnaryOp::Neg => instructions.push(Instruction::Neg),
                    ast::UnaryOp::Not => instructions.push(Instruction::Not),
                }
            }

            ast::Expr::Let { name, value, .. } => {
                self.compile_expr(&value.node, instructions)?;
                instructions.push(Instruction::StoreVar(name.clone()));
                // Every expression must push exactly one value for consistent Block handling.
                instructions.push(Instruction::PushNull);
            }

            ast::Expr::Call { func, args } => {
                if let ast::Expr::Ident(name) = &func.node {
                    for arg in args {
                        self.compile_expr(&arg.node, instructions)?;
                    }
                    instructions.push(Instruction::Call(name.clone(), args.len()));
                } else {
                    // Non-identifier call: compile callee first, then args, then CallClosure
                    self.compile_expr(&func.node, instructions)?;
                    for arg in args {
                        self.compile_expr(&arg.node, instructions)?;
                    }
                    instructions.push(Instruction::CallClosure(args.len()));
                }
            }

            ast::Expr::Array(elements) => {
                for elem in elements {
                    self.compile_expr(&elem.node, instructions)?;
                }
                instructions.push(Instruction::MakeArray(elements.len()));
            }

            ast::Expr::Record(fields) => {
                let names: Vec<String> = fields.iter().map(|(n, _)| n.clone()).collect();
                for (_, val) in fields {
                    self.compile_expr(&val.node, instructions)?;
                }
                instructions.push(Instruction::MakeRecord(names));
            }

            ast::Expr::Field { expr, name } => {
                self.compile_expr(&expr.node, instructions)?;
                instructions.push(Instruction::GetField(name.clone()));
            }

            // Pipeline: a |> f  =>  f(a)
            ast::Expr::Pipeline { left, right } => {
                if let ast::Expr::Ident(name) = &right.node {
                    self.compile_expr(&left.node, instructions)?;
                    instructions.push(Instruction::Call(name.clone(), 1));
                } else {
                    // Pipeline to lambda/closure: closure first, then arg
                    self.compile_expr(&right.node, instructions)?;
                    self.compile_expr(&left.node, instructions)?;
                    instructions.push(Instruction::CallClosure(1));
                }
            }

            ast::Expr::If { cond, then_, else_ } => {
                self.compile_expr(&cond.node, instructions)?;
                let jump_else = instructions.len();
                instructions.push(Instruction::JumpIfFalse(0)); // placeholder

                self.compile_expr(&then_.node, instructions)?;
                let jump_end = instructions.len();
                instructions.push(Instruction::Jump(0)); // placeholder

                let else_start = instructions.len();
                instructions[jump_else] = Instruction::JumpIfFalse(else_start);

                if let Some(else_expr) = else_ {
                    self.compile_expr(&else_expr.node, instructions)?;
                } else {
                    instructions.push(Instruction::PushNull);
                }

                let end = instructions.len();
                instructions[jump_end] = Instruction::Jump(end);
            }

            ast::Expr::Block(exprs) => {
                for (i, e) in exprs.iter().enumerate() {
                    self.compile_expr(&e.node, instructions)?;
                    if i < exprs.len() - 1 {
                        instructions.push(Instruction::Pop);
                    }
                }
                if exprs.is_empty() {
                    instructions.push(Instruction::PushNull);
                }
            }

            ast::Expr::Index { expr, index } => {
                self.compile_expr(&expr.node, instructions)?;
                self.compile_expr(&index.node, instructions)?;
                instructions.push(Instruction::IndexArray);
            }

            ast::Expr::Slice { expr, start, end } => {
                self.compile_expr(&expr.node, instructions)?;
                if let Some(s) = start {
                    self.compile_expr(&s.node, instructions)?;
                } else {
                    instructions.push(Instruction::PushInt(-1));
                }
                if let Some(e) = end {
                    self.compile_expr(&e.node, instructions)?;
                } else {
                    instructions.push(Instruction::PushInt(-1));
                }
                instructions.push(Instruction::SliceArray);
            }

            ast::Expr::Match { expr, arms } => {
                self.compile_match(expr, arms, instructions)?;
            }

            ast::Expr::Lambda { params, body } => {
                self.compile_lambda(params, body, instructions)?;
            }

            _ => {
                return Err(CodegenError::Unsupported(format!("{expr:?}")));
            }
        }

        Ok(())
    }

    fn compile_match(
        &mut self,
        scrutinee: &Spanned<ast::Expr>,
        arms: &[ast::MatchArm],
        instructions: &mut Vec<Instruction>,
    ) -> Result<(), CodegenError> {
        // Compile scrutinee → stack: [scrutinee]
        self.compile_expr(&scrutinee.node, instructions)?;

        let mut end_jumps = Vec::new();

        for arm in arms {
            // Dup scrutinee for pattern testing → stack: [scrutinee, dup]
            instructions.push(Instruction::Dup);

            // Pattern test: peeks at dup, pushes bool → stack: [scrutinee, dup, bool]
            self.compile_pattern_test(&arm.pattern.node, instructions)?;

            let next_arm = instructions.len();
            // JumpIfFalse pops bool → stack: [scrutinee, dup]
            instructions.push(Instruction::JumpIfFalse(0)); // placeholder

            // Pattern matched — bind variables (consumes dup) → stack: [scrutinee]
            self.compile_pattern_bind(&arm.pattern.node, instructions)?;

            // Compile arm body → stack: [scrutinee, result]
            self.compile_expr(&arm.body.node, instructions)?;

            // Swap result under scrutinee, pop scrutinee → stack: [result]
            instructions.push(Instruction::Swap);
            instructions.push(Instruction::Pop);

            end_jumps.push(instructions.len());
            instructions.push(Instruction::Jump(0)); // placeholder: jump to end

            let next = instructions.len();
            instructions[next_arm] = Instruction::JumpIfFalse(next);
            // Pattern didn't match — pop the dup → stack: [scrutinee]
            instructions.push(Instruction::Pop);
        }

        // No arm matched — pop scrutinee, push null
        instructions.push(Instruction::Pop);
        instructions.push(Instruction::PushNull);

        let end = instructions.len();
        for jump in end_jumps {
            instructions[jump] = Instruction::Jump(end);
        }

        Ok(())
    }

    /// Compile a pattern test. Stack before: [..., dup]. Stack after: [..., dup, bool].
    /// The dup value is left on the stack for compile_pattern_bind to consume.
    fn compile_pattern_test(
        &mut self,
        pattern: &ast::Pattern,
        instructions: &mut Vec<Instruction>,
    ) -> Result<(), CodegenError> {
        match pattern {
            ast::Pattern::Wildcard | ast::Pattern::Ident(_) => {
                // Always matches
                instructions.push(Instruction::PushBool(true));
            }
            ast::Pattern::Literal(lit) => {
                // TestXxx peeks at the value and pushes bool
                match &lit.node {
                    ast::Expr::IntLit(n) => {
                        instructions.push(Instruction::TestInt(*n));
                    }
                    ast::Expr::StringLit(s) => {
                        instructions.push(Instruction::TestString(s.clone()));
                    }
                    ast::Expr::BoolLit(b) => {
                        instructions.push(Instruction::TestBool(*b));
                    }
                    _ => {
                        return Err(CodegenError::Unsupported(
                            "complex literal in pattern".to_string(),
                        ));
                    }
                }
            }
            ast::Pattern::Constructor(name, _) => {
                instructions.push(Instruction::TestConstructor(name.clone()));
            }
            ast::Pattern::Record(_) => {
                return Err(CodegenError::Unsupported(
                    "record pattern matching".to_string(),
                ));
            }
        }
        Ok(())
    }

    fn compile_pattern_bind(
        &mut self,
        pattern: &ast::Pattern,
        instructions: &mut Vec<Instruction>,
    ) -> Result<(), CodegenError> {
        match pattern {
            ast::Pattern::Wildcard => {
                // Pop the duped scrutinee, nothing to bind
                instructions.push(Instruction::Pop);
            }
            ast::Pattern::Ident(name) => {
                // Bind the duped scrutinee to the variable
                instructions.push(Instruction::StoreVar(name.clone()));
            }
            ast::Pattern::Literal(_) => {
                // Pop the duped scrutinee, no binding needed
                instructions.push(Instruction::Pop);
            }
            ast::Pattern::Constructor(_, sub_patterns) => {
                // Extract fields for sub-patterns
                for (i, sub) in sub_patterns.iter().enumerate() {
                    if let ast::Pattern::Ident(name) = &sub.node {
                        instructions.push(Instruction::Dup);
                        instructions.push(Instruction::ExtractField(i));
                        instructions.push(Instruction::StoreVar(name.clone()));
                    }
                }
                instructions.push(Instruction::Pop); // pop constructor value
            }
            ast::Pattern::Record(_) => {
                return Err(CodegenError::Unsupported(
                    "record pattern binding".to_string(),
                ));
            }
        }
        Ok(())
    }

    /// Generate constructor functions for a sum type definition.
    /// e.g. `type Option = Some(value: Int) | None` produces:
    ///   fn Some(value) { MakeConstructor("Some", 1); Return }
    ///   fn None() { MakeConstructor("None", 0); Return }
    fn compile_type_def(&mut self, td: &ast::TypeDef) {
        if let ast::TypeExpr::Sum(variants) = &td.body.node {
            for variant in variants {
                let params: Vec<String> = variant.fields.iter().map(|(n, _)| n.clone()).collect();
                let arity = params.len();
                let mut instructions = Vec::new();
                // Load each parameter onto the stack in order
                for p in &params {
                    instructions.push(Instruction::LoadVar(p.clone()));
                }
                instructions.push(Instruction::MakeConstructor(variant.name.clone(), arity));
                instructions.push(Instruction::Return);
                self.functions.push(Function {
                    name: variant.name.clone(),
                    params,
                    instructions,
                });
            }
        }
    }

    fn compile_lambda(
        &mut self,
        params: &[ast::Param],
        body: &Spanned<ast::Expr>,
        instructions: &mut Vec<Instruction>,
    ) -> Result<(), CodegenError> {
        // Compile the lambda body as a separate function
        let mut body_instructions = Vec::new();
        self.compile_expr(&body.node, &mut body_instructions)?;
        body_instructions.push(Instruction::Return);

        let param_names: Vec<String> = params.iter().map(|p| p.name.clone()).collect();

        // Collect free variables via analysis
        let mut free_vars = std::collections::HashSet::new();
        let mut bound_vars: std::collections::HashSet<String> =
            param_names.iter().cloned().collect();
        collect_free_vars(&body.node, &mut free_vars, &mut bound_vars);
        let captured: Vec<String> = free_vars.into_iter().collect();

        let func_idx = self.functions.len();
        self.functions.push(Function {
            name: format!("__lambda_{func_idx}__"),
            params: param_names,
            instructions: body_instructions,
        });

        instructions.push(Instruction::MakeClosure(func_idx, captured));
        Ok(())
    }
}

/// Collect free variables in an expression (variables not bound by local scope).
fn collect_free_vars(
    expr: &ast::Expr,
    free: &mut std::collections::HashSet<String>,
    bound: &mut std::collections::HashSet<String>,
) {
    match expr {
        ast::Expr::Ident(name) => {
            if !bound.contains(name) {
                free.insert(name.clone());
            }
        }
        ast::Expr::IntLit(_)
        | ast::Expr::FloatLit(_)
        | ast::Expr::StringLit(_)
        | ast::Expr::BoolLit(_) => {}

        ast::Expr::BinOp { left, right, .. } => {
            collect_free_vars(&left.node, free, bound);
            collect_free_vars(&right.node, free, bound);
        }
        ast::Expr::UnaryOp { operand, .. } => {
            collect_free_vars(&operand.node, free, bound);
        }
        ast::Expr::Call { func, args } => {
            collect_free_vars(&func.node, free, bound);
            for arg in args {
                collect_free_vars(&arg.node, free, bound);
            }
        }
        ast::Expr::CallNamed { func, args } => {
            collect_free_vars(&func.node, free, bound);
            for (_, arg) in args {
                collect_free_vars(&arg.node, free, bound);
            }
        }
        ast::Expr::Field { expr, .. } => {
            collect_free_vars(&expr.node, free, bound);
        }
        ast::Expr::Index { expr, index } => {
            collect_free_vars(&expr.node, free, bound);
            collect_free_vars(&index.node, free, bound);
        }
        ast::Expr::Pipeline { left, right } => {
            collect_free_vars(&left.node, free, bound);
            collect_free_vars(&right.node, free, bound);
        }
        ast::Expr::Lambda { params, body } => {
            let mut inner_bound = bound.clone();
            for p in params {
                inner_bound.insert(p.name.clone());
            }
            collect_free_vars(&body.node, free, &mut inner_bound);
        }
        ast::Expr::Let { name, value, .. } => {
            collect_free_vars(&value.node, free, bound);
            bound.insert(name.clone());
        }
        ast::Expr::If { cond, then_, else_ } => {
            collect_free_vars(&cond.node, free, bound);
            collect_free_vars(&then_.node, free, bound);
            if let Some(e) = else_ {
                collect_free_vars(&e.node, free, bound);
            }
        }
        ast::Expr::Block(exprs) => {
            let mut inner_bound = bound.clone();
            for e in exprs {
                collect_free_vars(&e.node, free, &mut inner_bound);
            }
        }
        ast::Expr::Match { expr, arms } => {
            collect_free_vars(&expr.node, free, bound);
            for arm in arms {
                let mut arm_bound = bound.clone();
                collect_pattern_bindings(&arm.pattern.node, &mut arm_bound);
                if let Some(guard) = &arm.guard {
                    collect_free_vars(&guard.node, free, &mut arm_bound);
                }
                collect_free_vars(&arm.body.node, free, &mut arm_bound);
            }
        }
        ast::Expr::Array(elems) => {
            for e in elems {
                collect_free_vars(&e.node, free, bound);
            }
        }
        ast::Expr::Record(fields) => {
            for (_, v) in fields {
                collect_free_vars(&v.node, free, bound);
            }
        }
        // Other expressions: traverse children if any
        ast::Expr::Slice { expr, start, end } => {
            collect_free_vars(&expr.node, free, bound);
            if let Some(s) = start {
                collect_free_vars(&s.node, free, bound);
            }
            if let Some(e) = end {
                collect_free_vars(&e.node, free, bound);
            }
        }
        ast::Expr::Range { start, end } => {
            collect_free_vars(&start.node, free, bound);
            collect_free_vars(&end.node, free, bound);
        }
        ast::Expr::Try(e) | ast::Expr::Yield(e) => {
            collect_free_vars(&e.node, free, bound);
        }
        ast::Expr::Ascription { expr, .. } => {
            collect_free_vars(&expr.node, free, bound);
        }
        ast::Expr::WithUnit { value, .. } => {
            collect_free_vars(&value.node, free, bound);
        }
        _ => {
            // DoBlock, Select, Project, Join, GroupBy, ForAll, Exists, Branch, Synthesize
            // These are less common; skip for now
        }
    }
}

fn collect_pattern_bindings(
    pattern: &ast::Pattern,
    bound: &mut std::collections::HashSet<String>,
) {
    match pattern {
        ast::Pattern::Ident(name) => {
            bound.insert(name.clone());
        }
        ast::Pattern::Constructor(_, sub_pats) => {
            for p in sub_pats {
                collect_pattern_bindings(&p.node, bound);
            }
        }
        ast::Pattern::Record(fields) => {
            for (_, p) in fields {
                collect_pattern_bindings(&p.node, bound);
            }
        }
        ast::Pattern::Wildcard | ast::Pattern::Literal(_) => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lattice_parser::ast::{BinOp, Expr, Spanned, UnaryOp};

    fn s(expr: Expr) -> Spanned<Expr> {
        Spanned::dummy(expr)
    }

    #[test]
    fn compile_int_literal() {
        let mut c = Compiler::new();
        let prog = c.compile_expression(&Expr::IntLit(42)).unwrap();
        assert_eq!(prog.functions.len(), 1);
        assert!(matches!(
            prog.functions[0].instructions[0],
            Instruction::PushInt(42)
        ));
    }

    #[test]
    fn compile_binop() {
        let expr = Expr::BinOp {
            left: Box::new(s(Expr::IntLit(2))),
            op: BinOp::Add,
            right: Box::new(s(Expr::BinOp {
                left: Box::new(s(Expr::IntLit(3))),
                op: BinOp::Mul,
                right: Box::new(s(Expr::IntLit(4))),
            })),
        };
        let mut c = Compiler::new();
        let prog = c.compile_expression(&expr).unwrap();
        let instrs = &prog.functions[0].instructions;
        // PushInt(2), PushInt(3), PushInt(4), Mul, Add, Return
        assert!(matches!(instrs[0], Instruction::PushInt(2)));
        assert!(matches!(instrs[1], Instruction::PushInt(3)));
        assert!(matches!(instrs[2], Instruction::PushInt(4)));
        assert!(matches!(instrs[3], Instruction::Mul));
        assert!(matches!(instrs[4], Instruction::Add));
    }

    #[test]
    fn compile_let_binding() {
        let expr = Expr::Let {
            name: "x".into(),
            type_ann: None,
            value: Box::new(s(Expr::IntLit(5))),
        };
        let mut c = Compiler::new();
        let prog = c.compile_expression(&expr).unwrap();
        let instrs = &prog.functions[0].instructions;
        assert!(matches!(instrs[0], Instruction::PushInt(5)));
        assert!(matches!(instrs[1], Instruction::StoreVar(_)));
        assert!(matches!(instrs[2], Instruction::PushNull));
    }

    #[test]
    fn compile_if_expression() {
        let expr = Expr::If {
            cond: Box::new(s(Expr::BoolLit(true))),
            then_: Box::new(s(Expr::IntLit(1))),
            else_: Some(Box::new(s(Expr::IntLit(2)))),
        };
        let mut c = Compiler::new();
        let prog = c.compile_expression(&expr).unwrap();
        let instrs = &prog.functions[0].instructions;
        // PushBool(true), JumpIfFalse(_), PushInt(1), Jump(_), PushInt(2), Return
        assert!(matches!(instrs[0], Instruction::PushBool(true)));
        assert!(matches!(instrs[1], Instruction::JumpIfFalse(_)));
        assert!(matches!(instrs[2], Instruction::PushInt(1)));
        assert!(matches!(instrs[3], Instruction::Jump(_)));
        assert!(matches!(instrs[4], Instruction::PushInt(2)));
    }

    #[test]
    fn compile_unary_neg() {
        let expr = Expr::UnaryOp {
            op: UnaryOp::Neg,
            operand: Box::new(s(Expr::IntLit(7))),
        };
        let mut c = Compiler::new();
        let prog = c.compile_expression(&expr).unwrap();
        let instrs = &prog.functions[0].instructions;
        assert!(matches!(instrs[0], Instruction::PushInt(7)));
        assert!(matches!(instrs[1], Instruction::Neg));
    }

    #[test]
    fn compile_array() {
        let expr = Expr::Array(vec![s(Expr::IntLit(1)), s(Expr::IntLit(2))]);
        let mut c = Compiler::new();
        let prog = c.compile_expression(&expr).unwrap();
        let instrs = &prog.functions[0].instructions;
        assert!(matches!(instrs[0], Instruction::PushInt(1)));
        assert!(matches!(instrs[1], Instruction::PushInt(2)));
        assert!(matches!(instrs[2], Instruction::MakeArray(2)));
    }

    #[test]
    fn compile_record_and_field() {
        let record = Expr::Record(vec![
            ("x".into(), s(Expr::IntLit(10))),
            ("y".into(), s(Expr::IntLit(20))),
        ]);
        let expr = Expr::Field {
            expr: Box::new(s(record)),
            name: "x".into(),
        };
        let mut c = Compiler::new();
        let prog = c.compile_expression(&expr).unwrap();
        let instrs = &prog.functions[0].instructions;
        assert!(matches!(instrs[0], Instruction::PushInt(10)));
        assert!(matches!(instrs[1], Instruction::PushInt(20)));
        assert!(matches!(instrs[2], Instruction::MakeRecord(_)));
        assert!(matches!(instrs[3], Instruction::GetField(_)));
    }

    #[test]
    fn compile_pipeline() {
        let expr = Expr::Pipeline {
            left: Box::new(s(Expr::IntLit(5))),
            right: Box::new(s(Expr::Ident("double".into()))),
        };
        let mut c = Compiler::new();
        let prog = c.compile_expression(&expr).unwrap();
        let instrs = &prog.functions[0].instructions;
        assert!(matches!(instrs[0], Instruction::PushInt(5)));
        assert!(matches!(instrs[1], Instruction::Call(_, 1)));
    }

    #[test]
    fn compile_sum_type_constructors() {
        use crate::interpreter::Interpreter;
        use lattice_runtime::node::Value;

        let source = r#"
            type Option = Some(value: Int) | None
            let x = Some(42)
        "#;
        let items = lattice_parser::parser::parse(source).expect("parse failed");
        let mut compiler = Compiler::new();
        let program = compiler.compile_program(&items).unwrap();

        // Verify constructor functions are generated
        let func_names: Vec<&str> = program.functions.iter().map(|f| f.name.as_str()).collect();
        assert!(func_names.contains(&"Some"), "functions: {func_names:?}");
        assert!(func_names.contains(&"None"), "functions: {func_names:?}");

        let mut interp = Interpreter::new();
        interp.register_stdlib();
        interp.execute_persistent(&program).unwrap();
        assert_eq!(
            interp.globals().get("x").cloned(),
            Some(Value::Constructor {
                name: "Some".into(),
                fields: vec![Value::Int(42)],
            })
        );
    }

    #[test]
    fn compile_sum_type_match() {
        use crate::interpreter::Interpreter;
        use lattice_runtime::node::Value;

        let source = r#"
            type Option = Some(value: Int) | None
            function unwrap_or(opt: Option, default: Int) {
                match opt {
                    Some(v) -> v
                    None -> default
                }
            }
            let result = unwrap_or(Some(42), 0)
        "#;
        let items = lattice_parser::parser::parse(source).expect("parse failed");
        let mut compiler = Compiler::new();
        let program = compiler.compile_program(&items).unwrap();
        let mut interp = Interpreter::new();
        interp.register_stdlib();
        interp.execute_persistent(&program).unwrap();
        assert_eq!(interp.globals().get("result").cloned(), Some(Value::Int(42)));
    }

    #[test]
    fn compile_simple_function_call() {
        use crate::interpreter::Interpreter;
        use lattice_runtime::node::Value;

        // First test: does a simple function call work?
        let source = r#"
            function double(n: Int) {
                n * 2
            }
            let result = double(5)
        "#;
        let items = lattice_parser::parser::parse(source).expect("parse failed");
        let mut compiler = Compiler::new();
        let program = compiler.compile_program(&items).unwrap();
        let mut interp = Interpreter::new();
        interp.register_stdlib();
        interp.execute_persistent(&program).unwrap();
        assert_eq!(interp.globals().get("result").cloned(), Some(Value::Int(10)));
    }

    #[test]
    fn compile_recursive_factorial() {
        use crate::interpreter::Interpreter;
        use lattice_runtime::node::Value;

        let source = r#"
            function factorial(n: Int) {
                if n <= 1 then 1 else n * factorial(n - 1)
            }
            let result = factorial(5)
        "#;
        let items = lattice_parser::parser::parse(source).expect("parse failed");
        let mut compiler = Compiler::new();
        let program = compiler.compile_program(&items).unwrap();
        let mut interp = Interpreter::new();
        interp.register_stdlib();
        interp.execute_persistent(&program).unwrap();
        assert_eq!(interp.globals().get("result").cloned(), Some(Value::Int(120)));
    }

    #[test]
    fn compile_recursive_fibonacci() {
        use crate::interpreter::Interpreter;
        use lattice_runtime::node::Value;

        let source = r#"
            function fib(n: Int) {
                if n <= 1 then n else fib(n - 1) + fib(n - 2)
            }
            let result = fib(10)
        "#;
        let items = lattice_parser::parser::parse(source).expect("parse failed");
        let mut compiler = Compiler::new();
        let program = compiler.compile_program(&items).unwrap();
        let mut interp = Interpreter::new();
        interp.register_stdlib();
        interp.execute_persistent(&program).unwrap();
        assert_eq!(interp.globals().get("result").cloned(), Some(Value::Int(55)));
    }

    #[test]
    fn compile_mutual_recursion() {
        use crate::interpreter::Interpreter;
        use lattice_runtime::node::Value;

        let source = r#"
            function is_even(n: Int) {
                if n == 0 then true else is_odd(n - 1)
            }
            function is_odd(n: Int) {
                if n == 0 then false else is_even(n - 1)
            }
            let result = is_even(4)
        "#;
        let items = lattice_parser::parser::parse(source).expect("parse failed");
        let mut compiler = Compiler::new();
        let program = compiler.compile_program(&items).unwrap();
        let mut interp = Interpreter::new();
        interp.register_stdlib();
        interp.execute_persistent(&program).unwrap();
        assert_eq!(interp.globals().get("result").cloned(), Some(Value::Bool(true)));
    }

    #[test]
    fn compile_recursive_list_sum() {
        use crate::interpreter::Interpreter;
        use lattice_runtime::node::Value;

        let source = r#"
            type List = Cons(head: Int, tail: List) | Nil
            function sum_list(lst: List) {
                match lst {
                    Cons(h, t) -> h + sum_list(t)
                    Nil -> 0
                }
            }
            let mylist = Cons(1, Cons(2, Cons(3, Nil())))
            let result = sum_list(mylist)
        "#;
        let items = lattice_parser::parser::parse(source).expect("parse failed");
        let mut compiler = Compiler::new();
        let program = compiler.compile_program(&items).unwrap();
        let mut interp = Interpreter::new();
        interp.register_stdlib();
        interp.execute_persistent(&program).unwrap();
        assert_eq!(interp.globals().get("result").cloned(), Some(Value::Int(6)));
    }

    #[test]
    fn compile_sum_type_nullary_constructor() {
        use crate::interpreter::Interpreter;
        use lattice_runtime::node::Value;

        let source = r#"
            type Option = Some(value: Int) | None
            function unwrap_or(opt: Option, default: Int) {
                match opt {
                    Some(v) -> v
                    None -> default
                }
            }
            let result = unwrap_or(None(), 99)
        "#;
        let items = lattice_parser::parser::parse(source).expect("parse failed");
        let mut compiler = Compiler::new();
        let program = compiler.compile_program(&items).unwrap();
        let mut interp = Interpreter::new();
        interp.register_stdlib();
        interp.execute_persistent(&program).unwrap();
        assert_eq!(interp.globals().get("result").cloned(), Some(Value::Int(99)));
    }
}
