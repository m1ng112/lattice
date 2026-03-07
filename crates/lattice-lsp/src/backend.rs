use std::collections::HashMap;
use std::sync::Mutex;

use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::*;
use tower_lsp::{Client, LanguageServer};

use lattice_parser::ast::*;
use lattice_parser::parser;
use lattice_proof::obligation;
use lattice_types::checker::TypeChecker;

// ── Offset → Position conversion ─────────────────────────

/// Precomputed line-start table for converting byte offsets to LSP positions.
struct LineIndex {
    /// Byte offset where each line begins.
    line_starts: Vec<usize>,
}

impl LineIndex {
    fn new(text: &str) -> Self {
        let mut line_starts = vec![0];
        for (i, ch) in text.char_indices() {
            if ch == '\n' {
                line_starts.push(i + 1);
            }
        }
        Self { line_starts }
    }

    /// Convert a byte offset to an LSP `Position` (0-based line and character).
    fn offset_to_position(&self, offset: usize) -> Position {
        let line = self
            .line_starts
            .partition_point(|&start| start <= offset)
            .saturating_sub(1);
        let col = offset.saturating_sub(self.line_starts[line]);
        Position::new(line as u32, col as u32)
    }

    fn span_to_range(&self, span: &Span) -> Range {
        Range::new(
            self.offset_to_position(span.start),
            self.offset_to_position(span.end),
        )
    }
}

// ── Backend ──────────────────────────────────────────────

pub struct LatticeBackend {
    client: Client,
    /// Source text per open document URI.
    documents: Mutex<HashMap<Url, String>>,
}

impl LatticeBackend {
    pub fn new(client: Client) -> Self {
        Self {
            client,
            documents: Mutex::new(HashMap::new()),
        }
    }

    /// Parse source, publish diagnostics, and return the AST (if parsing succeeded).
    async fn analyze(&self, uri: &Url, text: &str) -> Option<Program> {
        let index = LineIndex::new(text);
        let mut diagnostics = Vec::new();

        match parser::parse(text) {
            Ok(program) => {
                // Extract proof obligations and report as warnings.
                let obligations = obligation::extract_obligations(&program);
                for ob in &obligations {
                    let range = index.span_to_range(&ob.span);
                    diagnostics.push(Diagnostic {
                        range,
                        severity: Some(DiagnosticSeverity::WARNING),
                        source: Some("lattice-proof".into()),
                        message: format!("[{}] {}", ob.kind_label(), ob.name),
                        ..Default::default()
                    });
                }

                // Run type checker and report errors as diagnostics.
                let mut tc = TypeChecker::new();
                for item in &program {
                    match &item.node {
                        Item::LetBinding(lb) => {
                            match tc.synthesize(&lb.value) {
                                Ok(ty) => {
                                    tc.env.bind(lb.name.clone(), ty);
                                }
                                Err(e) => {
                                    let range = Range::new(
                                        index.offset_to_position(lb.value.span.start),
                                        index.offset_to_position(lb.value.span.end),
                                    );
                                    diagnostics.push(Diagnostic {
                                        range,
                                        severity: Some(DiagnosticSeverity::ERROR),
                                        source: Some("lattice-types".into()),
                                        message: e.to_string(),
                                        ..Default::default()
                                    });
                                }
                            }
                        }
                        Item::Function(f) => {
                            // Register function type in TC environment
                            use lattice_types::checker::convert_type_expr;
                            let param_types: Vec<lattice_types::types::Type> = f
                                .params
                                .iter()
                                .map(|p| convert_type_expr(&p.type_expr.node))
                                .collect();
                            let ret_type = f
                                .return_type
                                .as_ref()
                                .map(|t| convert_type_expr(&t.node))
                                .unwrap_or(lattice_types::types::Type::Unit);
                            let fn_type = lattice_types::types::Type::Function {
                                params: param_types,
                                return_type: Box::new(ret_type),
                            };
                            tc.env.bind(f.name.clone(), fn_type);
                        }
                        _ => {}
                    }
                }

                // Collect non-exhaustive match warnings from TC
                for err in tc.errors() {
                    if let lattice_types::checker::TypeError::NonExhaustiveMatch {
                        missing,
                        span,
                    } = err
                    {
                        let range = Range::new(
                            index.offset_to_position(span.start),
                            index.offset_to_position(span.end),
                        );
                        diagnostics.push(Diagnostic {
                            range,
                            severity: Some(DiagnosticSeverity::WARNING),
                            source: Some("lattice-types".into()),
                            message: format!("non-exhaustive match: missing {}", missing),
                            ..Default::default()
                        });
                    }
                }

                self.client
                    .publish_diagnostics(uri.clone(), diagnostics, None)
                    .await;
                Some(program)
            }
            Err(errors) => {
                for e in &errors {
                    let pos = index.offset_to_position(e.offset);
                    let range = Range::new(pos, pos);
                    diagnostics.push(Diagnostic {
                        range,
                        severity: Some(DiagnosticSeverity::ERROR),
                        source: Some("lattice-parser".into()),
                        message: e.message.clone(),
                        ..Default::default()
                    });
                }

                self.client
                    .publish_diagnostics(uri.clone(), diagnostics, None)
                    .await;
                None
            }
        }
    }
}

