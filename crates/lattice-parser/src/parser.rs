//! Hand-written recursive-descent parser for Lattice surface syntax.
//!
//! Contains a lexer (tokenizer) and a top-down parser that produces
//! the AST types defined in [`crate::ast`].

use crate::ast::*;

// ─── Public API ──────────────────────────────────────────

/// Parse Lattice source code into an AST [`Program`].
///
/// Returns collected errors if any syntax issues are found.
pub fn parse(source: &str) -> Result<Program, Vec<ParseError>> {
    let tokens = Lexer::new(source).tokenize();
    let mut parser = Parser::new(tokens);
    let program = parser.parse_program();
    if parser.errors.is_empty() {
        Ok(program)
    } else {
        Err(parser.errors)
    }
}

/// Parse a single Lattice expression from source text.
pub fn parse_expression(source: &str) -> Result<Expr, Vec<ParseError>> {
    let tokens = Lexer::new(source).tokenize();
    let mut parser = Parser::new(tokens);
    let expr = parser.parse_expr(0);
    if parser.errors.is_empty() {
        Ok(expr)
    } else {
        Err(parser.errors)
    }
}

/// A parse error with location information.
#[derive(Debug, Clone)]
pub struct ParseError {
    pub offset: usize,
    pub message: String,
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "offset {}: {}", self.offset, self.message)
    }
}

impl std::error::Error for ParseError {}

impl ParseError {
    /// Render this error as a caret-style diagnostic using the original source.
    pub fn render(&self, source: &str, filename: Option<&str>) -> String {
        let sm = crate::diagnostic::SourceMap::new(source);
        let mut diag = crate::diagnostic::Diagnostic::error(&self.message, self.offset);
        if let Some(f) = filename {
            diag = diag.with_filename(f);
        }
        diag.render(&sm)
    }
}

// ─── Token ───────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
enum Token {
    // Literals
    Int(i64),
    Float(f64),
    Str(String),

    // Identifier
    Ident(String),

    // Keywords
    Graph,
    Node,
    Edge,
    Solve,
    Function,
    Type,
    Let,
    Module,
    Import,
    Model,
    Meta,
    Pre,
    Post,
    Invariant,
    Constraint,
    Goal,
    Domain,
    Strategy,
    Input,
    Output,
    Properties,
    Semantic,
    ProofObligations,
    Description,
    Formal,
    Synthesize,
    Do,
    Yield,
    Branch,
    Prior,
    Observe,
    Posterior,
    Where,
    If,
    Then,
    Else,
    Match,
    True,
    False,
    ForAll,
    Exists,
    Fn,
    Select,
    Project,
    Join,
    GroupBy,
    As,
    Version,
    Target,
    Not,

    // Operators
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    Eq,
    EqEq,
    Neq,
    Lt,
    Gt,
    Leq,
    Geq,
    Arrow,     // ->
    BackArrow, // <-
    Pipe,      // |>
    PipeSep,   // |
    Concat,    // ++
    DotDot,    // ..
    Tilde,     // ~
    At,        // @
    Question,  // ?

    // Unicode operators
    Sigma,        // σ
    Pi,           // π
    Bowtie,       // ⋈
    Gamma,        // γ
    Lambda,       // λ
    ForAllSym,    // ∀
    ExistsSym,    // ∃
    InSym,        // ∈
    NotInSym,     // ∉
    ImpliesSym,   // ⟹
    AndSym,       // ∧
    OrSym,        // ∨
    LeqSym,       // ≤
    GeqSym,       // ≥
    NeqSym,       // ≠
    ArrowSym,     // →
    BackArrowSym, // ←
    ApproxSym,    // ≈

    // Delimiters
    LParen,
    RParen,
    LBrace,
    RBrace,
    LBracket,
    RBracket,

    // Punctuation
    Colon,
    ColonColon,
    Comma,
    Dot,
    Semi,

    // Special
    Eof,
}

/// A token together with its source span.
#[derive(Debug, Clone)]
struct Tok {
    token: Token,
    span: Span,
}

// ─── Lexer ───────────────────────────────────────────────

struct Lexer {
    chars: Vec<char>,
    pos: usize,
}

impl Lexer {
    fn new(source: &str) -> Self {
        Self {
            chars: source.chars().collect(),
            pos: 0,
        }
    }

    fn byte_offset(&self) -> usize {
        // Convert char position to byte offset
        self.chars[..self.pos.min(self.chars.len())]
            .iter()
            .map(|c| c.len_utf8())
            .sum()
    }

    fn peek(&self) -> Option<char> {
        self.chars.get(self.pos).copied()
    }

    fn peek2(&self) -> Option<char> {
        self.chars.get(self.pos + 1).copied()
    }

    fn advance(&mut self) -> Option<char> {
        let c = self.chars.get(self.pos).copied();
        if c.is_some() {
            self.pos += 1;
        }
        c
    }

    fn tokenize(mut self) -> Vec<Tok> {
        let mut tokens = Vec::new();
        loop {
            self.skip_whitespace_and_comments();
            let start = self.byte_offset();
            match self.peek() {
                None => {
                    tokens.push(Tok {
                        token: Token::Eof,
                        span: Span::new(start, start),
                    });
                    break;
                }
                Some(c) => {
                    let tok = self.lex_token(c);
                    let end = self.byte_offset();
                    tokens.push(Tok {
                        token: tok,
                        span: Span::new(start, end),
                    });
                }
            }
        }
        tokens
    }

    fn skip_whitespace_and_comments(&mut self) {
        loop {
            match self.peek() {
                Some(' ') | Some('\t') | Some('\r') => {
                    self.advance();
                }
                Some('\n') => {
                    // We don't emit newline tokens; whitespace is insignificant
                    self.advance();
                }
                Some('-') if self.peek2() == Some('-') => {
                    // Line comment
                    self.advance();
                    self.advance();
                    while let Some(c) = self.peek() {
                        if c == '\n' {
                            break;
                        }
                        self.advance();
                    }
                }
                _ => break,
            }
        }
    }

    fn lex_token(&mut self, c: char) -> Token {
        match c {
            '"' => self.lex_string(),
            '0'..='9' => self.lex_number(),
            'a'..='z' | 'A'..='Z' | '_' => self.lex_ident(),

            '+' => {
                self.advance();
                if self.peek() == Some('+') {
                    self.advance();
                    Token::Concat
                } else {
                    Token::Plus
                }
            }
            '-' => {
                self.advance();
                if self.peek() == Some('>') {
                    self.advance();
                    Token::Arrow
                } else {
                    Token::Minus
                }
            }
            '*' => {
                self.advance();
                Token::Star
            }
            '/' => {
                self.advance();
                Token::Slash
            }
            '%' => {
                self.advance();
                Token::Percent
            }
            '=' => {
                self.advance();
                if self.peek() == Some('=') {
                    self.advance();
                    Token::EqEq
                } else {
                    Token::Eq
                }
            }
            '!' => {
                self.advance();
                if self.peek() == Some('=') {
                    self.advance();
                    Token::Neq
                } else {
                    // lone '!' — treat as not
                    Token::Not
                }
            }
            '<' => {
                self.advance();
                if self.peek() == Some('=') {
                    self.advance();
                    Token::Leq
                } else if self.peek() == Some('-') {
                    self.advance();
                    Token::BackArrow
                } else {
                    Token::Lt
                }
            }
            '>' => {
                self.advance();
                if self.peek() == Some('=') {
                    self.advance();
                    Token::Geq
                } else {
                    Token::Gt
                }
            }
            '|' => {
                self.advance();
                if self.peek() == Some('>') {
                    self.advance();
                    Token::Pipe
                } else {
                    Token::PipeSep
                }
            }
            '~' => {
                self.advance();
                Token::Tilde
            }
            '@' => {
                self.advance();
                Token::At
            }
            '?' => {
                self.advance();
                Token::Question
            }
            '(' => {
                self.advance();
                Token::LParen
            }
            ')' => {
                self.advance();
                Token::RParen
            }
            '{' => {
                self.advance();
                Token::LBrace
            }
            '}' => {
                self.advance();
                Token::RBrace
            }
            '[' => {
                self.advance();
                Token::LBracket
            }
            ']' => {
                self.advance();
                Token::RBracket
            }
            ':' => {
                self.advance();
                if self.peek() == Some(':') {
                    self.advance();
                    Token::ColonColon
                } else {
                    Token::Colon
                }
            }
            ',' => {
                self.advance();
                Token::Comma
            }
            '.' => {
                self.advance();
                if self.peek() == Some('.') {
                    self.advance();
                    Token::DotDot
                } else {
                    Token::Dot
                }
            }
            ';' => {
                self.advance();
                Token::Semi
            }

            // Unicode operators
            'σ' => {
                self.advance();
                Token::Sigma
            }
            'π' => {
                self.advance();
                Token::Pi
            }
            '⋈' => {
                self.advance();
                Token::Bowtie
            }
            'γ' => {
                self.advance();
                Token::Gamma
            }
            'λ' => {
                self.advance();
                Token::Lambda
            }
            '∀' => {
                self.advance();
                Token::ForAllSym
            }
            '∃' => {
                self.advance();
                Token::ExistsSym
            }
            '∈' => {
                self.advance();
                Token::InSym
            }
            '∉' => {
                self.advance();
                Token::NotInSym
            }
            '⟹' => {
                self.advance();
                Token::ImpliesSym
            }
            '∧' => {
                self.advance();
                Token::AndSym
            }
            '∨' => {
                self.advance();
                Token::OrSym
            }
            '≤' => {
                self.advance();
                Token::LeqSym
            }
            '≥' => {
                self.advance();
                Token::GeqSym
            }
            '≠' => {
                self.advance();
                Token::NeqSym
            }
            '→' => {
                self.advance();
                Token::ArrowSym
            }
            '←' => {
                self.advance();
                Token::BackArrowSym
            }
            '≈' => {
                self.advance();
                Token::ApproxSym
            }
            // Unicode math letters used as identifiers
            'ℝ' | 'ℤ' | 'ℕ' | 'Σ' | 'Π' => {
                self.advance();
                let name = match c {
                    'ℝ' => "Real",
                    'ℤ' => "Int",
                    'ℕ' => "Nat",
                    'Σ' => "sum",
                    'Π' => "prod",
                    _ => unreachable!(),
                };
                Token::Ident(name.to_string())
            }

            _ => {
                // Skip unknown character
                self.advance();
                Token::Ident(format!("<unknown:{}>", c))
            }
        }
    }

