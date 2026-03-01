use std::iter::Peekable;
use std::str::CharIndices;

// ── Token ────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub struct Token {
    pub kind: TokenKind,
    pub span: Span,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub enum TokenKind {
    // ── Keywords ──
    Graph,
    Node,
    Edge,
    Solve,
    Function,
    Type,
    Let,
    Module,
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
    Implies,
    And,
    Or,
    Not,
    In,
    NotIn,
    Fn,
    Select,
    Project,
    Join,
    GroupBy,
    Version,
    Target,
    Description,
    Formal,
    As,

    // ── Literals ──
    IntLit(i64),
    FloatLit(f64),
    StringLit(String),

    // ── Identifier ──
    Ident(String),

    // ── Unicode operators (with ASCII fallback handled in lexer) ──
    Sigma,      // σ or `select` keyword
    Pi,         // π or `project` keyword
    Bowtie,     // ⋈ or `join` keyword
    Gamma,      // γ or `group_by` keyword
    Lambda,     // λ or `fn` keyword
    ForAllSym,  // ∀ or `forall` keyword
    ExistsSym,  // ∃ or `exists` keyword
    InSym,      // ∈ or `in` keyword
    NotInSym,   // ∉ or `not_in` keyword
    ImpliesSym, // ⟹ or `implies` keyword
    AndSym,     // ∧ or `and` keyword
    OrSym,      // ∨ or `or` keyword
    Leq,        // ≤ or <=
    Geq,        // ≥ or >=
    Neq,        // ≠ or !=
    Arrow,      // → or ->
    LeftArrow,  // ← or <-
    Sum,        // Σ or `sum` keyword
    Prod,       // Π or `prod` keyword

    // ── Math set symbols ──
    RealSet, // ℝ or `Real`
    IntSet,  // ℤ or `Int`
    NatSet,  // ℕ or `Nat`

    // ── Punctuation ──
    LParen,     // (
    RParen,     // )
    LBrace,     // {
    RBrace,     // }
    LBracket,   // [
    RBracket,   // ]
    LAngle,     // <
    RAngle,     // >
    Comma,      // ,
    Colon,      // :
    ColonColon, // ::
    Semicolon,  // ;
    Dot,        // .
    DotDot,     // ..
    At,         // @
    Question,   // ?
    Pipe,       // |
    Underscore, // _

    // ── Operators ──
    Plus,      // +
    Minus,     // -
    Star,      // *
    Slash,     // /
    Percent,   // %
    Eq,        // =
    EqEq,      // ==
    BangEq,    // !=
    Lt,        // <
    Gt,        // >
    LtEq,      // <=
    GtEq,      // >=
    PipeRight, // |>
    Tilde,     // ~
    Approx,    // ≈
    PlusPlus,  // ++

    // ── Special ──
    Comment(String),
    Newline,
    Eof,
}

// ── Errors ───────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, thiserror::Error)]
pub enum LexError {
    #[error("Unexpected character '{ch}' at offset {offset}")]
    UnexpectedChar { ch: char, offset: usize },
    #[error("Unterminated string literal starting at offset {offset}")]
    UnterminatedString { offset: usize },
}

// ── Lexer ────────────────────────────────────────────────────────────────────

pub struct Lexer<'a> {
    source: &'a str,
    chars: Peekable<CharIndices<'a>>,
    position: usize,
}

impl<'a> Lexer<'a> {
    pub fn new(source: &'a str) -> Self {
        Self {
            source,
            chars: source.char_indices().peekable(),
            position: 0,
        }
    }

    pub fn tokenize(&mut self) -> Result<Vec<Token>, Vec<LexError>> {
        let mut tokens = Vec::new();
        let mut errors = Vec::new();

        loop {
            match self.next_token() {
                Ok(Some(token)) => {
                    if matches!(token.kind, TokenKind::Eof) {
                        tokens.push(token);
                        break;
                    }
                    tokens.push(token);
                }
                Ok(None) => {
                    tokens.push(Token {
                        kind: TokenKind::Eof,
                        span: Span {
                            start: self.position,
                            end: self.position,
                        },
                        text: String::new(),
                    });
                    break;
                }
                Err(e) => {
                    errors.push(e);
                }
            }
        }

        if errors.is_empty() {
            Ok(tokens)
        } else {
            Err(errors)
        }
    }