// ── LanguageServer implementation ────────────────────────

#[tower_lsp::async_trait]
impl LanguageServer for LatticeBackend {
    async fn initialize(&self, _: InitializeParams) -> Result<InitializeResult> {
        Ok(InitializeResult {
            capabilities: ServerCapabilities {
                text_document_sync: Some(TextDocumentSyncCapability::Kind(
                    TextDocumentSyncKind::FULL,
                )),
                hover_provider: Some(HoverProviderCapability::Simple(true)),
                completion_provider: Some(CompletionOptions {
                    trigger_characters: Some(vec![".".into(), ":".into()]),
                    ..Default::default()
                }),
                document_symbol_provider: Some(OneOf::Left(true)),
                definition_provider: Some(OneOf::Left(true)),
                ..Default::default()
            },
            ..Default::default()
        })
    }

    async fn initialized(&self, _: InitializedParams) {
        self.client
            .log_message(MessageType::INFO, "Lattice LSP initialized")
            .await;
    }

    async fn shutdown(&self) -> Result<()> {
        Ok(())
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        let uri = params.text_document.uri;
        let text = params.text_document.text;
        {
            let mut docs = self.documents.lock().unwrap();
            docs.insert(uri.clone(), text.clone());
        }
        self.analyze(&uri, &text).await;
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        let uri = params.text_document.uri;
        if let Some(change) = params.content_changes.into_iter().last() {
            {
                let mut docs = self.documents.lock().unwrap();
                docs.insert(uri.clone(), change.text.clone());
            }
            self.analyze(&uri, &change.text).await;
        }
    }

    async fn hover(&self, params: HoverParams) -> Result<Option<Hover>> {
        let uri = &params.text_document_position_params.text_document.uri;
        let pos = params.text_document_position_params.position;

        let text = {
            let docs = self.documents.lock().unwrap();
            docs.get(uri).cloned()
        };
        let Some(text) = text else {
            return Ok(None);
        };

        let program = match parser::parse(&text) {
            Ok(p) => p,
            Err(_) => return Ok(None),
        };

        let index = LineIndex::new(&text);
        // Convert LSP position to byte offset.
        let target_offset = position_to_offset(&text, pos);

        // Search for an item whose span contains the cursor.
        for item in &program {
            if !span_contains(&item.span, target_offset) {
                continue;
            }
            let content = match &item.node {
                Item::Function(f) => {
                    let params_str: Vec<String> = f
                        .params
                        .iter()
                        .map(|p| format!("{}: {}", p.name, type_expr_to_string(&p.type_expr.node)))
                        .collect();
                    let ret = f
                        .return_type
                        .as_ref()
                        .map(|t| type_expr_to_string(&t.node))
                        .unwrap_or_else(|| "Unit".into());
                    let mut hover = format!("**function** {}({}) -> {}", f.name, params_str.join(", "), ret);
                    if !f.pre.is_empty() {
                        hover.push_str("\n\n**pre:** ");
                        hover.push_str(&format!("{} condition(s)", f.pre.len()));
                    }
                    if !f.post.is_empty() {
                        hover.push_str("\n\n**post:** ");
                        hover.push_str(&format!("{} condition(s)", f.post.len()));
                    }
                    hover
                }
                Item::TypeDef(td) => {
                    format!("**type** {} = {}", td.name, type_expr_to_string(&td.body.node))
                }
                Item::Graph(g) => {
                    let node_count = g
                        .members
                        .iter()
                        .filter(|m| matches!(&m.node, GraphMember::Node(_)))
                        .count();
                    let edge_count = g
                        .members
                        .iter()
                        .filter(|m| matches!(&m.node, GraphMember::Edge(_)))
                        .count();
                    format!("**graph** {} ({} nodes, {} edges)", g.name, node_count, edge_count)
                }
                Item::LetBinding(lb) => {
                    let ty = lb
                        .type_ann
                        .as_ref()
                        .map(|t| format!(": {}", type_expr_to_string(&t.node)))
                        .or_else(|| {
                            // Try to infer the type via the type checker
                            let mut tc = TypeChecker::new();
                            let inferred = tc.synthesize(&lb.value).ok()?;
                            Some(format!(": {}", inferred))
                        })
                        .unwrap_or_default();
                    format!("**let** {}{}", lb.name, ty)
                }
                Item::Model(m) => format!("**model** {}", m.name),
                Item::Module(m) => format!("**module** {} ({} items)", m.name, m.items.len()),
                Item::Meta(m) => format!("**meta** {}", m.name),
            };

            return Ok(Some(Hover {
                contents: HoverContents::Markup(MarkupContent {
                    kind: MarkupKind::Markdown,
                    value: content,
                }),
                range: Some(index.span_to_range(&item.span)),
            }));
        }

        Ok(None)
    }

