//! AST types for the Lattice surface syntax.
//!
//! These types represent the parsed structure of `.lattice` files
//! before conversion to the Binary Semantic Graph (BSG) format.
//! Every node carries a [`Span`] for error reporting.

use serde::{Deserialize, Serialize};

// ── Span tracking ──────────────────────────

/// Source location span (byte offsets).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Span {
    pub start: usize, // byte offset
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

/// Wrapper that pairs any AST node with its source span.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Spanned<T> {
    pub node: T,
    pub span: Span,
}

impl<T> Spanned<T> {
    pub fn new(node: T, span: Span) -> Self {
        Self { node, span }
    }

    pub fn dummy(node: T) -> Self {
        Self {
            node,
            span: Span::dummy(),
        }
    }
}

// ── Top-level Program ──────────────────────

/// A complete Lattice source file.
pub type Program = Vec<Spanned<Item>>;

/// Top-level item.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Item {
    Graph(Graph),
    Function(Function),
    TypeDef(TypeDef),
    LetBinding(LetBinding),
    Module(Module),
    Model(Model),
    Meta(Meta),
    Import(Import),
}

// ── Graph ──────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Graph {
    pub name: String,
    pub version: Option<String>,
    pub targets: Vec<String>,
    pub members: Vec<Spanned<GraphMember>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum GraphMember {
    Node(NodeDef),
    Edge(EdgeDef),
    Solve(SolveBlock),
}

