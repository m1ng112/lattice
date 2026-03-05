/// A compiled program in Lattice IR.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Program {
    pub functions: Vec<Function>,
    /// Index of the entry function in `functions`.
    pub entry: usize,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Function {
    pub name: String,
    pub params: Vec<String>,
    pub instructions: Vec<Instruction>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum Instruction {
    // ── Constants ────────────────────────────
    PushInt(i64),
    PushFloat(f64),
    PushBool(bool),
    PushString(String),
    PushNull,

    // ── Variables ────────────────────────────
    LoadVar(String),
    StoreVar(String),

    // ── Arithmetic ──────────────────────────
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Neg,
    Concat,

    // ── Comparison ──────────────────────────
    Eq,
    Neq,
    Lt,
    Gt,
    Leq,
    Geq,

    // ── Logic ───────────────────────────────
    And,
    Or,
    Not,

    // ── Stack manipulation ──────────────────
    Dup,
    Pop,
    Swap,

    // ── Control flow ────────────────────────
    Jump(usize),
    JumpIfFalse(usize),

    // ── Functions ───────────────────────────
    /// Call a function by name with `n` arguments on the stack.
    Call(String, usize),
    Return,

    // ── Data structures ─────────────────────
    GetField(String),
    MakeArray(usize),
    MakeRecord(Vec<String>),

    // ── Pattern matching ─────────────────────
    /// Test if top of stack equals a literal int (pushes bool, doesn't pop).
    TestInt(i64),
    /// Test if top of stack equals a literal string (pushes bool, doesn't pop).
    TestString(String),
    /// Test if top of stack equals a literal bool (pushes bool, doesn't pop).
    TestBool(bool),
    /// Test if top of stack is a constructor with the given name (pushes bool, doesn't pop).
    TestConstructor(String),
    /// Extract the nth field from a constructor value on top of stack.
    ExtractField(usize),

    // ── Closures ────────────────────────────
    /// Create a closure: captures listed variables, function index, param count.
    MakeClosure(usize, Vec<String>),
    /// Call a closure on top of stack with n arguments below it.
    CallClosure(usize),

    // ── Debugging ───────────────────────────
    Print,

    // ── No-op ───────────────────────────────
    Nop,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serialization_roundtrip() {
        let program = Program {
            functions: vec![Function {
                name: "__main__".into(),
                params: vec![],
                instructions: vec![
                    Instruction::PushInt(2),
                    Instruction::PushInt(3),
                    Instruction::Add,
                    Instruction::StoreVar("x".into()),
                    Instruction::PushNull,
                    Instruction::Pop,
                    Instruction::LoadVar("x".into()),
                    Instruction::Return,
                ],
            }],
            entry: 0,
        };

        let json = serde_json::to_string(&program).unwrap();
        let back: Program = serde_json::from_str(&json).unwrap();

        assert_eq!(back.functions.len(), 1);
        assert_eq!(back.entry, 0);
        assert_eq!(back.functions[0].name, "__main__");
        assert_eq!(back.functions[0].instructions.len(), 8);
    }
}