    fn lex_string(&mut self) -> Token {
        self.advance(); // skip opening "
        let mut s = String::new();
        loop {
            match self.advance() {
                None | Some('\n') => break,
                Some('"') => break,
                Some('\\') => {
                    if let Some(esc) = self.advance() {
                        match esc {
                            'n' => s.push('\n'),
                            't' => s.push('\t'),
                            '\\' => s.push('\\'),
                            '"' => s.push('"'),
                            _ => {
                                s.push('\\');
                                s.push(esc);
                            }
                        }
                    }
                }
                Some(c) => s.push(c),
            }
        }
        Token::Str(s)
    }

    fn lex_number(&mut self) -> Token {
        let mut num = String::new();
        let mut is_float = false;

        while let Some(c) = self.peek() {
            if c.is_ascii_digit() {
                num.push(c);
                self.advance();
            } else if c == '.' && self.peek2().map_or(false, |c2| c2.is_ascii_digit()) {
                is_float = true;
                num.push(c);
                self.advance();
            } else {
                break;
            }
        }

        if is_float {
            Token::Float(num.parse().unwrap_or(0.0))
        } else {
            Token::Int(num.parse().unwrap_or(0))
        }
    }

    fn lex_ident(&mut self) -> Token {
        let mut ident = String::new();
        while let Some(c) = self.peek() {
            if c.is_alphanumeric() || c == '_' {
                ident.push(c);
                self.advance();
            } else {
                break;
            }
        }
        match ident.as_str() {
            "graph" => Token::Graph,
            "node" => Token::Node,
            "edge" => Token::Edge,
            "solve" => Token::Solve,
            "function" => Token::Function,
            "type" => Token::Type,
            "let" => Token::Let,
            "module" => Token::Module,
            "import" => Token::Import,
            "model" => Token::Model,
            "meta" => Token::Meta,
            "pre" => Token::Pre,
            "post" => Token::Post,
            "invariant" => Token::Invariant,
            "constraint" => Token::Constraint,
            "goal" => Token::Goal,
            "domain" => Token::Domain,
            "strategy" => Token::Strategy,
            "input" => Token::Input,
            "output" => Token::Output,
            "properties" => Token::Properties,
            "semantic" => Token::Semantic,
            "proof_obligations" => Token::ProofObligations,
            "description" => Token::Description,
            "formal" => Token::Formal,
            "synthesize" => Token::Synthesize,
            "do" => Token::Do,
            "yield" => Token::Yield,
            "branch" => Token::Branch,
            "prior" => Token::Prior,
            "observe" => Token::Observe,
            "posterior" => Token::Posterior,
            "where" => Token::Where,
            "if" => Token::If,
            "then" => Token::Then,
            "else" => Token::Else,
            "match" => Token::Match,
            "true" => Token::True,
            "false" => Token::False,
            "forall" => Token::ForAll,
            "exists" => Token::Exists,
            "fn" => Token::Fn,
            "select" | "sigma" => Token::Select,
            "project" => Token::Project,
            "join" => Token::Join,
            "group_by" => Token::GroupBy,
            "as" => Token::As,
            "version" => Token::Version,
            "target" => Token::Target,
            "and" => Token::AndSym,
            "or" => Token::OrSym,
            "not" => Token::Not,
            "implies" => Token::ImpliesSym,
            "in" => Token::InSym,
            "not_in" => Token::NotInSym,
            _ => Token::Ident(ident),
        }
    }
}

// ─── Parser ──────────────────────────────────────────────

struct Parser {
    tokens: Vec<Tok>,
    pos: usize,
    errors: Vec<ParseError>,
}

impl Parser {
    fn new(tokens: Vec<Tok>) -> Self {
        Self {
            tokens,
            pos: 0,
            errors: Vec::new(),
        }
    }

    // ── Helpers ──────────────────────────────

    fn peek_token(&self) -> &Token {
        self.tokens
            .get(self.pos)
            .map(|t| &t.token)
            .unwrap_or(&Token::Eof)
    }

    fn current_span(&self) -> Span {
        self.tokens
            .get(self.pos)
            .map(|t| t.span.clone())
            .unwrap_or_else(Span::dummy)
    }

    fn advance(&mut self) -> Tok {
        let tok = self.tokens.get(self.pos).cloned().unwrap_or(Tok {
            token: Token::Eof,
            span: Span::dummy(),
        });
        if self.pos < self.tokens.len() {
            self.pos += 1;
        }
        tok
    }

    fn expect(&mut self, expected: &Token) -> Span {
        if self.peek_token() == expected {
            self.advance().span
        } else {
            self.error(format!("expected {:?}, found {:?}", expected, self.peek_token()));
            self.current_span()
        }
    }

    fn expect_ident(&mut self) -> String {
        if let Token::Ident(name) = self.peek_token().clone() {
            self.advance();
            name
        } else if let Some(kw) = self.keyword_as_ident() {
            self.advance();
            kw
        } else {
            self.error(format!("expected identifier, found {:?}", self.peek_token()));
            "<error>".to_string()
        }
    }

    /// Convert a keyword token to an identifier string when it appears in
    /// a position that accepts identifiers (e.g., property keys, field names).
    fn keyword_as_ident(&self) -> Option<String> {
        let s = match self.peek_token() {
            Token::Strategy => "strategy",
            Token::Goal => "goal",
            Token::Domain => "domain",
            Token::Constraint => "constraint",
            Token::Version => "version",
            Token::Target => "target",
            Token::Input => "input",
            Token::Output => "output",
            Token::Properties => "properties",
            Token::Semantic => "semantic",
            Token::Description => "description",
            Token::Formal => "formal",
            Token::Synthesize => "synthesize",
            Token::Pre => "pre",
            Token::Post => "post",
            Token::Invariant => "invariant",
            Token::Prior => "prior",
            Token::Observe => "observe",
            Token::Posterior => "posterior",
            Token::ProofObligations => "proof_obligations",
            Token::Graph => "graph",
            Token::Node => "node",
            Token::Edge => "edge",
            Token::Solve => "solve",
            Token::Function => "function",
            Token::Type => "type",
            Token::Let => "let",
            Token::Module => "module",
            Token::Import => "import",
            Token::Model => "model",
            Token::Meta => "meta",
            Token::Do => "do",
            Token::Yield => "yield",
            Token::Branch => "branch",
            Token::Where => "where",
            Token::If => "if",
            Token::Else => "else",
            Token::Match => "match",
            Token::True => "true",
            Token::False => "false",
            Token::ForAll => "forall",
            Token::Exists => "exists",
            Token::Fn => "fn",
            Token::Select => "select",
            Token::Project => "project",
            Token::Join => "join",
            Token::GroupBy => "group_by",
            Token::As => "as",
            Token::Not => "not",
            _ => return None,
        };
        Some(s.to_string())
    }

    fn eat(&mut self, token: &Token) -> bool {
        if self.peek_token() == token {
            self.advance();
            true
        } else {
            false
        }
    }

    fn error(&mut self, message: String) {
        let offset = self.current_span().start;
        self.errors.push(ParseError { offset, message });
    }

    fn spanned<T>(&self, node: T, start: usize) -> Spanned<T> {
        let end = if self.pos > 0 {
            self.tokens
                .get(self.pos - 1)
                .map(|t| t.span.end)
                .unwrap_or(start)
        } else {
            start
        };
        Spanned::new(node, Span::new(start, end))
    }

    /// Skip tokens until we find something that looks like a sync point.
    fn synchronize(&mut self) {
        loop {
            match self.peek_token() {
                Token::Graph
                | Token::Node
                | Token::Edge
                | Token::Solve
                | Token::Function
                | Token::Type
                | Token::Let
                | Token::Module
                | Token::Model
                | Token::Meta
                | Token::RBrace
                | Token::Eof => break,
                _ => {
                    self.advance();
                }
            }
        }
    }