    fn next_token(&mut self) -> Result<Option<Token>, LexError> {
        self.skip_whitespace();

        let (start, ch) = match self.chars.peek().copied() {
            Some(pair) => pair,
            None => return Ok(None),
        };

        // Newlines
        if ch == '\n' {
            self.advance();
            return Ok(Some(Token {
                kind: TokenKind::Newline,
                span: Span {
                    start,
                    end: self.position,
                },
                text: "\n".into(),
            }));
        }

        // String literals
        if ch == '"' {
            return self.read_string().map(Some);
        }

        // Numbers
        if ch.is_ascii_digit() {
            return self.read_number().map(Some);
        }

        // Unicode operators (must come before identifier check since
        // Greek/math chars like σ, π, γ pass `is_alphabetic()`)
        if let Some(tok) = self.try_unicode_operator(start, ch) {
            return Ok(Some(tok));
        }

        // Identifiers and keywords
        if ch.is_alphabetic() || ch == '_' {
            // Don't consume lone '_' as an identifier start if followed by non-ident chars
            if ch == '_' {
                // Peek at the next char after '_'
                let next_ch = self.source[start + 1..].chars().next();
                if let Some(nc) = next_ch {
                    if is_ident_continue(nc) {
                        return Ok(Some(self.read_ident_or_keyword()));
                    }
                }
                // Lone underscore (or at end of input)
                self.advance();
                return Ok(Some(Token {
                    kind: TokenKind::Underscore,
                    span: Span {
                        start,
                        end: self.position,
                    },
                    text: "_".into(),
                }));
            }
            return Ok(Some(self.read_ident_or_keyword()));
        }

        // Comments: --
        if ch == '-' {
            let next_ch = self.source[start + 1..].chars().next();
            if next_ch == Some('-') {
                return Ok(Some(self.read_comment()));
            }
        }

        // Multi-char and single-char operators/punctuation
        self.read_operator_or_punct(start, ch).map(Some)
    }

    // ── Helpers ──────────────────────────────────────────────────────────────

    fn advance(&mut self) -> Option<(usize, char)> {
        let result = self.chars.next();
        if let Some((pos, ch)) = result {
            self.position = pos + ch.len_utf8();
        }
        result
    }

    fn skip_whitespace(&mut self) {
        while let Some(&(_, ch)) = self.chars.peek() {
            if ch == ' ' || ch == '\t' || ch == '\r' {
                self.advance();
            } else {
                break;
            }
        }
    }

    fn read_string(&mut self) -> Result<Token, LexError> {
        let (start, _) = self.advance().unwrap(); // consume opening "
        let mut value = String::new();

        loop {
            match self.advance() {
                Some((_, '\\')) => {
                    // Escape sequence
                    match self.advance() {
                        Some((_, 'n')) => value.push('\n'),
                        Some((_, 't')) => value.push('\t'),
                        Some((_, 'r')) => value.push('\r'),
                        Some((_, '\\')) => value.push('\\'),
                        Some((_, '"')) => value.push('"'),
                        Some((_, ch)) => {
                            value.push('\\');
                            value.push(ch);
                        }
                        None => return Err(LexError::UnterminatedString { offset: start }),
                    }
                }
                Some((_, '"')) => {
                    // Closing quote
                    let text = &self.source[start..self.position];
                    return Ok(Token {
                        kind: TokenKind::StringLit(value),
                        span: Span {
                            start,
                            end: self.position,
                        },
                        text: text.to_string(),
                    });
                }
                Some((_, ch)) => value.push(ch),
                None => return Err(LexError::UnterminatedString { offset: start }),
            }
        }
    }

    fn read_number(&mut self) -> Result<Token, LexError> {
        let (start, _) = *self.chars.peek().unwrap();
        let mut is_float = false;

        // Read integer part
        self.eat_digits();

        // Check for dot followed by digits (float), vs dot followed by
        // letter/underscore (unit literal like `200.ms` → IntLit Dot Ident)
        if let Some(&(dot_pos, '.')) = self.chars.peek() {
            // Look ahead past the dot
            let after_dot = self.source[dot_pos + 1..].chars().next();
            match after_dot {
                Some(ch) if ch.is_ascii_digit() => {
                    // It's a float like 3.14
                    self.advance(); // consume '.'
                    is_float = true;
                    self.eat_digits();
                }
                _ => {
                    // Dot followed by letter/underscore/nothing → leave dot for next token
                }
            }
        }

        let text = &self.source[start..self.position];
        let kind = if is_float {
            TokenKind::FloatLit(text.parse::<f64>().unwrap())
        } else {
            TokenKind::IntLit(text.parse::<i64>().unwrap())
        };

        Ok(Token {
            kind,
            span: Span {
                start,
                end: self.position,
            },
            text: text.to_string(),
        })
    }

    fn eat_digits(&mut self) {
        while let Some(&(_, ch)) = self.chars.peek() {
            if ch.is_ascii_digit() {
                self.advance();
            } else {
                break;
            }
        }
    }