    async fn completion(&self, _: CompletionParams) -> Result<Option<CompletionResponse>> {
        let keywords = [
            "graph", "node", "edge", "solve", "function", "type", "let", "module", "model",
            "meta", "pre", "post", "invariant", "semantic", "synthesize", "input", "output",
            "properties", "proof_obligations", "forall", "exists", "do", "yield", "fn", "if",
            "else", "match", "true", "false", "and", "or", "not", "in",
        ];
        let builtin_types = [
            "Int", "Float", "String", "Bool", "Unit", "List", "Result", "Option",
            "Stream", "Distribution", "Vector", "Matrix", "Nat", "Money", "Account",
        ];

        let mut items: Vec<CompletionItem> = keywords
            .iter()
            .map(|kw| CompletionItem {
                label: kw.to_string(),
                kind: Some(CompletionItemKind::KEYWORD),
                ..Default::default()
            })
            .collect();

        items.extend(builtin_types.iter().map(|ty| CompletionItem {
            label: ty.to_string(),
            kind: Some(CompletionItemKind::TYPE_PARAMETER),
            ..Default::default()
        }));

        Ok(Some(CompletionResponse::Array(items)))
    }

    async fn document_symbol(
        &self,
        params: DocumentSymbolParams,
    ) -> Result<Option<DocumentSymbolResponse>> {
        let uri = &params.text_document.uri;
        let text = {
            let docs = self.documents.lock().unwrap();
            docs.get(uri).cloned()
        };
        let Some(text) = text else {
            return Ok(None);
        };

        let program = match parser::parse(&text) {
            Ok(p) => p,
            Err(_) => return Ok(None),
        };

        let index = LineIndex::new(&text);
        let mut symbols = Vec::new();

        for item in &program {
            let range = index.span_to_range(&item.span);
            match &item.node {
                Item::Function(f) => {
                    symbols.push(symbol_info(&f.name, SymbolKind::FUNCTION, range, uri));
                }
                Item::TypeDef(td) => {
                    symbols.push(symbol_info(&td.name, SymbolKind::TYPE_PARAMETER, range, uri));
                }
                Item::Graph(g) => {
                    symbols.push(symbol_info(&g.name, SymbolKind::CLASS, range, uri));
                    for member in &g.members {
                        let member_range = index.span_to_range(&member.span);
                        match &member.node {
                            GraphMember::Node(n) => {
                                symbols.push(symbol_info(
                                    &n.name,
                                    SymbolKind::STRUCT,
                                    member_range,
                                    uri,
                                ));
                            }
                            GraphMember::Edge(e) => {
                                let label = format!("{} -> {}", e.from, e.to);
                                symbols.push(symbol_info(
                                    &label,
                                    SymbolKind::INTERFACE,
                                    member_range,
                                    uri,
                                ));
                            }
                            GraphMember::Solve(_) => {
                                symbols.push(symbol_info(
                                    "solve",
                                    SymbolKind::EVENT,
                                    member_range,
                                    uri,
                                ));
                            }
                        }
                    }
                }
                Item::LetBinding(lb) => {
                    symbols.push(symbol_info(&lb.name, SymbolKind::VARIABLE, range, uri));
                }
                Item::Module(m) => {
                    symbols.push(symbol_info(&m.name, SymbolKind::MODULE, range, uri));
                }
                Item::Model(m) => {
                    symbols.push(symbol_info(&m.name, SymbolKind::CLASS, range, uri));
                }
                Item::Meta(m) => {
                    symbols.push(symbol_info(&m.name, SymbolKind::PROPERTY, range, uri));
                }
            }
        }

        #[allow(deprecated)] // SymbolInformation::location
        Ok(Some(DocumentSymbolResponse::Flat(symbols)))
    }