    // ── Program ─────────────────────────────

    fn parse_program(&mut self) -> Program {
        let mut items = Vec::new();
        while *self.peek_token() != Token::Eof {
            let start = self.current_span().start;
            match self.parse_item() {
                Some(item) => items.push(self.spanned(item, start)),
                None => self.synchronize(),
            }
        }
        items
    }

    fn parse_item(&mut self) -> Option<Item> {
        match self.peek_token().clone() {
            Token::Graph => Some(Item::Graph(self.parse_graph())),
            Token::Function => Some(Item::Function(self.parse_function())),
            Token::Type => Some(Item::TypeDef(self.parse_type_def())),
            Token::Let => Some(Item::LetBinding(self.parse_let_binding())),
            Token::Module => Some(Item::Module(self.parse_module())),
            Token::Import => Some(Item::Import(self.parse_import())),
            Token::Model => Some(Item::Model(self.parse_model())),
            Token::Meta => Some(Item::Meta(self.parse_meta())),
            Token::At => {
                // Annotation — skip it, parse next item
                self.advance(); // @
                self.expect_ident(); // annotation name
                if self.eat(&Token::LParen) {
                    self.skip_until_balanced_paren();
                }
                self.parse_item()
            }
            _ => {
                self.error(format!(
                    "expected top-level item (graph, function, type, let, module, import, model, meta), found {:?}",
                    self.peek_token()
                ));
                None
            }
        }
    }

    fn skip_until_balanced_paren(&mut self) {
        let mut depth = 1;
        while depth > 0 {
            match self.peek_token() {
                Token::LParen => {
                    depth += 1;
                    self.advance();
                }
                Token::RParen => {
                    depth -= 1;
                    self.advance();
                }
                Token::Eof => break,
                _ => {
                    self.advance();
                }
            }
        }
    }

    // ── Graph ───────────────────────────────

    fn parse_graph(&mut self) -> Graph {
        self.advance(); // consume 'graph'
        let name = self.expect_ident();
        self.expect(&Token::LBrace);

        let mut version = None;
        let mut targets = Vec::new();
        let mut members = Vec::new();

        while *self.peek_token() != Token::RBrace && *self.peek_token() != Token::Eof {
            let start = self.current_span().start;
            match self.peek_token().clone() {
                Token::Version => {
                    self.advance();
                    self.expect(&Token::Colon);
                    if let Token::Str(v) = self.peek_token().clone() {
                        self.advance();
                        version = Some(v);
                    }
                }
                Token::Target => {
                    self.advance();
                    self.expect(&Token::Colon);
                    targets = self.parse_target_list();
                }
                Token::Node => {
                    let node = self.parse_node_def();
                    members.push(self.spanned(GraphMember::Node(node), start));
                }
                Token::Edge => {
                    let edge = self.parse_edge_def();
                    members.push(self.spanned(GraphMember::Edge(edge), start));
                }
                Token::Solve => {
                    let solve = self.parse_solve_block();
                    members.push(self.spanned(GraphMember::Solve(solve), start));
                }
                _ => {
                    self.error(format!(
                        "unexpected token in graph body: {:?}",
                        self.peek_token()
                    ));
                    self.advance();
                }
            }
        }
        self.eat(&Token::RBrace);

        Graph {
            name,
            version,
            targets,
            members,
        }
    }

    fn parse_target_list(&mut self) -> Vec<String> {
        let mut targets = Vec::new();
        if self.eat(&Token::LBracket) {
            loop {
                if *self.peek_token() == Token::RBracket || *self.peek_token() == Token::Eof {
                    break;
                }
                targets.push(self.expect_ident());
                if !self.eat(&Token::Comma) {
                    break;
                }
            }
            self.expect(&Token::RBracket);
        } else {
            targets.push(self.expect_ident());
        }
        targets
    }

    // ── Node ────────────────────────────────

    fn parse_node_def(&mut self) -> NodeDef {
        self.advance(); // consume 'node'
        let name = self.expect_ident();
        self.expect(&Token::LBrace);

        let mut fields = Vec::new();

        while *self.peek_token() != Token::RBrace && *self.peek_token() != Token::Eof {
            match self.peek_token().clone() {
                Token::Input => {
                    self.advance();
                    self.expect(&Token::Colon);
                    let start = self.current_span().start;
                    let ty = self.parse_type_expr();
                    fields.push(NodeField::Input(self.spanned(ty, start)));
                }
                Token::Output => {
                    self.advance();
                    self.expect(&Token::Colon);
                    let start = self.current_span().start;
                    let ty = self.parse_type_expr();
                    fields.push(NodeField::Output(self.spanned(ty, start)));
                }
                Token::Properties => {
                    self.advance();
                    self.expect(&Token::Colon);
                    let props = self.parse_properties_block();
                    fields.push(NodeField::Properties(props));
                }
                Token::Semantic => {
                    self.advance();
                    self.expect(&Token::Colon);
                    let sem = self.parse_semantic_block();
                    fields.push(NodeField::Semantic(sem));
                }
                Token::ProofObligations => {
                    self.advance();
                    self.expect(&Token::Colon);
                    let proofs = self.parse_proof_obligations_block();
                    fields.push(NodeField::ProofObligations(proofs));
                }
                Token::Pre => {
                    self.advance();
                    self.expect(&Token::Colon);
                    let exprs = self.parse_expr_block();
                    fields.push(NodeField::Pre(exprs));
                }
                Token::Post => {
                    self.advance();
                    self.expect(&Token::Colon);
                    let exprs = self.parse_expr_block();
                    fields.push(NodeField::Post(exprs));
                }
                Token::Solve => {
                    let solve = self.parse_solve_block();
                    fields.push(NodeField::Solve(solve));
                }
                _ => {
                    self.error(format!(
                        "unexpected token in node body: {:?}",
                        self.peek_token()
                    ));
                    self.advance();
                }
            }
        }
        self.eat(&Token::RBrace);

        NodeDef { name, fields }
    }

    fn parse_properties_block(&mut self) -> Vec<Property> {
        let mut props = Vec::new();
        self.expect(&Token::LBrace);
        while *self.peek_token() != Token::RBrace && *self.peek_token() != Token::Eof {
            let key = self.expect_ident();
            self.expect(&Token::Colon);
            let start = self.current_span().start;
            let value = self.parse_expr(0);
            props.push(Property {
                key,
                value: self.spanned(value, start),
            });
            self.eat(&Token::Comma);
        }
        self.eat(&Token::RBrace);
        props
    }

    fn parse_semantic_block(&mut self) -> SemanticBlock {
        let mut description = None;
        let mut formal = None;

        self.expect(&Token::LBrace);
        while *self.peek_token() != Token::RBrace && *self.peek_token() != Token::Eof {
            match self.peek_token().clone() {
                Token::Description => {
                    self.advance();
                    self.expect(&Token::Colon);
                    if let Token::Str(s) = self.peek_token().clone() {
                        self.advance();
                        description = Some(s);
                    }
                }
                Token::Formal => {
                    self.advance();
                    self.expect(&Token::Colon);
                    let start = self.current_span().start;
                    let expr = self.parse_expr(0);
                    formal = Some(self.spanned(expr, start));
                }
                _ => {
                    // Skip unknown keys
                    self.advance();
                    if self.eat(&Token::Colon) {
                        self.parse_expr(0);
                    }
                }
            }
        }
        self.eat(&Token::RBrace);

        SemanticBlock {
            description,
            formal,
            examples: Vec::new(),
        }
    }

    fn parse_proof_obligations_block(&mut self) -> Vec<ProofObligation> {
        let mut proofs = Vec::new();
        self.expect(&Token::LBrace);
        while *self.peek_token() != Token::RBrace && *self.peek_token() != Token::Eof {
            let name = self.expect_ident();
            self.expect(&Token::Colon);
            let start = self.current_span().start;
            let expr = self.parse_expr(0);
            proofs.push(ProofObligation {
                name,
                expr: self.spanned(expr, start),
            });
        }
        self.eat(&Token::RBrace);
        proofs
    }

    fn parse_expr_block(&mut self) -> Vec<Spanned<Expr>> {
        let mut exprs = Vec::new();
        self.expect(&Token::LBrace);
        while *self.peek_token() != Token::RBrace && *self.peek_token() != Token::Eof {
            let start = self.current_span().start;
            let expr = self.parse_expr(0);
            exprs.push(self.spanned(expr, start));
        }
        self.eat(&Token::RBrace);
        exprs
    }

    // ── Edge ────────────────────────────────

    fn parse_edge_def(&mut self) -> EdgeDef {
        self.advance(); // consume 'edge'
        let from = self.expect_ident();
        // expect ->
        if !self.eat(&Token::Arrow) && !self.eat(&Token::ArrowSym) {
            self.expect(&Token::Arrow);
        }
        let to = self.expect_ident();

        let mut properties = Vec::new();
        if self.eat(&Token::LBrace) {
            while *self.peek_token() != Token::RBrace && *self.peek_token() != Token::Eof {
                let key = self.expect_ident();
                self.expect(&Token::Colon);
                let start = self.current_span().start;
                let value = self.parse_expr(0);
                properties.push(Property {
                    key,
                    value: self.spanned(value, start),
                });
                self.eat(&Token::Comma);
            }
            self.eat(&Token::RBrace);
        }

        EdgeDef {
            from,
            to,
            properties,
        }
    }