    fn read_ident_or_keyword(&mut self) -> Token {
        let (start, _) = *self.chars.peek().unwrap();

        while let Some(&(_, ch)) = self.chars.peek() {
            if is_ident_continue(ch) {
                self.advance();
            } else {
                break;
            }
        }

        let text = &self.source[start..self.position];
        let kind = keyword_or_ident(text);

        Token {
            kind,
            span: Span {
                start,
                end: self.position,
            },
            text: text.to_string(),
        }
    }

    fn read_comment(&mut self) -> Token {
        let (start, _) = self.advance().unwrap(); // first '-'
        self.advance(); // second '-'

        // Skip optional leading space
        if let Some(&(_, ' ')) = self.chars.peek() {
            self.advance();
        }

        let content_start = self.position;

        while let Some(&(_, ch)) = self.chars.peek() {
            if ch == '\n' {
                break;
            }
            self.advance();
        }

        let content = self.source[content_start..self.position].to_string();
        let text = self.source[start..self.position].to_string();

        Token {
            kind: TokenKind::Comment(content),
            span: Span {
                start,
                end: self.position,
            },
            text,
        }
    }

    fn try_unicode_operator(&mut self, start: usize, ch: char) -> Option<Token> {
        let kind = match ch {
            'σ' => TokenKind::Sigma,
            'π' => TokenKind::Pi,
            '⋈' => TokenKind::Bowtie,
            'γ' => TokenKind::Gamma,
            'λ' => TokenKind::Lambda,
            '∀' => TokenKind::ForAllSym,
            '∃' => TokenKind::ExistsSym,
            '∈' => TokenKind::InSym,
            '∉' => TokenKind::NotInSym,
            '⟹' => TokenKind::ImpliesSym,
            '∧' => TokenKind::AndSym,
            '∨' => TokenKind::OrSym,
            '≤' => TokenKind::Leq,
            '≥' => TokenKind::Geq,
            '≠' => TokenKind::Neq,
            '→' => TokenKind::Arrow,
            '←' => TokenKind::LeftArrow,
            'Σ' => TokenKind::Sum,
            'Π' => TokenKind::Prod,
            'ℝ' => TokenKind::RealSet,
            'ℤ' => TokenKind::IntSet,
            'ℕ' => TokenKind::NatSet,
            '≈' => TokenKind::Approx,
            _ => return None,
        };

        self.advance();
        let text = &self.source[start..self.position];
        Some(Token {
            kind,
            span: Span {
                start,
                end: self.position,
            },
            text: text.to_string(),
        })
    }