    async fn goto_definition(
        &self,
        params: GotoDefinitionParams,
    ) -> Result<Option<GotoDefinitionResponse>> {
        let uri = &params.text_document_position_params.text_document.uri;
        let pos = params.text_document_position_params.position;

        let text = {
            let docs = self.documents.lock().unwrap();
            docs.get(uri).cloned()
        };
        let Some(text) = text else {
            return Ok(None);
        };

        let program = match parser::parse(&text) {
            Ok(p) => p,
            Err(_) => return Ok(None),
        };

        let target_offset = position_to_offset(&text, pos);

        // Find the identifier at cursor by scanning for a word boundary around the offset.
        let ident = extract_ident_at(&text, target_offset);
        let Some(ident) = ident else {
            return Ok(None);
        };

        let index = LineIndex::new(&text);

        // Search for a top-level definition matching the identifier.
        for item in &program {
            let name = match &item.node {
                Item::Function(f) => &f.name,
                Item::TypeDef(td) => &td.name,
                Item::Graph(g) => &g.name,
                Item::LetBinding(lb) => &lb.name,
                Item::Module(m) => &m.name,
                Item::Model(m) => &m.name,
                Item::Meta(m) => &m.name,
            };
            if name == &ident {
                let range = index.span_to_range(&item.span);
                return Ok(Some(GotoDefinitionResponse::Scalar(Location::new(
                    uri.clone(),
                    range,
                ))));
            }
            // Also search inside graphs for node names.
            if let Item::Graph(g) = &item.node {
                for member in &g.members {
                    if let GraphMember::Node(n) = &member.node {
                        if n.name == ident {
                            let range = index.span_to_range(&member.span);
                            return Ok(Some(GotoDefinitionResponse::Scalar(Location::new(
                                uri.clone(),
                                range,
                            ))));
                        }
                    }
                }
            }
        }

        Ok(None)
    }
}

// ── Helpers ──────────────────────────────────────────────

fn position_to_offset(text: &str, pos: Position) -> usize {
    let mut line = 0u32;
    let mut col = 0u32;
    for (i, ch) in text.char_indices() {
        if line == pos.line && col == pos.character {
            return i;
        }
        if ch == '\n' {
            line += 1;
            col = 0;
        } else {
            col += 1;
        }
    }
    text.len()
}

fn span_contains(span: &Span, offset: usize) -> bool {
    offset >= span.start && offset < span.end
}

fn extract_ident_at(text: &str, offset: usize) -> Option<String> {
    if offset >= text.len() {
        return None;
    }
    let bytes = text.as_bytes();
    if !is_ident_char(bytes[offset]) {
        return None;
    }
    let mut start = offset;
    while start > 0 && is_ident_char(bytes[start - 1]) {
        start -= 1;
    }
    let mut end = offset;
    while end < bytes.len() && is_ident_char(bytes[end]) {
        end += 1;
    }
    Some(text[start..end].to_string())
}