// ── Node ───────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeDef {
    pub name: String,
    pub fields: Vec<NodeField>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum NodeField {
    Input(Spanned<TypeExpr>),
    Output(Spanned<TypeExpr>),
    Properties(Vec<Property>),
    Semantic(SemanticBlock),
    ProofObligations(Vec<ProofObligation>),
    Pre(Vec<Spanned<Expr>>),
    Post(Vec<Spanned<Expr>>),
    Solve(SolveBlock),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Property {
    pub key: String,
    pub value: Spanned<Expr>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SemanticBlock {
    pub description: Option<String>,
    pub formal: Option<Spanned<Expr>>,
    pub examples: Vec<Example>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Example {
    pub input: Spanned<Expr>,
    pub output: Spanned<Expr>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProofObligation {
    pub name: String,
    pub expr: Spanned<Expr>,
}

// ── Edge ───────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EdgeDef {
    pub from: String,
    pub to: String,
    pub properties: Vec<Property>,
}

// ── Solve / Intent Block ───────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SolveBlock {
    pub goal: Option<Spanned<Expr>>,
    pub constraints: Vec<Spanned<Expr>>,
    pub invariants: Vec<Spanned<Expr>>,
    pub domain: Option<DomainBlock>,
    pub strategy: Option<Spanned<Expr>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DomainBlock {
    pub kind: String,
    pub config: Vec<Property>,
}

// ── Function ───────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Function {
    pub name: String,
    pub params: Vec<Param>,
    pub return_type: Option<Spanned<TypeExpr>>,
    pub pre: Vec<Spanned<Expr>>,
    pub post: Vec<Spanned<Expr>>,
    pub invariants: Vec<Spanned<Expr>>,
    pub body: FunctionBody,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Param {
    pub name: String,
    pub type_expr: Spanned<TypeExpr>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum FunctionBody {
    Synthesize(Vec<Property>),
    Block(Vec<Spanned<Expr>>),
}

// ── Type Definitions ───────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TypeDef {
    pub name: String,
    pub params: Vec<TypeParam>,
    pub body: Spanned<TypeExpr>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TypeParam {
    pub name: String,
    pub bound: Option<Spanned<TypeExpr>>,
}

// ── Type Expressions ───────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TypeExpr {
    /// Simple named type: `Int`, `String`, `Bool`.
    Named(String),
    /// Generic application: `List<T>`, `Result<T, E>`.
    Applied {
        name: String,
        args: Vec<Spanned<TypeExpr>>,
    },
    /// Function type: `A -> B`.
    Function {
        params: Vec<Spanned<TypeExpr>>,
        ret: Box<Spanned<TypeExpr>>,
    },
    /// Product / record: `{ name: String, age: Int }`.
    Record(Vec<(String, Spanned<TypeExpr>)>),
    /// Sum type: `Ok(T) | Err(E)`.
    Sum(Vec<Variant>),
    /// Refinement type: `{ x ∈ T | predicate }`.
    Refinement {
        var: String,
        base: Box<Spanned<TypeExpr>>,
        predicate: Box<Spanned<Expr>>,
    },
    /// Dependent: `Vector(n: Nat)`.
    Dependent {
        name: String,
        params: Vec<(String, Spanned<TypeExpr>)>,
    },
    /// `Stream<T>`.
    Stream(Box<Spanned<TypeExpr>>),
    /// `Distribution<T>`.
    Distribution(Box<Spanned<TypeExpr>>),
    /// Where clause: `T where constraint`.
    Where {
        base: Box<Spanned<TypeExpr>>,
        constraint: Box<Spanned<Expr>>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Variant {
    pub name: String,
    pub fields: Vec<(String, Spanned<TypeExpr>)>,
}

// ── Expressions ────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Expr {
    // Literals
    IntLit(i64),
    FloatLit(f64),
    StringLit(String),
    BoolLit(bool),

    // Identifier
    Ident(String),

    // Binary operation: a + b, a ∧ b, etc.
    BinOp {
        left: Box<Spanned<Expr>>,
        op: BinOp,
        right: Box<Spanned<Expr>>,
    },

    // Unary operation: -x, ¬x
    UnaryOp {
        op: UnaryOp,
        operand: Box<Spanned<Expr>>,
    },

    // Function call: f(a, b, c)
    Call {
        func: Box<Spanned<Expr>>,
        args: Vec<Spanned<Expr>>,
    },

    // Named arguments: f(key: value, ...)
    CallNamed {
        func: Box<Spanned<Expr>>,
        args: Vec<(String, Spanned<Expr>)>,
    },

    // Field access: expr.field
    Field {
        expr: Box<Spanned<Expr>>,
        name: String,
    },

    // Index: expr[idx]
    Index {
        expr: Box<Spanned<Expr>>,
        index: Box<Spanned<Expr>>,
    },

    // Slice: expr[start:end]
    Slice {
        expr: Box<Spanned<Expr>>,
        start: Option<Box<Spanned<Expr>>>,
        end: Option<Box<Spanned<Expr>>>,
    },

    // Pipeline: expr |> expr
    Pipeline {
        left: Box<Spanned<Expr>>,
        right: Box<Spanned<Expr>>,
    },

    // Lambda: λ params → body  OR  fn(params) -> body
    Lambda {
        params: Vec<Param>,
        body: Box<Spanned<Expr>>,
    },

    // Let binding expression
    Let {
        name: String,
        type_ann: Option<Spanned<TypeExpr>>,
        value: Box<Spanned<Expr>>,
    },

    // Record literal: { key: value, ... }
    Record(Vec<(String, Spanned<Expr>)>),

    // Array literal: [a, b, c]
    Array(Vec<Spanned<Expr>>),

    // Unit annotation: 200.ms, 4.GiB
    WithUnit {
        value: Box<Spanned<Expr>>,
        unit: String,
    },

    // Relational algebra operations
    Select {
        predicate: Box<Spanned<Expr>>,
        relation: Box<Spanned<Expr>>,
    },
    Project {
        fields: Vec<String>,
        relation: Box<Spanned<Expr>>,
    },
    Join {
        left: Box<Spanned<Expr>>,
        condition: Box<Spanned<Expr>>,
        right: Box<Spanned<Expr>>,
    },
    GroupBy {
        keys: Vec<String>,
        aggregates: Vec<Spanned<Expr>>,
        relation: Box<Spanned<Expr>>,
    },

    // Do-block (monadic)
    DoBlock(Vec<Spanned<DoStatement>>),

    // Quantifiers
    ForAll {
        var: String,
        domain: Box<Spanned<Expr>>,
        body: Box<Spanned<Expr>>,
    },
    Exists {
        var: String,
        domain: Box<Spanned<Expr>>,
        body: Box<Spanned<Expr>>,
    },

    // Branch (probabilistic)
    Branch {
        expr: Box<Spanned<Expr>>,
        arms: Vec<BranchArm>,
    },

    // Match
    Match {
        expr: Box<Spanned<Expr>>,
        arms: Vec<MatchArm>,
    },

    // If expression
    If {
        cond: Box<Spanned<Expr>>,
        then_: Box<Spanned<Expr>>,
        else_: Option<Box<Spanned<Expr>>>,
    },

    // Block
    Block(Vec<Spanned<Expr>>),

    // Synthesize
    Synthesize(Vec<Property>),

    // Type ascription: expr : Type
    Ascription {
        expr: Box<Spanned<Expr>>,
        type_expr: Spanned<TypeExpr>,
    },

    // Range: start..end
    Range {
        start: Box<Spanned<Expr>>,
        end: Box<Spanned<Expr>>,
    },

    // Try operator: expr?
    Try(Box<Spanned<Expr>>),

    // Yield (in do-blocks)
    Yield(Box<Spanned<Expr>>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BinOp {
    // Arithmetic
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    // Comparison
    Eq,
    Neq,
    Lt,
    Gt,
    Leq,
    Geq,
    // Logical
    And,
    Or,
    Implies,
    // Set membership
    In,
    NotIn,
    // Assignment-like
    Assign,
    // Concatenation
    Concat,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum UnaryOp {
    Neg, // -x
    Not, // ¬x / not x
}

// ── Do-block statements ────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DoStatement {
    Bind {
        name: String,
        expr: Spanned<Expr>,
    }, // x ← expr?
    Let {
        name: String,
        expr: Spanned<Expr>,
    }, // let x = expr
    Expr(Spanned<Expr>),  // expr
    Yield(Spanned<Expr>), // yield expr
}

// ── Branch/Match arms ──────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BranchArm {
    pub pattern: Spanned<Pattern>,
    pub guard: Option<Spanned<Expr>>,
    pub body: Spanned<Expr>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MatchArm {
    pub pattern: Spanned<Pattern>,
    pub guard: Option<Spanned<Expr>>,
    pub body: Spanned<Expr>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Pattern {
    Wildcard,                                       // _
    Ident(String),                                  // x
    Constructor(String, Vec<Spanned<Pattern>>),     // Ok(x)
    Literal(Spanned<Expr>),                         // 42, "hello"
    Record(Vec<(String, Spanned<Pattern>)>),        // { name: p, age: q }
}

// ── Let binding (top-level) ────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LetBinding {
    pub name: String,
    pub type_ann: Option<Spanned<TypeExpr>>,
    pub value: Spanned<Expr>,
}

// ── Module ─────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Module {
    pub name: String,
    pub items: Vec<Spanned<Item>>,
}

// ── Import ─────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Import {
    /// Dotted module path, e.g. `["std", "math"]`.
    pub path: Vec<String>,
    /// Selective imports. `None` means import all (`import std.math`),
    /// `Some(names)` means selective (`import std.math.{sin, cos}`).
    pub names: Option<Vec<ImportName>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportName {
    pub name: String,
    pub alias: Option<String>,
}

// ── Probabilistic Model ────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Model {
    pub name: String,
    pub statements: Vec<Spanned<ModelStatement>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ModelStatement {
    Prior {
        name: String,
        distribution: Spanned<Expr>,
    },
    Observe {
        name: String,
        distribution: Spanned<Expr>,
    },
    Posterior(Spanned<Expr>),
}

// ── Meta / Self-modification ───────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Meta {
    pub name: String,
    pub target: Spanned<Expr>,
    pub body: Vec<MetaField>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetaField {
    pub key: String,
    pub value: Spanned<Expr>,
}

// ── Level annotation ───────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Annotation {
    pub name: String,
    pub args: Vec<String>,
}