    fn read_operator_or_punct(
        &mut self,
        start: usize,
        ch: char,
    ) -> Result<Token, LexError> {
        // Helper: consume current char and return a token
        macro_rules! single {
            ($kind:expr) => {{
                self.advance();
                let text = &self.source[start..self.position];
                Ok(Token {
                    kind: $kind,
                    span: Span {
                        start,
                        end: self.position,
                    },
                    text: text.to_string(),
                })
            }};
        }

        match ch {
            '(' => single!(TokenKind::LParen),
            ')' => single!(TokenKind::RParen),
            '{' => single!(TokenKind::LBrace),
            '}' => single!(TokenKind::RBrace),
            '[' => single!(TokenKind::LBracket),
            ']' => single!(TokenKind::RBracket),
            ',' => single!(TokenKind::Comma),
            ';' => single!(TokenKind::Semicolon),
            '@' => single!(TokenKind::At),
            '?' => single!(TokenKind::Question),
            '*' => single!(TokenKind::Star),
            '%' => single!(TokenKind::Percent),
            '~' => single!(TokenKind::Tilde),

            ':' => {
                self.advance();
                if let Some(&(_, ':')) = self.chars.peek() {
                    self.advance();
                    let text = &self.source[start..self.position];
                    Ok(Token {
                        kind: TokenKind::ColonColon,
                        span: Span {
                            start,
                            end: self.position,
                        },
                        text: text.to_string(),
                    })
                } else {
                    let text = &self.source[start..self.position];
                    Ok(Token {
                        kind: TokenKind::Colon,
                        span: Span {
                            start,
                            end: self.position,
                        },
                        text: text.to_string(),
                    })
                }
            }

            '.' => {
                self.advance();
                if let Some(&(_, '.')) = self.chars.peek() {
                    self.advance();
                    let text = &self.source[start..self.position];
                    Ok(Token {
                        kind: TokenKind::DotDot,
                        span: Span {
                            start,
                            end: self.position,
                        },
                        text: text.to_string(),
                    })
                } else {
                    let text = &self.source[start..self.position];
                    Ok(Token {
                        kind: TokenKind::Dot,
                        span: Span {
                            start,
                            end: self.position,
                        },
                        text: text.to_string(),
                    })
                }
            }

            '+' => {
                self.advance();
                if let Some(&(_, '+')) = self.chars.peek() {
                    self.advance();
                    let text = &self.source[start..self.position];
                    Ok(Token {
                        kind: TokenKind::PlusPlus,
                        span: Span {
                            start,
                            end: self.position,
                        },
                        text: text.to_string(),
                    })
                } else {
                    let text = &self.source[start..self.position];
                    Ok(Token {
                        kind: TokenKind::Plus,
                        span: Span {
                            start,
                            end: self.position,
                        },
                        text: text.to_string(),
                    })
                }
            }

            '-' => {
                // We already checked for `--` (comment) before calling this
                self.advance();
                if let Some(&(_, '>')) = self.chars.peek() {
                    self.advance();
                    let text = &self.source[start..self.position];
                    Ok(Token {
                        kind: TokenKind::Arrow,
                        span: Span {
                            start,
                            end: self.position,
                        },
                        text: text.to_string(),
                    })
                } else {
                    let text = &self.source[start..self.position];
                    Ok(Token {
                        kind: TokenKind::Minus,
                        span: Span {
                            start,
                            end: self.position,
                        },
                        text: text.to_string(),
                    })
                }
            }

            '/' => single!(TokenKind::Slash),

            '=' => {
                self.advance();
                if let Some(&(_, '=')) = self.chars.peek() {
                    self.advance();
                    let text = &self.source[start..self.position];
                    Ok(Token {
                        kind: TokenKind::EqEq,
                        span: Span {
                            start,
                            end: self.position,
                        },
                        text: text.to_string(),
                    })
                } else {
                    let text = &self.source[start..self.position];
                    Ok(Token {
                        kind: TokenKind::Eq,
                        span: Span {
                            start,
                            end: self.position,
                        },
                        text: text.to_string(),
                    })
                }
            }

            '!' => {
                self.advance();
                if let Some(&(_, '=')) = self.chars.peek() {
                    self.advance();
                    let text = &self.source[start..self.position];
                    Ok(Token {
                        kind: TokenKind::BangEq,
                        span: Span {
                            start,
                            end: self.position,
                        },
                        text: text.to_string(),
                    })
                } else {
                    // Standalone '!' — treat as unexpected for now
                    Err(LexError::UnexpectedChar {
                        ch: '!',
                        offset: start,
                    })
                }
            }

            '<' => {
                self.advance();
                if let Some(&(_, '=')) = self.chars.peek() {
                    self.advance();
                    let text = &self.source[start..self.position];
                    Ok(Token {
                        kind: TokenKind::LtEq,
                        span: Span {
                            start,
                            end: self.position,
                        },
                        text: text.to_string(),
                    })
                } else if let Some(&(_, '-')) = self.chars.peek() {
                    self.advance();
                    let text = &self.source[start..self.position];
                    Ok(Token {
                        kind: TokenKind::LeftArrow,
                        span: Span {
                            start,
                            end: self.position,
                        },
                        text: text.to_string(),
                    })
                } else {
                    let text = &self.source[start..self.position];
                    Ok(Token {
                        kind: TokenKind::Lt,
                        span: Span {
                            start,
                            end: self.position,
                        },
                        text: text.to_string(),
                    })
                }
            }

            '>' => {
                self.advance();
                if let Some(&(_, '=')) = self.chars.peek() {
                    self.advance();
                    let text = &self.source[start..self.position];
                    Ok(Token {
                        kind: TokenKind::GtEq,
                        span: Span {
                            start,
                            end: self.position,
                        },
                        text: text.to_string(),
                    })
                } else {
                    let text = &self.source[start..self.position];
                    Ok(Token {
                        kind: TokenKind::Gt,
                        span: Span {
                            start,
                            end: self.position,
                        },
                        text: text.to_string(),
                    })
                }
            }

            '|' => {
                self.advance();
                if let Some(&(_, '>')) = self.chars.peek() {
                    self.advance();
                    let text = &self.source[start..self.position];
                    Ok(Token {
                        kind: TokenKind::PipeRight,
                        span: Span {
                            start,
                            end: self.position,
                        },
                        text: text.to_string(),
                    })
                } else {
                    let text = &self.source[start..self.position];
                    Ok(Token {
                        kind: TokenKind::Pipe,
                        span: Span {
                            start,
                            end: self.position,
                        },
                        text: text.to_string(),
                    })
                }
            }

            _ => {
                self.advance();
                Err(LexError::UnexpectedChar { ch, offset: start })
            }
        }
    }
}

// ── Free functions ───────────────────────────────────────────────────────────

fn is_ident_continue(ch: char) -> bool {
    ch.is_alphanumeric() || ch == '_'
}