fn is_ident_char(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

#[allow(deprecated)] // SymbolInformation::deprecated field
fn symbol_info(name: &str, kind: SymbolKind, range: Range, uri: &Url) -> SymbolInformation {
    SymbolInformation {
        name: name.to_string(),
        kind,
        tags: None,
        deprecated: None,
        location: Location::new(uri.clone(), range),
        container_name: None,
    }
}

fn type_expr_to_string(te: &TypeExpr) -> String {
    match te {
        TypeExpr::Named(n) => n.clone(),
        TypeExpr::Applied { name, args } => {
            let args_str: Vec<String> = args.iter().map(|a| type_expr_to_string(&a.node)).collect();
            format!("{}<{}>", name, args_str.join(", "))
        }
        TypeExpr::Function { params, ret } => {
            let ps: Vec<String> = params.iter().map(|p| type_expr_to_string(&p.node)).collect();
            format!("({}) -> {}", ps.join(", "), type_expr_to_string(&ret.node))
        }
        TypeExpr::Record(fields) => {
            let fs: Vec<String> = fields
                .iter()
                .map(|(n, t)| format!("{}: {}", n, type_expr_to_string(&t.node)))
                .collect();
            format!("{{ {} }}", fs.join(", "))
        }
        TypeExpr::Sum(variants) => {
            let vs: Vec<String> = variants.iter().map(|v| v.name.clone()).collect();
            vs.join(" | ")
        }
        TypeExpr::Refinement { var, base, .. } => {
            format!("{{ {} in {} | ... }}", var, type_expr_to_string(&base.node))
        }
        TypeExpr::Dependent { name, params } => {
            let ps: Vec<String> = params
                .iter()
                .map(|(n, t)| format!("{}: {}", n, type_expr_to_string(&t.node)))
                .collect();
            format!("{}({})", name, ps.join(", "))
        }
        TypeExpr::Stream(inner) => format!("Stream<{}>", type_expr_to_string(&inner.node)),
        TypeExpr::Distribution(inner) => {
            format!("Distribution<{}>", type_expr_to_string(&inner.node))
        }
        TypeExpr::Where { base, .. } => {
            format!("{} where ...", type_expr_to_string(&base.node))
        }
    }
}

/// Label for proof obligation kind, used in diagnostics.
trait ObligationLabel {
    fn kind_label(&self) -> &'static str;
}

impl ObligationLabel for obligation::ProofObligation {
    fn kind_label(&self) -> &'static str {
        match &self.kind {
            obligation::ObligationKind::Precondition => "precondition",
            obligation::ObligationKind::Postcondition => "postcondition",
            obligation::ObligationKind::Invariant => "invariant",
            obligation::ObligationKind::ProofObligation => "proof obligation",
            obligation::ObligationKind::TypeRefinement => "type refinement",
            obligation::ObligationKind::Conservation => "conservation",
            obligation::ObligationKind::Exhaustiveness => "exhaustiveness",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── LineIndex tests ──────────────────────────────────

    #[test]
    fn line_index_single_line() {
        let idx = LineIndex::new("hello world");
        assert_eq!(idx.offset_to_position(0), Position::new(0, 0));
        assert_eq!(idx.offset_to_position(5), Position::new(0, 5));
    }

    #[test]
    fn line_index_multi_line() {
        let text = "line1\nline2\nline3";
        let idx = LineIndex::new(text);
        assert_eq!(idx.offset_to_position(0), Position::new(0, 0));
        assert_eq!(idx.offset_to_position(6), Position::new(1, 0));
        assert_eq!(idx.offset_to_position(12), Position::new(2, 0));
        assert_eq!(idx.offset_to_position(14), Position::new(2, 2));
    }

    // ── Diagnostics tests ────────────────────────────────

    #[test]
    fn diagnostics_from_parse_error() {
        let src = "graph {"; // missing name
        let result = parser::parse(src);
        assert!(result.is_err());
        let errors = result.unwrap_err();
        assert!(!errors.is_empty());
    }

    #[test]
    fn diagnostics_from_valid_source() {
        let src = r#"
graph Hello {
  node A {
    input: Int
    output: Int
  }
}
"#;
        let result = parser::parse(src);
        assert!(result.is_ok());
    }

    // ── Document symbol extraction ───────────────────────

    #[test]
    fn extract_document_symbols() {
        let src = r#"
function add(a: Int, b: Int) -> Int {
  a + b
}
type Nat = { n in Int | n >= 0 }
let x = 42
"#;
        let program = parser::parse(src).unwrap();

        let mut names = Vec::new();
        for item in &program {
            match &item.node {
                Item::Function(f) => names.push(("function", f.name.clone())),
                Item::TypeDef(td) => names.push(("type", td.name.clone())),
                Item::LetBinding(lb) => names.push(("let", lb.name.clone())),
                _ => {}
            }
        }

        assert_eq!(names.len(), 3);
        assert_eq!(names[0], ("function", "add".to_string()));
        assert_eq!(names[1], ("type", "Nat".to_string()));
        assert_eq!(names[2], ("let", "x".to_string()));
    }

    // ── Hover content ────────────────────────────────────

    #[test]
    fn hover_function_signature() {
        let src = r#"
function transfer(from: Account, to: Account, amount: Money) -> Result {
  pre: {
    from.balance >= amount
  }
  synthesize(strategy: auto)
}
"#;
        let program = parser::parse(src).unwrap();
        let func = match &program[0].node {
            Item::Function(f) => f,
            _ => panic!("expected function"),
        };

        let params_str: Vec<String> = func
            .params
            .iter()
            .map(|p| format!("{}: {}", p.name, type_expr_to_string(&p.type_expr.node)))
            .collect();
        let ret = func
            .return_type
            .as_ref()
            .map(|t| type_expr_to_string(&t.node))
            .unwrap_or_else(|| "Unit".into());
        let sig = format!("function {}({}) -> {}", func.name, params_str.join(", "), ret);

        assert!(sig.contains("transfer"));
        assert!(sig.contains("from: Account"));
        assert!(sig.contains("Result"));
    }

    // ── Completion ───────────────────────────────────────

    #[test]
    fn completion_includes_keywords() {
        let keywords = [
            "graph", "node", "edge", "function", "type", "let", "solve",
        ];
        // Verify our keyword list is comprehensive.
        for kw in &keywords {
            assert!(
                [
                    "graph", "node", "edge", "solve", "function", "type", "let", "module",
                    "model", "meta", "pre", "post", "invariant", "semantic", "synthesize",
                    "input", "output", "properties", "proof_obligations", "forall", "exists",
                    "do", "yield", "fn", "if", "else", "match", "true", "false", "and", "or",
                    "not", "in",
                ]
                .contains(kw),
                "keyword {kw} missing from completion list"
            );
        }
    }

    // ── Go-to-definition ─────────────────────────────────

    #[test]
    fn goto_definition_finds_function() {
        let src = r#"
function add(a: Int, b: Int) -> Int {
  a + b
}
let result = add(1, 2)
"#;
        let program = parser::parse(src).unwrap();
        let ident = "add";

        let found = program.iter().find(|item| match &item.node {
            Item::Function(f) => f.name == ident,
            _ => false,
        });

        assert!(found.is_some());
    }

    // ── Identity extraction ──────────────────────────────

    #[test]
    fn extract_ident_at_offset() {
        let text = "let foo = bar + baz";
        assert_eq!(extract_ident_at(text, 4), Some("foo".to_string()));
        assert_eq!(extract_ident_at(text, 10), Some("bar".to_string()));
        assert_eq!(extract_ident_at(text, 16), Some("baz".to_string()));
        assert_eq!(extract_ident_at(text, 8), None); // space
    }

    // ── Type inference in hover ──────────────────────────

    #[test]
    fn hover_let_shows_inferred_type() {
        let src = "let x = 42";
        let program = parser::parse(src).unwrap();
        let lb = match &program[0].node {
            Item::LetBinding(lb) => lb,
            _ => panic!("expected let binding"),
        };

        let mut tc = TypeChecker::new();
        let inferred = tc.synthesize(&lb.value).unwrap();
        assert_eq!(format!("{}", inferred), "Int");
    }

    #[test]
    fn hover_let_string_type() {
        let src = r#"let greeting = "hello""#;
        let program = parser::parse(src).unwrap();
        let lb = match &program[0].node {
            Item::LetBinding(lb) => lb,
            _ => panic!("expected let binding"),
        };

        let mut tc = TypeChecker::new();
        let inferred = tc.synthesize(&lb.value).unwrap();
        assert_eq!(format!("{}", inferred), "String");
    }

    #[test]
    fn hover_let_array_type() {
        let src = "let xs = [1, 2, 3]";
        let program = parser::parse(src).unwrap();
        let lb = match &program[0].node {
            Item::LetBinding(lb) => lb,
            _ => panic!("expected let binding"),
        };

        let mut tc = TypeChecker::new();
        let inferred = tc.synthesize(&lb.value).unwrap();
        assert_eq!(format!("{}", inferred), "[Int]");
    }

    // ── Span helpers ─────────────────────────────────────

    #[test]
    fn span_contains_works() {
        let span = Span::new(5, 10);
        assert!(span_contains(&span, 5));
        assert!(span_contains(&span, 9));
        assert!(!span_contains(&span, 10));
        assert!(!span_contains(&span, 4));
    }

    // ── Position to offset ───────────────────────────────

    #[test]
    fn position_to_offset_conversion() {
        let text = "abc\ndef\nghi";
        assert_eq!(position_to_offset(text, Position::new(0, 0)), 0);
        assert_eq!(position_to_offset(text, Position::new(1, 0)), 4);
        assert_eq!(position_to_offset(text, Position::new(2, 2)), 10);
    }
}