    // ── Solve Block ─────────────────────────

    fn parse_solve_block(&mut self) -> SolveBlock {
        self.advance(); // consume 'solve'
        self.expect(&Token::LBrace);

        let mut goal = None;
        let mut constraints = Vec::new();
        let mut invariants = Vec::new();
        let mut domain = None;
        let mut strategy = None;

        while *self.peek_token() != Token::RBrace && *self.peek_token() != Token::Eof {
            match self.peek_token().clone() {
                Token::Goal => {
                    self.advance();
                    self.expect(&Token::Colon);
                    let start = self.current_span().start;
                    let expr = self.parse_expr(0);
                    goal = Some(self.spanned(expr, start));
                }
                Token::Constraint => {
                    self.advance();
                    self.expect(&Token::Colon);
                    let start = self.current_span().start;
                    let expr = self.parse_expr(0);
                    constraints.push(self.spanned(expr, start));
                }
                Token::Invariant => {
                    self.advance();
                    self.expect(&Token::Colon);
                    let start = self.current_span().start;
                    let expr = self.parse_expr(0);
                    invariants.push(self.spanned(expr, start));
                }
                Token::Domain => {
                    self.advance();
                    self.expect(&Token::Colon);
                    domain = Some(self.parse_domain_block());
                }
                Token::Strategy => {
                    self.advance();
                    self.expect(&Token::Colon);
                    let start = self.current_span().start;
                    let expr = self.parse_expr(0);
                    strategy = Some(self.spanned(expr, start));
                }
                _ => {
                    self.error(format!(
                        "unexpected token in solve block: {:?}",
                        self.peek_token()
                    ));
                    self.advance();
                }
            }
        }
        self.eat(&Token::RBrace);

        SolveBlock {
            goal,
            constraints,
            invariants,
            domain,
            strategy,
        }
    }

    fn parse_domain_block(&mut self) -> DomainBlock {
        let kind = self.expect_ident();
        let mut config = Vec::new();

        if self.eat(&Token::LParen) {
            while *self.peek_token() != Token::RParen && *self.peek_token() != Token::Eof {
                let key = self.expect_ident();
                self.expect(&Token::Colon);
                let start = self.current_span().start;
                let value = self.parse_expr(0);
                config.push(Property {
                    key,
                    value: self.spanned(value, start),
                });
                self.eat(&Token::Comma);
            }
            self.eat(&Token::RParen);
        } else if self.eat(&Token::LBrace) {
            while *self.peek_token() != Token::RBrace && *self.peek_token() != Token::Eof {
                let key = self.expect_ident();
                self.expect(&Token::Colon);
                let start = self.current_span().start;
                let value = self.parse_expr(0);
                config.push(Property {
                    key,
                    value: self.spanned(value, start),
                });
                self.eat(&Token::Comma);
            }
            self.eat(&Token::RBrace);
        }

        DomainBlock { kind, config }
    }

    // ── Function ────────────────────────────

    fn parse_function(&mut self) -> Function {
        self.advance(); // consume 'function'
        let name = self.expect_ident();

        // Type params (skip for now — not in AST)
        if *self.peek_token() == Token::Lt {
            self.advance();
            let mut depth = 1;
            while depth > 0 {
                match self.peek_token() {
                    Token::Lt => {
                        depth += 1;
                        self.advance();
                    }
                    Token::Gt => {
                        depth -= 1;
                        self.advance();
                    }
                    Token::Eof => break,
                    _ => {
                        self.advance();
                    }
                }
            }
        }

        // Parameters
        self.expect(&Token::LParen);
        let mut params = Vec::new();
        while *self.peek_token() != Token::RParen && *self.peek_token() != Token::Eof {
            let pname = self.expect_ident();
            self.expect(&Token::Colon);
            let start = self.current_span().start;
            let ty = self.parse_type_expr();
            params.push(Param {
                name: pname,
                type_expr: self.spanned(ty, start),
            });
            self.eat(&Token::Comma);
        }
        self.eat(&Token::RParen);

        // Return type
        let mut return_type = None;
        if self.eat(&Token::Arrow) || self.eat(&Token::ArrowSym) {
            let start = self.current_span().start;
            let ty = self.parse_type_expr();
            return_type = Some(self.spanned(ty, start));
        }

        self.expect(&Token::LBrace);

        let mut pre = Vec::new();
        let mut post = Vec::new();
        let mut invariants = Vec::new();
        let mut body_exprs: Vec<Spanned<Expr>> = Vec::new();
        let mut synthesize = None;

        while *self.peek_token() != Token::RBrace && *self.peek_token() != Token::Eof {
            match self.peek_token().clone() {
                Token::Pre => {
                    self.advance();
                    self.expect(&Token::Colon);
                    pre = self.parse_expr_block();
                }
                Token::Post => {
                    self.advance();
                    self.expect(&Token::Colon);
                    post = self.parse_expr_block();
                }
                Token::Invariant => {
                    self.advance();
                    self.expect(&Token::Colon);
                    invariants = self.parse_expr_block();
                }
                Token::Synthesize => {
                    self.advance();
                    let mut args = Vec::new();
                    if self.eat(&Token::LParen) {
                        while *self.peek_token() != Token::RParen
                            && *self.peek_token() != Token::Eof
                        {
                            let key = self.expect_ident();
                            self.expect(&Token::Colon);
                            let start = self.current_span().start;
                            let value = self.parse_expr(0);
                            args.push(Property {
                                key,
                                value: self.spanned(value, start),
                            });
                            self.eat(&Token::Comma);
                        }
                        self.eat(&Token::RParen);
                    }
                    synthesize = Some(args);
                }
                _ => {
                    let start = self.current_span().start;
                    let expr = self.parse_expr(0);
                    body_exprs.push(self.spanned(expr, start));
                }
            }
        }
        self.eat(&Token::RBrace);

        let body = if let Some(syn) = synthesize {
            FunctionBody::Synthesize(syn)
        } else {
            FunctionBody::Block(body_exprs)
        };

        Function {
            name,
            params,
            return_type,
            pre,
            post,
            invariants,
            body,
        }
    }

    // ── Type Definition ─────────────────────

    fn parse_type_def(&mut self) -> TypeDef {
        self.advance(); // consume 'type'
        let name = self.expect_ident();

        // Type params
        let mut params = Vec::new();
        if *self.peek_token() == Token::Lt {
            self.advance();
            while *self.peek_token() != Token::Gt && *self.peek_token() != Token::Eof {
                let pname = self.expect_ident();
                let bound = if self.eat(&Token::Colon) {
                    let start = self.current_span().start;
                    let ty = self.parse_type_expr();
                    Some(self.spanned(ty, start))
                } else {
                    None
                };
                params.push(TypeParam { name: pname, bound });
                self.eat(&Token::Comma);
            }
            self.eat(&Token::Gt);
        }

        self.expect(&Token::Eq);

        let start = self.current_span().start;
        let body = self.parse_type_body();

        TypeDef {
            name,
            params,
            body: self.spanned(body, start),
        }
    }

    fn parse_type_body(&mut self) -> TypeExpr {
        // Check for refinement: { var ∈ Type | pred }
        if *self.peek_token() == Token::LBrace {
            if let Some(ty) = self.try_parse_refinement_type() {
                return ty;
            }
        }

        // Check for sum type: first variant, then | next variant ...
        let first = self.parse_type_expr();

        if *self.peek_token() == Token::PipeSep {
            // Sum type
            let mut variants = vec![self.type_expr_to_variant(first)];
            while self.eat(&Token::PipeSep) {
                let ty = self.parse_type_expr();
                variants.push(self.type_expr_to_variant(ty));
            }
            TypeExpr::Sum(variants)
        } else {
            first
        }
    }

    fn type_expr_to_variant(&self, ty: TypeExpr) -> Variant {
        match ty {
            TypeExpr::Named(name) => Variant {
                name,
                fields: Vec::new(),
            },
            TypeExpr::Applied { name, args } => {
                // Treat type args as positional fields
                let fields = args
                    .into_iter()
                    .enumerate()
                    .map(|(i, a)| (format!("_{}", i), a))
                    .collect();
                Variant { name, fields }
            }
            TypeExpr::Dependent { name, params } => Variant {
                name,
                fields: params,
            },
            _ => Variant {
                name: "<error>".to_string(),
                fields: Vec::new(),
            },
        }
    }