fn keyword_or_ident(text: &str) -> TokenKind {
    match text {
        "graph" => TokenKind::Graph,
        "node" => TokenKind::Node,
        "edge" => TokenKind::Edge,
        "solve" => TokenKind::Solve,
        "function" => TokenKind::Function,
        "type" => TokenKind::Type,
        "let" => TokenKind::Let,
        "module" => TokenKind::Module,
        "model" => TokenKind::Model,
        "meta" => TokenKind::Meta,
        "pre" => TokenKind::Pre,
        "post" => TokenKind::Post,
        "invariant" => TokenKind::Invariant,
        "constraint" => TokenKind::Constraint,
        "goal" => TokenKind::Goal,
        "domain" => TokenKind::Domain,
        "strategy" => TokenKind::Strategy,
        "input" => TokenKind::Input,
        "output" => TokenKind::Output,
        "properties" => TokenKind::Properties,
        "semantic" => TokenKind::Semantic,
        "proof_obligations" => TokenKind::ProofObligations,
        "synthesize" => TokenKind::Synthesize,
        "do" => TokenKind::Do,
        "yield" => TokenKind::Yield,
        "branch" => TokenKind::Branch,
        "prior" => TokenKind::Prior,
        "observe" => TokenKind::Observe,
        "posterior" => TokenKind::Posterior,
        "where" => TokenKind::Where,
        "if" => TokenKind::If,
        "then" => TokenKind::Then,
        "else" => TokenKind::Else,
        "match" => TokenKind::Match,
        "true" => TokenKind::True,
        "false" => TokenKind::False,
        "forall" => TokenKind::ForAll,
        "exists" => TokenKind::Exists,
        "implies" => TokenKind::Implies,
        "and" => TokenKind::And,
        "or" => TokenKind::Or,
        "not" => TokenKind::Not,
        "in" => TokenKind::In,
        "not_in" => TokenKind::NotIn,
        "fn" => TokenKind::Fn,
        "select" => TokenKind::Select,
        "project" => TokenKind::Project,
        "join" => TokenKind::Join,
        "group_by" => TokenKind::GroupBy,
        "version" => TokenKind::Version,
        "target" => TokenKind::Target,
        "description" => TokenKind::Description,
        "formal" => TokenKind::Formal,
        "as" => TokenKind::As,
        _ => TokenKind::Ident(text.to_string()),
    }
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper: tokenize and return only non-Newline, non-Eof, non-Comment kinds.
    fn token_kinds(source: &str) -> Vec<TokenKind> {
        let mut lexer = Lexer::new(source);
        let tokens = lexer.tokenize().expect("lex error");
        tokens
            .into_iter()
            .filter(|t| {
                !matches!(
                    t.kind,
                    TokenKind::Newline | TokenKind::Eof | TokenKind::Comment(_)
                )
            })
            .map(|t| t.kind)
            .collect()
    }

    #[test]
    fn test_keywords() {
        let kinds = token_kinds("graph node edge solve function type let module model meta");
        assert_eq!(
            kinds,
            vec![
                TokenKind::Graph,
                TokenKind::Node,
                TokenKind::Edge,
                TokenKind::Solve,
                TokenKind::Function,
                TokenKind::Type,
                TokenKind::Let,
                TokenKind::Module,
                TokenKind::Model,
                TokenKind::Meta,
            ]
        );
    }

    #[test]
    fn test_more_keywords() {
        let kinds = token_kinds(
            "pre post invariant constraint goal domain strategy input output properties",
        );
        assert_eq!(
            kinds,
            vec![
                TokenKind::Pre,
                TokenKind::Post,
                TokenKind::Invariant,
                TokenKind::Constraint,
                TokenKind::Goal,
                TokenKind::Domain,
                TokenKind::Strategy,
                TokenKind::Input,
                TokenKind::Output,
                TokenKind::Properties,
            ]
        );
    }

    #[test]
    fn test_logic_keywords() {
        let kinds =
            token_kinds("forall exists implies and or not in not_in where if then else match");
        assert_eq!(
            kinds,
            vec![
                TokenKind::ForAll,
                TokenKind::Exists,
                TokenKind::Implies,
                TokenKind::And,
                TokenKind::Or,
                TokenKind::Not,
                TokenKind::In,
                TokenKind::NotIn,
                TokenKind::Where,
                TokenKind::If,
                TokenKind::Then,
                TokenKind::Else,
                TokenKind::Match,
            ]
        );
    }

    #[test]
    fn test_numbers_int() {
        let kinds = token_kinds("42 0 1000");
        assert_eq!(
            kinds,
            vec![
                TokenKind::IntLit(42),
                TokenKind::IntLit(0),
                TokenKind::IntLit(1000),
            ]
        );
    }

    #[test]
    fn test_numbers_float() {
        let kinds = token_kinds("3.14 0.99 100.0");
        assert_eq!(
            kinds,
            vec![
                TokenKind::FloatLit(3.14),
                TokenKind::FloatLit(0.99),
                TokenKind::FloatLit(100.0),
            ]
        );
    }

    #[test]
    fn test_strings() {
        let kinds = token_kinds(r#""hello" "world" "foo bar""#);
        assert_eq!(
            kinds,
            vec![
                TokenKind::StringLit("hello".into()),
                TokenKind::StringLit("world".into()),
                TokenKind::StringLit("foo bar".into()),
            ]
        );
    }

    #[test]
    fn test_string_escapes() {
        let kinds = token_kinds(r#""hello\nworld" "tab\there" "quote\"end""#);
        assert_eq!(
            kinds,
            vec![
                TokenKind::StringLit("hello\nworld".into()),
                TokenKind::StringLit("tab\there".into()),
                TokenKind::StringLit("quote\"end".into()),
            ]
        );
    }

    #[test]
    fn test_unterminated_string() {
        let mut lexer = Lexer::new(r#""hello"#);
        let result = lexer.tokenize();
        assert!(result.is_err());
        let errors = result.unwrap_err();
        assert!(matches!(
            errors[0],
            LexError::UnterminatedString { .. }
        ));
    }

    #[test]
    fn test_unicode_operators() {
        let kinds = token_kinds("σ π ⋈ γ λ ∀ ∃ ∈ ∉ ⟹ ∧ ∨ ≤ ≥ ≠ → ← Σ Π ℝ ℤ ℕ ≈");
        assert_eq!(
            kinds,
            vec![
                TokenKind::Sigma,
                TokenKind::Pi,
                TokenKind::Bowtie,
                TokenKind::Gamma,
                TokenKind::Lambda,
                TokenKind::ForAllSym,
                TokenKind::ExistsSym,
                TokenKind::InSym,
                TokenKind::NotInSym,
                TokenKind::ImpliesSym,
                TokenKind::AndSym,
                TokenKind::OrSym,
                TokenKind::Leq,
                TokenKind::Geq,
                TokenKind::Neq,
                TokenKind::Arrow,
                TokenKind::LeftArrow,
                TokenKind::Sum,
                TokenKind::Prod,
                TokenKind::RealSet,
                TokenKind::IntSet,
                TokenKind::NatSet,
                TokenKind::Approx,
            ]
        );
    }

    #[test]
    fn test_ascii_fallback_operators() {
        let kinds = token_kinds("-> <- <= >= != ==");
        assert_eq!(
            kinds,
            vec![
                TokenKind::Arrow,
                TokenKind::LeftArrow,
                TokenKind::LtEq,
                TokenKind::GtEq,
                TokenKind::BangEq,
                TokenKind::EqEq,
            ]
        );
    }

    #[test]
    fn test_comments() {
        let mut lexer = Lexer::new("-- this is a comment\nlet x = 1");
        let tokens = lexer.tokenize().unwrap();
        assert!(matches!(&tokens[0].kind, TokenKind::Comment(c) if c == "this is a comment"));
    }

    #[test]
    fn test_unit_literals() {
        // "200.ms" should lex as IntLit(200) Dot Ident("ms")
        let kinds = token_kinds("200.ms");
        assert_eq!(
            kinds,
            vec![
                TokenKind::IntLit(200),
                TokenKind::Dot,
                TokenKind::Ident("ms".into()),
            ]
        );

        // "3.14" should lex as FloatLit(3.14)
        let kinds = token_kinds("3.14");
        assert_eq!(kinds, vec![TokenKind::FloatLit(3.14)]);

        // "4.GiB" should lex as IntLit(4) Dot Ident("GiB")
        let kinds = token_kinds("4.GiB");
        assert_eq!(
            kinds,
            vec![
                TokenKind::IntLit(4),
                TokenKind::Dot,
                TokenKind::Ident("GiB".into()),
            ]
        );
    }

    #[test]
    fn test_multichar_operators() {
        let kinds = token_kinds("|> :: .. ++ == !=");
        assert_eq!(
            kinds,
            vec![
                TokenKind::PipeRight,
                TokenKind::ColonColon,
                TokenKind::DotDot,
                TokenKind::PlusPlus,
                TokenKind::EqEq,
                TokenKind::BangEq,
            ]
        );
    }

    #[test]
    fn test_punctuation() {
        let kinds = token_kinds("( ) { } [ ] < > , : ; . @ ? | _");
        assert_eq!(
            kinds,
            vec![
                TokenKind::LParen,
                TokenKind::RParen,
                TokenKind::LBrace,
                TokenKind::RBrace,
                TokenKind::LBracket,
                TokenKind::RBracket,
                TokenKind::Lt,
                TokenKind::Gt,
                TokenKind::Comma,
                TokenKind::Colon,
                TokenKind::Semicolon,
                TokenKind::Dot,
                TokenKind::At,
                TokenKind::Question,
                TokenKind::Pipe,
                TokenKind::Underscore,
            ]
        );
    }

    #[test]
    fn test_arithmetic_operators() {
        let kinds = token_kinds("+ - * / % = ~");
        assert_eq!(
            kinds,
            vec![
                TokenKind::Plus,
                TokenKind::Minus,
                TokenKind::Star,
                TokenKind::Slash,
                TokenKind::Percent,
                TokenKind::Eq,
                TokenKind::Tilde,
            ]
        );
    }

    #[test]
    fn test_simple_graph() {
        let input = r#"graph MyApp { version: "1.0" }"#;
        let kinds = token_kinds(input);
        assert_eq!(
            kinds,
            vec![
                TokenKind::Graph,
                TokenKind::Ident("MyApp".into()),
                TokenKind::LBrace,
                TokenKind::Version,
                TokenKind::Colon,
                TokenKind::StringLit("1.0".into()),
                TokenKind::RBrace,
            ]
        );
    }

    #[test]
    fn test_graph_with_node_and_edge() {
        let input = r#"graph Test {
  node A {
    input: String
    output: Int
  }
  edge A -> B { buffered: true }
}"#;
        let kinds = token_kinds(input);
        assert_eq!(
            kinds,
            vec![
                TokenKind::Graph,
                TokenKind::Ident("Test".into()),
                TokenKind::LBrace,
                TokenKind::Node,
                TokenKind::Ident("A".into()),
                TokenKind::LBrace,
                TokenKind::Input,
                TokenKind::Colon,
                TokenKind::Ident("String".into()),
                TokenKind::Output,
                TokenKind::Colon,
                TokenKind::Ident("Int".into()),
                TokenKind::RBrace,
                TokenKind::Edge,
                TokenKind::Ident("A".into()),
                TokenKind::Arrow,
                TokenKind::Ident("B".into()),
                TokenKind::LBrace,
                TokenKind::Ident("buffered".into()),
                TokenKind::Colon,
                TokenKind::True,
                TokenKind::RBrace,
                TokenKind::RBrace,
            ]
        );
    }

    #[test]
    fn test_function_with_pre_post() {
        let input = "function transfer(from: Account, to: Account) -> Result {
  pre: { from.balance >= amount }
  post: { to.balance = to.balance + amount }
  synthesize(strategy: pessimistic_locking)
}";
        let kinds = token_kinds(input);
        // Verify it starts with function keyword and contains expected tokens
        assert_eq!(kinds[0], TokenKind::Function);
        assert_eq!(kinds[1], TokenKind::Ident("transfer".into()));
        assert_eq!(kinds[2], TokenKind::LParen);
        // Contains pre and post
        assert!(kinds.contains(&TokenKind::Pre));
        assert!(kinds.contains(&TokenKind::Post));
        assert!(kinds.contains(&TokenKind::Synthesize));
    }

    #[test]
    fn test_pipeline() {
        let kinds = token_kinds("Users |> sigma(verified = true) |> limit(100)");
        assert_eq!(kinds[0], TokenKind::Ident("Users".into()));
        assert_eq!(kinds[1], TokenKind::PipeRight);
        assert_eq!(kinds[2], TokenKind::Ident("sigma".into()));
        assert_eq!(kinds[3], TokenKind::LParen);
    }

    #[test]
    fn test_identifiers() {
        let kinds = token_kinds("my_var MyType _private foo123");
        assert_eq!(
            kinds,
            vec![
                TokenKind::Ident("my_var".into()),
                TokenKind::Ident("MyType".into()),
                TokenKind::Ident("_private".into()),
                TokenKind::Ident("foo123".into()),
            ]
        );
    }

    #[test]
    fn test_let_binding() {
        let kinds = token_kinds("let x = 42");
        assert_eq!(
            kinds,
            vec![
                TokenKind::Let,
                TokenKind::Ident("x".into()),
                TokenKind::Eq,
                TokenKind::IntLit(42),
            ]
        );
    }

    #[test]
    fn test_quantifier_expression() {
        let kinds = token_kinds("∀ x ∈ Request → response_time(x) < 200.ms");
        assert_eq!(kinds[0], TokenKind::ForAllSym);
        assert_eq!(kinds[1], TokenKind::Ident("x".into()));
        assert_eq!(kinds[2], TokenKind::InSym);
        assert_eq!(kinds[3], TokenKind::Ident("Request".into()));
        assert_eq!(kinds[4], TokenKind::Arrow);
    }

    #[test]
    fn test_relational_algebra() {
        let kinds = token_kinds("σ(age > 18 ∧ status = Active)");
        assert_eq!(kinds[0], TokenKind::Sigma);
        assert_eq!(kinds[1], TokenKind::LParen);
        assert_eq!(kinds[2], TokenKind::Ident("age".into()));
        assert_eq!(kinds[3], TokenKind::Gt);
        assert_eq!(kinds[4], TokenKind::IntLit(18));
        assert_eq!(kinds[5], TokenKind::AndSym);
    }

    #[test]
    fn test_do_block() {
        let kinds = token_kinds("do { user <- find_user(id)? yield user }");
        assert_eq!(kinds[0], TokenKind::Do);
        assert_eq!(kinds[1], TokenKind::LBrace);
        assert_eq!(kinds[2], TokenKind::Ident("user".into()));
        assert_eq!(kinds[3], TokenKind::LeftArrow);
        assert!(kinds.contains(&TokenKind::Question));
        assert!(kinds.contains(&TokenKind::Yield));
    }

    #[test]
    fn test_span_tracking() {
        let mut lexer = Lexer::new("let x");
        let tokens = lexer.tokenize().unwrap();
        assert_eq!(tokens[0].span, Span { start: 0, end: 3 });
        assert_eq!(tokens[0].text, "let");
        assert_eq!(tokens[1].span, Span { start: 4, end: 5 });
        assert_eq!(tokens[1].text, "x");
    }

    #[test]
    fn test_empty_input() {
        let mut lexer = Lexer::new("");
        let tokens = lexer.tokenize().unwrap();
        assert_eq!(tokens.len(), 1);
        assert!(matches!(tokens[0].kind, TokenKind::Eof));
    }

    #[test]
    fn test_unexpected_char() {
        let mut lexer = Lexer::new("let x = §");
        let result = lexer.tokenize();
        assert!(result.is_err());
    }

    #[test]
    fn test_newlines_tracked() {
        let mut lexer = Lexer::new("a\nb");
        let tokens = lexer.tokenize().unwrap();
        let kinds: Vec<_> = tokens.iter().map(|t| &t.kind).collect();
        assert_eq!(
            kinds,
            vec![
                &TokenKind::Ident("a".into()),
                &TokenKind::Newline,
                &TokenKind::Ident("b".into()),
                &TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn test_math_set_symbols() {
        let kinds = token_kinds("ℝ ℤ ℕ");
        assert_eq!(
            kinds,
            vec![TokenKind::RealSet, TokenKind::IntSet, TokenKind::NatSet,]
        );
    }

    #[test]
    fn test_model_keywords() {
        let kinds = token_kinds("model prior observe posterior branch");
        assert_eq!(
            kinds,
            vec![
                TokenKind::Model,
                TokenKind::Prior,
                TokenKind::Observe,
                TokenKind::Posterior,
                TokenKind::Branch,
            ]
        );
    }

    #[test]
    fn test_type_definition() {
        let kinds = token_kinds("type Nat = { n ∈ ℤ | n ≥ 0 }");
        assert_eq!(
            kinds,
            vec![
                TokenKind::Type,
                TokenKind::Ident("Nat".into()),
                TokenKind::Eq,
                TokenKind::LBrace,
                TokenKind::Ident("n".into()),
                TokenKind::InSym,
                TokenKind::IntSet,
                TokenKind::Pipe,
                TokenKind::Ident("n".into()),
                TokenKind::Geq,
                TokenKind::IntLit(0),
                TokenKind::RBrace,
            ]
        );
    }

    #[test]
    fn test_solve_block() {
        let kinds = token_kinds("solve { goal: minimize(latency) constraint: availability > 0.999 strategy: auto }");
        assert_eq!(kinds[0], TokenKind::Solve);
        assert!(kinds.contains(&TokenKind::Goal));
        assert!(kinds.contains(&TokenKind::Constraint));
        assert!(kinds.contains(&TokenKind::Strategy));
        assert!(kinds.contains(&TokenKind::FloatLit(0.999)));
    }

    #[test]
    fn test_complex_unit_expression() {
        // 5.s should be IntLit(5) Dot Ident("s")
        let kinds = token_kinds("5.s");
        assert_eq!(
            kinds,
            vec![
                TokenKind::IntLit(5),
                TokenKind::Dot,
                TokenKind::Ident("s".into()),
            ]
        );

        // 19.99.USD should be FloatLit(19.99) Dot Ident("USD")
        let kinds = token_kinds("19.99.USD");
        assert_eq!(
            kinds,
            vec![
                TokenKind::FloatLit(19.99),
                TokenKind::Dot,
                TokenKind::Ident("USD".into()),
            ]
        );
    }

    #[test]
    fn test_lone_underscore() {
        let kinds = token_kinds("match _ -> default");
        assert_eq!(
            kinds,
            vec![
                TokenKind::Match,
                TokenKind::Underscore,
                TokenKind::Arrow,
                TokenKind::Ident("default".into()),
            ]
        );
    }
}
