//! Minimal local AST definitions for use by the type checker.
//!
//! These are self-contained within lattice-types to avoid coupling
//! with lattice-parser. They will be unified later.

use crate::types::{PhysicalUnit, Type};

/// Source span for error reporting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

impl Span {
    pub fn new(start: usize, end: usize) -> Self {
        Self { start, end }
    }

    pub fn dummy() -> Self {
        Self { start: 0, end: 0 }
    }
}

impl std::fmt::Display for Span {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}..{}", self.start, self.end)
    }
}

/// Binary operators.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinOp {
    // Arithmetic
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    // Comparison
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    // Logical
    And,
    Or,
    Implies,
    // String
    Concat,
    // Pipeline
    Pipe,
}

/// Unary operators.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnaryOp {
    Neg,
    Not,
}

/// A statement inside a do-block.
#[derive(Debug, Clone, PartialEq)]
pub enum DoStatement {
    Bind { name: String, expr: Expr },
    Let { name: String, expr: Expr },
    Expr(Expr),
    Yield(Expr),
}

impl BinOp {
    /// Returns true if this is an arithmetic operator.
    pub fn is_arithmetic(self) -> bool {
        matches!(self, BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Mod)
    }

    /// Returns true if this is a comparison operator.
    pub fn is_comparison(self) -> bool {
        matches!(self, BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge)
    }

    /// Returns true if this is a logical operator.
    pub fn is_logical(self) -> bool {
        matches!(self, BinOp::And | BinOp::Or | BinOp::Implies)
    }
}

/// A minimal expression type for type checking.
#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    /// Integer literal: `42`
    IntLit {
        value: i64,
        span: Span,
    },
    /// Float literal: `3.14`
    FloatLit {
        value: f64,
        span: Span,
    },
    /// String literal: `"hello"`
    StringLit {
        value: String,
        span: Span,
    },
    /// Boolean literal: `true` / `false`
    BoolLit {
        value: bool,
        span: Span,
    },
    /// Unit literal: `()`
    UnitLit {
        span: Span,
    },
    /// Variable reference: `x`
    Var {
        name: String,
        span: Span,
    },
    /// Let binding: `let x: T = value in body`
    Let {
        name: String,
        annotation: Option<Type>,
        value: Box<Expr>,
        body: Box<Expr>,
        span: Span,
    },
    /// Function application: `f(args...)`
    Apply {
        func: Box<Expr>,
        args: Vec<Expr>,
        span: Span,
    },
    /// Lambda / anonymous function: `fn(params) -> body`
    Lambda {
        params: Vec<(String, Type)>,
        body: Box<Expr>,
        span: Span,
    },
    /// Binary operation: `lhs op rhs`
    BinOp {
        op: BinOp,
        lhs: Box<Expr>,
        rhs: Box<Expr>,
        span: Span,
    },
    /// Record construction: `{ field1: expr1, field2: expr2 }`
    Record {
        fields: Vec<(String, Expr)>,
        span: Span,
    },
    /// Field access: `expr.field`
    FieldAccess {
        expr: Box<Expr>,
        field: String,
        span: Span,
    },
    /// Unit-annotated literal: `200.ms`, `4.GiB`
    WithUnit {
        expr: Box<Expr>,
        unit: PhysicalUnit,
        span: Span,
    },
    /// If-then-else: `if cond then t else e`
    If {
        cond: Box<Expr>,
        then_branch: Box<Expr>,
        else_branch: Box<Expr>,
        span: Span,
    },
    /// Array literal: `[expr1, expr2, ...]`
    Array {
        elements: Vec<Expr>,
        span: Span,
    },
    /// Index access: `expr[index]`
    Index {
        expr: Box<Expr>,
        index: Box<Expr>,
        span: Span,
    },
    /// Match expression: `match expr { pat -> body, ... }`
    Match {
        expr: Box<Expr>,
        arms: Vec<MatchArm>,
        span: Span,
    },
    /// Block expression: sequence of expressions, last is the value.
    Block {
        exprs: Vec<Expr>,
        span: Span,
    },
    /// Unary operation: `-x`, `not x`
    UnaryOp {
        op: UnaryOp,
        operand: Box<Expr>,
        span: Span,
    },
    /// Type ascription: `expr : Type`
    Ascription {
        expr: Box<Expr>,
        ty: Type,
        span: Span,
    },
    /// Do-block (monadic sequencing): `do { stmts... }`
    DoBlock {
        stmts: Vec<DoStatement>,
        span: Span,
    },
    /// Range expression: `start..end`
    Range {
        start: Box<Expr>,
        end: Box<Expr>,
        span: Span,
    },
    /// Slice: `expr[start:end]`
    Slice {
        expr: Box<Expr>,
        start: Option<Box<Expr>>,
        end: Option<Box<Expr>>,
        span: Span,
    },
}

/// A match arm in the type checker AST.
#[derive(Debug, Clone, PartialEq)]
pub struct MatchArm {
    pub pattern: Pattern,
    pub body: Expr,
}

/// Pattern in the type checker AST.
#[derive(Debug, Clone, PartialEq)]
pub enum Pattern {
    Wildcard,
    Ident(String),
    Literal(Expr),
    Constructor(String, Vec<Pattern>),
}

impl Expr {
    /// Returns the span of this expression.
    pub fn span(&self) -> Span {
        match self {
            Expr::IntLit { span, .. }
            | Expr::FloatLit { span, .. }
            | Expr::StringLit { span, .. }
            | Expr::BoolLit { span, .. }
            | Expr::UnitLit { span }
            | Expr::Var { span, .. }
            | Expr::Let { span, .. }
            | Expr::Apply { span, .. }
            | Expr::Lambda { span, .. }
            | Expr::BinOp { span, .. }
            | Expr::Record { span, .. }
            | Expr::FieldAccess { span, .. }
            | Expr::WithUnit { span, .. }
            | Expr::If { span, .. }
            | Expr::Array { span, .. }
            | Expr::Index { span, .. }
            | Expr::Match { span, .. }
            | Expr::Block { span, .. }
            | Expr::UnaryOp { span, .. }
            | Expr::Ascription { span, .. }
            | Expr::DoBlock { span, .. }
            | Expr::Range { span, .. }
            | Expr::Slice { span, .. } => *span,
        }
    }
}