    fn try_parse_refinement_type(&mut self) -> Option<TypeExpr> {
        // Look ahead: { ident (∈|in) Type | pred }
        let save = self.pos;

        self.advance(); // {
        if let Token::Ident(var) = self.peek_token().clone() {
            self.advance();
            if *self.peek_token() == Token::InSym
                || (matches!(self.peek_token(), Token::Ident(s) if s == "in"))
            {
                self.advance(); // ∈ or in
                let start = self.current_span().start;
                let base_type = self.parse_type_expr();
                if self.eat(&Token::PipeSep) {
                    let pred_start = self.current_span().start;
                    let predicate = self.parse_expr(0);
                    self.eat(&Token::RBrace);
                    return Some(TypeExpr::Refinement {
                        var,
                        base: Box::new(self.spanned(base_type, start)),
                        predicate: Box::new(self.spanned(predicate, pred_start)),
                    });
                }
            }
        }

        // Not a refinement type, restore position
        self.pos = save;
        None
    }

    // ── Type Expressions ────────────────────

    fn parse_type_expr(&mut self) -> TypeExpr {
        let base = self.parse_type_atom();

        // Function type: A -> B
        if *self.peek_token() == Token::Arrow || *self.peek_token() == Token::ArrowSym {
            self.advance();
            let ret_start = self.current_span().start;
            let ret = self.parse_type_expr();
            return TypeExpr::Function {
                params: vec![Spanned::dummy(base)],
                ret: Box::new(self.spanned(ret, ret_start)),
            };
        }

        // Where clause: T where constraint
        if *self.peek_token() == Token::Where {
            self.advance();
            let c_start = self.current_span().start;
            let constraint = self.parse_expr(0);
            return TypeExpr::Where {
                base: Box::new(Spanned::dummy(base)),
                constraint: Box::new(self.spanned(constraint, c_start)),
            };
        }

        base
    }

    fn parse_type_atom(&mut self) -> TypeExpr {
        match self.peek_token().clone() {
            Token::Ident(name) => {
                self.advance();

                // Check for generic: Name<T, U>
                if *self.peek_token() == Token::Lt {
                    // Disambiguate: if next after < looks like a type, it's generic
                    let save = self.pos;
                    self.advance(); // <
                    let mut args = Vec::new();
                    let mut depth = 1;
                    let mut ok = true;

                    // Try parsing type args
                    loop {
                        if *self.peek_token() == Token::Gt {
                            depth -= 1;
                            if depth == 0 {
                                self.advance();
                                break;
                            }
                        }
                        if *self.peek_token() == Token::Eof {
                            ok = false;
                            break;
                        }
                        let start = self.current_span().start;
                        // Try to parse as named arg: name: expr
                        if let Token::Ident(aname) = self.peek_token().clone() {
                            let save2 = self.pos;
                            self.advance();
                            if self.eat(&Token::Colon) {
                                // Named type arg — treat as named param in Dependent type
                                let ty_start = self.current_span().start;
                                let ty = self.parse_type_expr();
                                args.push((Some(aname), self.spanned(ty, ty_start)));
                            } else {
                                self.pos = save2;
                                let ty = self.parse_type_expr();
                                args.push((None, self.spanned(ty, start)));
                            }
                        } else {
                            let ty = self.parse_type_expr();
                            args.push((None, self.spanned(ty, start)));
                        }
                        if *self.peek_token() == Token::Lt {
                            depth += 1;
                        }
                        if !self.eat(&Token::Comma) {
                            if *self.peek_token() == Token::Gt {
                                self.advance();
                                break;
                            }
                        }
                    }

                    if !ok {
                        self.pos = save;
                        return TypeExpr::Named(name);
                    }

                    return TypeExpr::Applied {
                        name,
                        args: args.into_iter().map(|(_, ty)| ty).collect(),
                    };
                }

                // Check for dependent: Name(param: Type, ...)
                if *self.peek_token() == Token::LParen {
                    let save = self.pos;
                    self.advance(); // (

                    // Try to parse as named params
                    let mut dep_params = Vec::new();
                    let mut is_dep = true;
                    loop {
                        if *self.peek_token() == Token::RParen || *self.peek_token() == Token::Eof {
                            break;
                        }
                        if let Token::Ident(pname) = self.peek_token().clone() {
                            let save2 = self.pos;
                            self.advance();
                            if self.eat(&Token::Colon) {
                                let ts = self.current_span().start;
                                let ty = self.parse_type_expr();
                                dep_params.push((pname, self.spanned(ty, ts)));
                            } else {
                                // Not a dependent type param syntax
                                is_dep = false;
                                self.pos = save2;
                                break;
                            }
                        } else {
                            is_dep = false;
                            break;
                        }
                        if !self.eat(&Token::Comma) {
                            break;
                        }
                    }

                    if is_dep && !dep_params.is_empty() && *self.peek_token() == Token::RParen {
                        self.advance(); // )
                        return TypeExpr::Dependent {
                            name,
                            params: dep_params,
                        };
                    }

                    // Not dependent type, restore
                    self.pos = save;
                }

                TypeExpr::Named(name)
            }
            Token::LBrace => {
                // Record type: { field: Type, ... }
                self.advance();
                let mut fields = Vec::new();
                while *self.peek_token() != Token::RBrace && *self.peek_token() != Token::Eof {
                    let fname = self.expect_ident();
                    self.expect(&Token::Colon);
                    let start = self.current_span().start;
                    let ty = self.parse_type_expr();
                    fields.push((fname, self.spanned(ty, start)));
                    self.eat(&Token::Comma);
                }
                self.eat(&Token::RBrace);
                TypeExpr::Record(fields)
            }
            Token::LParen => {
                // Parenthesized type
                self.advance();
                let ty = self.parse_type_expr();
                self.expect(&Token::RParen);
                ty
            }
            _ => {
                self.error(format!("expected type expression, found {:?}", self.peek_token()));
                self.advance();
                TypeExpr::Named("<error>".to_string())
            }
        }
    }

    // ── Let Binding ─────────────────────────

    fn parse_let_binding(&mut self) -> LetBinding {
        self.advance(); // consume 'let'
        let name = self.expect_ident();

        let type_ann = if self.eat(&Token::Colon) {
            let start = self.current_span().start;
            let ty = self.parse_type_expr();
            Some(self.spanned(ty, start))
        } else {
            None
        };

        self.expect(&Token::Eq);
        let start = self.current_span().start;
        let value = self.parse_expr(0);

        LetBinding {
            name,
            type_ann,
            value: self.spanned(value, start),
        }
    }

    // ── Module ──────────────────────────────

    fn parse_module(&mut self) -> Module {
        self.advance(); // consume 'module'
        let name = self.expect_ident();
        self.expect(&Token::LBrace);

        let mut items = Vec::new();
        while *self.peek_token() != Token::RBrace && *self.peek_token() != Token::Eof {
            let start = self.current_span().start;
            if let Some(item) = self.parse_item() {
                items.push(self.spanned(item, start));
            } else {
                self.synchronize();
            }
        }
        self.eat(&Token::RBrace);

        Module { name, items }
    }

    // ── Import ──────────────────────────────

    fn parse_import(&mut self) -> Import {
        self.advance(); // consume 'import'

        // Parse dotted path: `std.math.trig`
        let mut path = vec![self.expect_ident()];
        while self.eat(&Token::Dot) {
            // Check for selective import: `std.math.{sin, cos}`
            if *self.peek_token() == Token::LBrace {
                break;
            }
            path.push(self.expect_ident());
        }

        // Parse optional selective imports: `.{name1, name2 as alias}`
        let names = if self.eat(&Token::LBrace) {
            let mut names = Vec::new();
            loop {
                if *self.peek_token() == Token::RBrace || *self.peek_token() == Token::Eof {
                    break;
                }
                let name = self.expect_ident();
                let alias = if self.eat(&Token::As) {
                    Some(self.expect_ident())
                } else {
                    None
                };
                names.push(ImportName { name, alias });
                if !self.eat(&Token::Comma) {
                    break;
                }
            }
            self.expect(&Token::RBrace);
            Some(names)
        } else {
            None
        };

        Import { path, names }
    }

    // ── Model ───────────────────────────────

    fn parse_model(&mut self) -> Model {
        self.advance(); // consume 'model'
        let name = self.expect_ident();
        self.expect(&Token::LBrace);

        let mut statements = Vec::new();
        while *self.peek_token() != Token::RBrace && *self.peek_token() != Token::Eof {
            let start = self.current_span().start;
            match self.peek_token().clone() {
                Token::Prior => {
                    self.advance();
                    let pname = self.expect_ident();
                    self.expect(&Token::Colon);
                    let dstart = self.current_span().start;
                    let dist = self.parse_expr(0);
                    statements.push(self.spanned(
                        ModelStatement::Prior {
                            name: pname,
                            distribution: self.spanned(dist, dstart),
                        },
                        start,
                    ));
                }
                Token::Observe => {
                    self.advance();
                    let oname = self.expect_ident();
                    // observe can have `~ distribution`, `: distribution`, or just be a name
                    let dist = if self.eat(&Token::Tilde) || self.eat(&Token::Colon) {
                        let dstart = self.current_span().start;
                        let d = self.parse_expr(0);
                        self.spanned(d, dstart)
                    } else {
                        self.spanned(Expr::Ident(oname.clone()), start)
                    };
                    statements.push(self.spanned(
                        ModelStatement::Observe {
                            name: oname,
                            distribution: dist,
                        },
                        start,
                    ));
                }
                Token::Posterior => {
                    self.advance();
                    self.expect(&Token::Eq);
                    let estart = self.current_span().start;
                    let expr = self.parse_expr(0);
                    statements
                        .push(self.spanned(ModelStatement::Posterior(self.spanned(expr, estart)), start));
                }
                _ => {
                    self.error(format!(
                        "unexpected token in model body: {:?}",
                        self.peek_token()
                    ));
                    self.advance();
                }
            }
        }
        self.eat(&Token::RBrace);

        Model { name, statements }
    }

