use crate::error::CodegenError;
use crate::ir::*;
use lattice_parser::ast;

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
                _ => {} // Skip graphs, types, modules, models, meta for now
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

        Ok(Program {
            functions: vec![Function {
                name: "__expr__".into(),
                params: vec![],
                instructions,
            }],
            entry: 0,
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
                for arg in args {
                    self.compile_expr(&arg.node, instructions)?;
                }
                if let ast::Expr::Ident(name) = &func.node {
                    instructions.push(Instruction::Call(name.clone(), args.len()));
                } else {
                    return Err(CodegenError::Unsupported(
                        "non-identifier function calls".to_string(),
                    ));
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
                self.compile_expr(&left.node, instructions)?;
                if let ast::Expr::Ident(name) = &right.node {
                    instructions.push(Instruction::Call(name.clone(), 1));
                } else {
                    return Err(CodegenError::Unsupported(
                        "non-identifier pipeline target".to_string(),
                    ));
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

            _ => {
                return Err(CodegenError::Unsupported(format!("{expr:?}")));
            }
        }

        Ok(())
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
}