    // ── Meta ────────────────────────────────

    fn parse_meta(&mut self) -> Meta {
        self.advance(); // consume 'meta'
        let name = self.expect_ident();
        self.expect(&Token::LParen);
        let tstart = self.current_span().start;
        let target = self.parse_expr(0);
        self.expect(&Token::RParen);

        self.expect(&Token::LBrace);
        let mut body = Vec::new();
        while *self.peek_token() != Token::RBrace && *self.peek_token() != Token::Eof {
            let key = self.expect_ident();
            self.expect(&Token::Colon);
            let vstart = self.current_span().start;
            let value = self.parse_expr(0);
            body.push(MetaField {
                key,
                value: self.spanned(value, vstart),
            });
        }
        self.eat(&Token::RBrace);

        Meta {
            name,
            target: self.spanned(target, tstart),
            body,
        }
    }

    // ── Expression Parser (Pratt) ───────────

    fn parse_expr(&mut self, min_bp: u8) -> Expr {
        let left_start = self.current_span().start;
        let mut left = self.parse_prefix();

        loop {
            // Postfix operations
            match self.peek_token().clone() {
                Token::Dot => {
                    // Could be field access or unit literal
                    let save = self.pos;
                    self.advance(); // .
                    if let Token::Ident(field) = self.peek_token().clone() {
                        self.advance();
                        // Check if this is a unit literal (number.unit)
                        if matches!(left, Expr::IntLit(_) | Expr::FloatLit(_))
                            && *self.peek_token() != Token::LParen
                            && *self.peek_token() != Token::Dot
                            && !is_operator(self.peek_token())
                            || (matches!(left, Expr::IntLit(_) | Expr::FloatLit(_))
                                && is_unit_name(&field))
                        {
                            left = Expr::WithUnit {
                                value: Box::new(self.spanned(left, left_start)),
                                unit: field,
                            };
                        } else if *self.peek_token() == Token::LParen {
                            // Method call: obj.method(args)
                            left = Expr::Field {
                                expr: Box::new(self.spanned(left, left_start)),
                                name: field,
                            };
                        } else {
                            left = Expr::Field {
                                expr: Box::new(self.spanned(left, left_start)),
                                name: field,
                            };
                        }
                        continue;
                    } else {
                        self.pos = save;
                    }
                }
                Token::LParen => {
                    // Function call
                    self.advance();
                    let args = self.parse_call_args();
                    self.eat(&Token::RParen);

                    // Check if args are named or positional
                    let (named, positional): (Vec<_>, Vec<_>) =
                        args.into_iter().partition(|a| a.0.is_some());
                    if !named.is_empty() && positional.is_empty() {
                        left = Expr::CallNamed {
                            func: Box::new(self.spanned(left, left_start)),
                            args: named
                                .into_iter()
                                .map(|(n, e)| (n.unwrap(), e))
                                .collect(),
                        };
                    } else {
                        let all_args: Vec<_> = positional.into_iter().chain(named).map(|(_, e)| e).collect();
                        left = Expr::Call {
                            func: Box::new(self.spanned(left, left_start)),
                            args: all_args,
                        };
                    }
                    continue;
                }
                Token::LBracket => {
                    // Index: expr[idx]
                    self.advance();
                    let idx_start = self.current_span().start;
                    // Check for slice: expr[start:end]
                    if *self.peek_token() == Token::Colon {
                        self.advance();
                        let e_start = self.current_span().start;
                        let end_expr = self.parse_expr(0);
                        self.eat(&Token::RBracket);
                        left = Expr::Slice {
                            expr: Box::new(self.spanned(left, left_start)),
                            start: None,
                            end: Some(Box::new(self.spanned(end_expr, e_start))),
                        };
                    } else {
                        let idx = self.parse_expr(0);
                        if self.eat(&Token::Colon) {
                            let e_start = self.current_span().start;
                            let end_expr = self.parse_expr(0);
                            self.eat(&Token::RBracket);
                            left = Expr::Slice {
                                expr: Box::new(self.spanned(left, left_start)),
                                start: Some(Box::new(self.spanned(idx, idx_start))),
                                end: Some(Box::new(self.spanned(end_expr, e_start))),
                            };
                        } else {
                            self.eat(&Token::RBracket);
                            left = Expr::Index {
                                expr: Box::new(self.spanned(left, left_start)),
                                index: Box::new(self.spanned(idx, idx_start)),
                            };
                        }
                    }
                    continue;
                }
                Token::Question => {
                    self.advance();
                    left = Expr::Try(Box::new(self.spanned(left, left_start)));
                    continue;
                }
                _ => {}
            }

            // Infix operations with binding power
            if let Some((l_bp, r_bp)) = infix_binding_power(self.peek_token()) {
                if l_bp < min_bp {
                    break;
                }

                let op_token = self.advance().token;
                let right_start = self.current_span().start;
                let right = self.parse_expr(r_bp);

                left = match op_token {
                    Token::Pipe => Expr::Pipeline {
                        left: Box::new(self.spanned(left, left_start)),
                        right: Box::new(self.spanned(right, right_start)),
                    },
                    _ => {
                        let op = token_to_binop(&op_token);
                        Expr::BinOp {
                            left: Box::new(self.spanned(left, left_start)),
                            op,
                            right: Box::new(self.spanned(right, right_start)),
                        }
                    }
                };
                let _ = left_start; // preserve original start
                continue;
            }

            break;
        }

        left
    }

    fn parse_prefix(&mut self) -> Expr {
        match self.peek_token().clone() {
            Token::Int(n) => {
                self.advance();
                Expr::IntLit(n)
            }
            Token::Float(n) => {
                self.advance();
                Expr::FloatLit(n)
            }
            Token::Str(s) => {
                self.advance();
                Expr::StringLit(s)
            }
            Token::True => {
                self.advance();
                Expr::BoolLit(true)
            }
            Token::False => {
                self.advance();
                Expr::BoolLit(false)
            }
            Token::Ident(name) => {
                self.advance();
                Expr::Ident(name)
            }

            Token::Minus => {
                self.advance();
                let start = self.current_span().start;
                let operand = self.parse_expr(PREC_UNARY);
                Expr::UnaryOp {
                    op: UnaryOp::Neg,
                    operand: Box::new(self.spanned(operand, start)),
                }
            }
            Token::Not => {
                self.advance();
                let start = self.current_span().start;
                let operand = self.parse_expr(PREC_UNARY);
                Expr::UnaryOp {
                    op: UnaryOp::Not,
                    operand: Box::new(self.spanned(operand, start)),
                }
            }

            Token::LParen => {
                self.advance();
                let expr = self.parse_expr(0);
                self.expect(&Token::RParen);
                expr
            }

            Token::LBracket => {
                // Array literal [a, b, c]
                self.advance();
                let mut items = Vec::new();
                while *self.peek_token() != Token::RBracket && *self.peek_token() != Token::Eof {
                    let start = self.current_span().start;
                    let item = self.parse_expr(0);
                    items.push(self.spanned(item, start));
                    self.eat(&Token::Comma);
                }
                self.eat(&Token::RBracket);
                Expr::Array(items)
            }

            Token::LBrace => {
                // Record literal { key: value, ... }
                self.advance();
                let mut pairs = Vec::new();
                while *self.peek_token() != Token::RBrace && *self.peek_token() != Token::Eof {
                    if let Token::Ident(key) = self.peek_token().clone() {
                        let save = self.pos;
                        self.advance();
                        if self.eat(&Token::Colon) {
                            let vstart = self.current_span().start;
                            let value = self.parse_expr(0);
                            pairs.push((key, self.spanned(value, vstart)));
                            self.eat(&Token::Comma);
                        } else {
                            // Not a record — could be set literal or single ident
                            self.pos = save;
                            break;
                        }
                    } else {
                        break;
                    }
                }
                self.eat(&Token::RBrace);
                Expr::Record(pairs)
            }

            Token::Do => self.parse_do_block(),

            Token::If => self.parse_if_expr(),

            Token::Match => self.parse_match_expr(),

            Token::Branch => self.parse_branch_expr(),

            Token::ForAll | Token::ForAllSym => self.parse_quantifier(true),

            Token::Exists | Token::ExistsSym => self.parse_quantifier(false),

            Token::Fn | Token::Lambda => self.parse_lambda(),

            Token::Sigma | Token::Select => self.parse_select_expr(),

            Token::Pi | Token::Project => self.parse_project_expr(),

            Token::Gamma | Token::GroupBy => self.parse_group_by_expr(),

            Token::Yield => {
                self.advance();
                let start = self.current_span().start;
                let expr = self.parse_expr(0);
                Expr::Yield(Box::new(self.spanned(expr, start)))
            }

            Token::Synthesize => {
                self.advance();
                let mut args = Vec::new();
                if self.eat(&Token::LParen) {
                    while *self.peek_token() != Token::RParen && *self.peek_token() != Token::Eof {
                        let key = self.expect_ident();
                        self.expect(&Token::Colon);
                        let start = self.current_span().start;
                        let value = self.parse_expr(0);
                        args.push(Property {
                            key,
                            value: self.spanned(value, start),
                        });
                        self.eat(&Token::Comma);
                    }
                    self.eat(&Token::RParen);
                }
                Expr::Synthesize(args)
            }

            // Keywords that can be used as identifiers in expressions
            Token::Version
            | Token::Target
            | Token::Strategy
            | Token::Domain
            | Token::Goal
            | Token::Constraint
            | Token::Invariant
            | Token::Description
            | Token::Formal
            | Token::Prior
            | Token::Posterior
            | Token::Observe
            | Token::Input
            | Token::Output => {
                let name = format!("{:?}", self.peek_token()).to_lowercase();
                self.advance();
                Expr::Ident(name)
            }

            _ => {
                self.error(format!("expected expression, found {:?}", self.peek_token()));
                self.advance();
                Expr::Ident("<error>".to_string())
            }
        }
    }

    fn parse_call_args(&mut self) -> Vec<(Option<String>, Spanned<Expr>)> {
        let mut args = Vec::new();
        while *self.peek_token() != Token::RParen && *self.peek_token() != Token::Eof {
            // Try named arg: ident ':'
            if let Token::Ident(name) = self.peek_token().clone() {
                let save = self.pos;
                self.advance();
                if self.eat(&Token::Colon) {
                    let start = self.current_span().start;
                    let value = self.parse_expr(0);
                    args.push((Some(name), self.spanned(value, start)));
                    self.eat(&Token::Comma);
                    continue;
                } else {
                    self.pos = save;
                }
            }
            // Positional arg
            let start = self.current_span().start;
            let value = self.parse_expr(0);
            args.push((None, self.spanned(value, start)));
            self.eat(&Token::Comma);
        }
        args
    }

    fn parse_do_block(&mut self) -> Expr {
        self.advance(); // consume 'do'
        self.expect(&Token::LBrace);
        let mut stmts = Vec::new();

        while *self.peek_token() != Token::RBrace && *self.peek_token() != Token::Eof {
            let start = self.current_span().start;
            let stmt = self.parse_do_statement();
            stmts.push(self.spanned(stmt, start));
        }
        self.eat(&Token::RBrace);

        Expr::DoBlock(stmts)
    }

    fn parse_do_statement(&mut self) -> DoStatement {
        if *self.peek_token() == Token::Yield {
            self.advance();
            let start = self.current_span().start;
            let expr = self.parse_expr(0);
            return DoStatement::Yield(self.spanned(expr, start));
        }

        if *self.peek_token() == Token::Let {
            self.advance();
            let name = self.expect_ident();
            self.expect(&Token::Eq);
            let start = self.current_span().start;
            let expr = self.parse_expr(0);
            return DoStatement::Let {
                name,
                expr: self.spanned(expr, start),
            };
        }

        // Try bind: ident <- expr? or ident ← expr?
        if let Token::Ident(name) = self.peek_token().clone() {
            let save = self.pos;
            self.advance();
            if *self.peek_token() == Token::BackArrow || *self.peek_token() == Token::BackArrowSym
            {
                self.advance();
                let estart = self.current_span().start;
                let mut expr = self.parse_expr(0);
                // Check for trailing ?
                if let Expr::Try(inner) = expr {
                    expr = inner.node;
                }
                return DoStatement::Bind {
                    name,
                    expr: self.spanned(expr, estart),
                };
            }
            self.pos = save;
        }

        let start = self.current_span().start;
        let expr = self.parse_expr(0);
        DoStatement::Expr(self.spanned(expr, start))
    }

    fn parse_if_expr(&mut self) -> Expr {
        self.advance(); // consume 'if'
        let cstart = self.current_span().start;
        let cond = self.parse_expr(0);

        // 'then' is optional for block-style if
        self.eat(&Token::Then);
        let tstart = self.current_span().start;
        let then_ = self.parse_expr(0);

        let else_ = if self.eat(&Token::Else) {
            let estart = self.current_span().start;
            let e = self.parse_expr(0);
            Some(Box::new(self.spanned(e, estart)))
        } else {
            None
        };

        Expr::If {
            cond: Box::new(self.spanned(cond, cstart)),
            then_: Box::new(self.spanned(then_, tstart)),
            else_,
        }
    }

    fn parse_match_expr(&mut self) -> Expr {
        self.advance(); // consume 'match'
        let estart = self.current_span().start;
        let scrutinee = self.parse_expr(0);
        self.expect(&Token::LBrace);

        let mut arms = Vec::new();
        while *self.peek_token() != Token::RBrace && *self.peek_token() != Token::Eof {
            let pstart = self.current_span().start;
            let pattern = self.parse_pattern();
            let guard = if *self.peek_token() == Token::LParen {
                // Guard in parens
                self.advance();
                let gstart = self.current_span().start;
                let g = self.parse_expr(0);
                self.eat(&Token::RParen);
                Some(self.spanned(g, gstart))
            } else {
                None
            };

            // Arrow
            if !self.eat(&Token::Arrow) && !self.eat(&Token::ArrowSym) {
                self.expect(&Token::Arrow);
            }

            let bstart = self.current_span().start;
            let body = self.parse_expr(0);

            arms.push(MatchArm {
                pattern: self.spanned(pattern, pstart),
                guard,
                body: self.spanned(body, bstart),
            });
        }
        self.eat(&Token::RBrace);

        Expr::Match {
            expr: Box::new(self.spanned(scrutinee, estart)),
            arms,
        }
    }

    fn parse_branch_expr(&mut self) -> Expr {
        self.advance(); // consume 'branch'
        let estart = self.current_span().start;
        let distribution = self.parse_expr(0);
        self.expect(&Token::LBrace);

        let mut arms = Vec::new();
        while *self.peek_token() != Token::RBrace && *self.peek_token() != Token::Eof {
            let pstart = self.current_span().start;
            let pattern = self.parse_pattern();
            let guard = if *self.peek_token() == Token::LParen {
                self.advance();
                let gstart = self.current_span().start;
                let g = self.parse_expr(0);
                self.eat(&Token::RParen);
                Some(self.spanned(g, gstart))
            } else {
                None
            };

            if !self.eat(&Token::Arrow) && !self.eat(&Token::ArrowSym) {
                self.expect(&Token::Arrow);
            }

            let bstart = self.current_span().start;
            let body = self.parse_expr(0);

            arms.push(BranchArm {
                pattern: self.spanned(pattern, pstart),
                guard,
                body: self.spanned(body, bstart),
            });
        }
        self.eat(&Token::RBrace);

        Expr::Branch {
            expr: Box::new(self.spanned(distribution, estart)),
            arms,
        }
    }

    fn parse_pattern(&mut self) -> Pattern {
        match self.peek_token().clone() {
            Token::Ident(name) if name == "_" => {
                self.advance();
                Pattern::Wildcard
            }
            Token::Ident(name) => {
                self.advance();
                if *self.peek_token() == Token::LParen {
                    self.advance();
                    let mut fields = Vec::new();
                    while *self.peek_token() != Token::RParen && *self.peek_token() != Token::Eof {
                        let pstart = self.current_span().start;
                        let p = self.parse_pattern();
                        fields.push(self.spanned(p, pstart));
                        self.eat(&Token::Comma);
                    }
                    self.eat(&Token::RParen);
                    Pattern::Constructor(name, fields)
                } else {
                    Pattern::Ident(name)
                }
            }
            Token::Int(n) => {
                let start = self.current_span().start;
                self.advance();
                Pattern::Literal(self.spanned(Expr::IntLit(n), start))
            }
            Token::Float(n) => {
                let start = self.current_span().start;
                self.advance();
                Pattern::Literal(self.spanned(Expr::FloatLit(n), start))
            }
            Token::Str(s) => {
                let start = self.current_span().start;
                self.advance();
                Pattern::Literal(self.spanned(Expr::StringLit(s), start))
            }
            Token::True => {
                let start = self.current_span().start;
                self.advance();
                Pattern::Literal(self.spanned(Expr::BoolLit(true), start))
            }
            Token::False => {
                let start = self.current_span().start;
                self.advance();
                Pattern::Literal(self.spanned(Expr::BoolLit(false), start))
            }
            _ => {
                // Wildcard for unrecognized patterns
                self.advance();
                Pattern::Wildcard
            }
        }
    }

    fn parse_quantifier(&mut self, is_forall: bool) -> Expr {
        self.advance(); // consume forall/exists/∀/∃
        let var = self.expect_ident();
        // expect ∈ or 'in'
        if !self.eat(&Token::InSym) {
            self.expect(&Token::InSym); // already handles error
        }
        let dstart = self.current_span().start;
        let domain = self.parse_expr(PREC_COMPARISON + 1);

        // expect → or ->
        let body = if self.eat(&Token::Arrow)
            || self.eat(&Token::ArrowSym)
            || self.eat(&Token::ImpliesSym)
        {
            let bstart = self.current_span().start;
            let b = self.parse_expr(0);
            self.spanned(b, bstart)
        } else {
            self.spanned(Expr::BoolLit(true), dstart)
        };

        if is_forall {
            Expr::ForAll {
                var,
                domain: Box::new(self.spanned(domain, dstart)),
                body: Box::new(body),
            }
        } else {
            Expr::Exists {
                var,
                domain: Box::new(self.spanned(domain, dstart)),
                body: Box::new(body),
            }
        }
    }

    fn parse_lambda(&mut self) -> Expr {
        self.advance(); // consume 'fn' or 'λ'
        let mut params = Vec::new();

        // Single param: fn x: Type -> body
        // Multi param: fn(x: Type, y: Type) -> body
        if self.eat(&Token::LParen) {
            while *self.peek_token() != Token::RParen && *self.peek_token() != Token::Eof {
                let pname = self.expect_ident();
                self.expect(&Token::Colon);
                let tstart = self.current_span().start;
                let ty = self.parse_type_expr();
                params.push(Param {
                    name: pname,
                    type_expr: self.spanned(ty, tstart),
                });
                self.eat(&Token::Comma);
            }
            self.eat(&Token::RParen);
        } else {
            let pname = self.expect_ident();
            let type_expr = if self.eat(&Token::Colon) {
                let tstart = self.current_span().start;
                let ty = self.parse_type_expr();
                self.spanned(ty, tstart)
            } else {
                Spanned::dummy(TypeExpr::Named("_".to_string()))
            };
            params.push(Param {
                name: pname,
                type_expr,
            });
        }

        // Arrow
        if !self.eat(&Token::Arrow) && !self.eat(&Token::ArrowSym) {
            self.expect(&Token::Arrow);
        }

        let bstart = self.current_span().start;
        let body = self.parse_expr(0);

        Expr::Lambda {
            params,
            body: Box::new(self.spanned(body, bstart)),
        }
    }

    fn parse_select_expr(&mut self) -> Expr {
        self.advance(); // σ or select
        self.expect(&Token::LParen);
        let pstart = self.current_span().start;
        let predicate = self.parse_expr(0);
        self.expect(&Token::RParen);

        // Optional relation application: σ(pred)(relation)
        if self.eat(&Token::LParen) {
            let rstart = self.current_span().start;
            let relation = self.parse_expr(0);
            self.expect(&Token::RParen);
            Expr::Select {
                predicate: Box::new(self.spanned(predicate, pstart)),
                relation: Box::new(self.spanned(relation, rstart)),
            }
        } else {
            // Return as a partial application (call)
            Expr::Call {
                func: Box::new(Spanned::dummy(Expr::Ident("σ".to_string()))),
                args: vec![self.spanned(predicate, pstart)],
            }
        }
    }

    fn parse_project_expr(&mut self) -> Expr {
        self.advance(); // π or project
        self.expect(&Token::LBracket);
        let mut fields = Vec::new();
        while *self.peek_token() != Token::RBracket && *self.peek_token() != Token::Eof {
            // Accept both identifiers and *
            if *self.peek_token() == Token::Star {
                self.advance();
                fields.push("*".to_string());
            } else {
                fields.push(self.expect_ident());
            }
            self.eat(&Token::Comma);
        }
        self.eat(&Token::RBracket);

        // Optional relation application: π[fields](relation)
        if self.eat(&Token::LParen) {
            let rstart = self.current_span().start;
            let relation = self.parse_expr(0);
            self.expect(&Token::RParen);
            Expr::Project {
                fields,
                relation: Box::new(self.spanned(relation, rstart)),
            }
        } else {
            Expr::Project {
                fields,
                relation: Box::new(Spanned::dummy(Expr::Ident("<pending>".to_string()))),
            }
        }
    }

    fn parse_group_by_expr(&mut self) -> Expr {
        self.advance(); // γ or group_by
        self.expect(&Token::LBracket);
        let mut keys = Vec::new();
        // Parse keys until ; or ]
        while *self.peek_token() != Token::Semi
            && *self.peek_token() != Token::RBracket
            && *self.peek_token() != Token::Eof
        {
            keys.push(self.expect_ident());
            self.eat(&Token::Comma);
        }

        let mut aggregates = Vec::new();
        if self.eat(&Token::Semi) {
            while *self.peek_token() != Token::RBracket && *self.peek_token() != Token::Eof {
                let start = self.current_span().start;
                let agg = self.parse_expr(0);
                aggregates.push(self.spanned(agg, start));
                self.eat(&Token::Comma);
            }
        }
        self.eat(&Token::RBracket);

        // Optional relation application
        if self.eat(&Token::LParen) {
            let rstart = self.current_span().start;
            let relation = self.parse_expr(0);
            self.expect(&Token::RParen);
            Expr::GroupBy {
                keys,
                aggregates,
                relation: Box::new(self.spanned(relation, rstart)),
            }
        } else {
            Expr::GroupBy {
                keys,
                aggregates,
                relation: Box::new(Spanned::dummy(Expr::Ident("<pending>".to_string()))),
            }
        }
    }
}

// ─── Operator Precedence ─────────────────────────────────

const PREC_PIPELINE: u8 = 1;
const PREC_OR: u8 = 3;
const PREC_AND: u8 = 5;
const PREC_IMPLIES: u8 = 4;
const PREC_COMPARISON: u8 = 7;
const PREC_RANGE: u8 = 9;
const PREC_CONCAT: u8 = 10;
const PREC_ADD: u8 = 11;
const PREC_MUL: u8 = 13;
const PREC_UNARY: u8 = 15;

fn infix_binding_power(token: &Token) -> Option<(u8, u8)> {
    let bp = match token {
        Token::Pipe => (PREC_PIPELINE, PREC_PIPELINE + 1),
        Token::OrSym => (PREC_OR, PREC_OR + 1),
        Token::AndSym => (PREC_AND, PREC_AND + 1),
        Token::ImpliesSym => (PREC_IMPLIES, PREC_IMPLIES + 1),
        Token::EqEq | Token::Eq => (PREC_COMPARISON, PREC_COMPARISON + 1),
        Token::Neq | Token::NeqSym => (PREC_COMPARISON, PREC_COMPARISON + 1),
        Token::Lt | Token::Gt => (PREC_COMPARISON, PREC_COMPARISON + 1),
        Token::Leq | Token::LeqSym | Token::Geq | Token::GeqSym => {
            (PREC_COMPARISON, PREC_COMPARISON + 1)
        }
        Token::InSym | Token::NotInSym => (PREC_COMPARISON, PREC_COMPARISON + 1),
        Token::ApproxSym | Token::Tilde => (PREC_COMPARISON, PREC_COMPARISON + 1),
        Token::DotDot => (PREC_RANGE, PREC_RANGE + 1),
        Token::Concat => (PREC_CONCAT, PREC_CONCAT + 1),
        Token::Plus | Token::Minus => (PREC_ADD, PREC_ADD + 1),
        Token::Star | Token::Slash | Token::Percent => (PREC_MUL, PREC_MUL + 1),
        // Join operator ⋈
        Token::Bowtie => (PREC_COMPARISON, PREC_COMPARISON + 1),
        _ => return None,
    };
    Some(bp)
}

fn token_to_binop(token: &Token) -> BinOp {
    match token {
        Token::Plus => BinOp::Add,
        Token::Minus => BinOp::Sub,
        Token::Star => BinOp::Mul,
        Token::Slash => BinOp::Div,
        Token::Percent => BinOp::Mod,
        Token::EqEq | Token::Eq => BinOp::Eq,
        Token::Neq | Token::NeqSym => BinOp::Neq,
        Token::Lt => BinOp::Lt,
        Token::Gt => BinOp::Gt,
        Token::Leq | Token::LeqSym => BinOp::Leq,
        Token::Geq | Token::GeqSym => BinOp::Geq,
        Token::AndSym => BinOp::And,
        Token::OrSym => BinOp::Or,
        Token::ImpliesSym => BinOp::Implies,
        Token::InSym => BinOp::In,
        Token::NotInSym => BinOp::NotIn,
        Token::Concat => BinOp::Concat,
        Token::DotDot => BinOp::Add, // Range handled separately
        Token::ApproxSym | Token::Tilde => BinOp::Assign, // ~ approximation
        _ => BinOp::Add, // fallback
    }
}

fn is_operator(token: &Token) -> bool {
    infix_binding_power(token).is_some()
}

fn is_unit_name(name: &str) -> bool {
    matches!(
        name,
        "ms" | "s"
            | "min"
            | "hour"
            | "day"
            | "ns"
            | "us"
            | "GiB"
            | "MiB"
            | "KiB"
            | "GB"
            | "MB"
            | "KB"
            | "Gbps"
            | "Mbps"
            | "Kbps"
            | "USD"
            | "EUR"
    )
}
