pub mod ast;
pub mod diagnostic;
pub mod lexer;
pub mod parser;
pub mod printer;
pub mod resolver;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_simple_graph_with_nodes_and_edges() {
        let source = r#"
graph HelloWorld {
  version: "0.1.0"
  target: [native_x86_64]

  node Greeter {
    input: String
    output: String

    semantic: {
      description: "Produces a greeting"
    }
  }

  node Printer {
    input: String

    properties: {
      idempotent: true
    }
  }

  edge Greeter -> Printer {
    buffered: true
  }
}
"#;
        let program = parser::parse(source).expect("parse failed");
        assert_eq!(program.len(), 1);

        let item = &program[0].node;
        match item {
            ast::Item::Graph(g) => {
                assert_eq!(g.name, "HelloWorld");
                assert_eq!(g.version.as_deref(), Some("0.1.0"));
                assert_eq!(g.targets, vec!["native_x86_64"]);

                // Count nodes and edges
                let nodes: Vec<_> = g
                    .members
                    .iter()
                    .filter(|m| matches!(&m.node, ast::GraphMember::Node(_)))
                    .collect();
                let edges: Vec<_> = g
                    .members
                    .iter()
                    .filter(|m| matches!(&m.node, ast::GraphMember::Edge(_)))
                    .collect();
                assert_eq!(nodes.len(), 2);
                assert_eq!(edges.len(), 1);

                // Check first node
                if let ast::GraphMember::Node(n) = &nodes[0].node {
                    assert_eq!(n.name, "Greeter");
                    // NodeDef now has `fields: Vec<NodeField>` -- check for Input/Output/Semantic
                    let has_input = n
                        .fields
                        .iter()
                        .any(|f| matches!(f, ast::NodeField::Input(_)));
                    let has_output = n
                        .fields
                        .iter()
                        .any(|f| matches!(f, ast::NodeField::Output(_)));
                    let has_semantic = n
                        .fields
                        .iter()
                        .any(|f| matches!(f, ast::NodeField::Semantic(_)));
                    assert!(has_input);
                    assert!(has_output);
                    assert!(has_semantic);
                    if let Some(ast::NodeField::Semantic(sem)) =
                        n.fields.iter().find(|f| matches!(f, ast::NodeField::Semantic(_)))
                    {
                        assert_eq!(sem.description.as_deref(), Some("Produces a greeting"));
                    }
                } else {
                    panic!("expected node");
                }

                // Check edge
                if let ast::GraphMember::Edge(e) = &edges[0].node {
                    assert_eq!(e.from, "Greeter");
                    assert_eq!(e.to, "Printer");
                    assert!(!e.properties.is_empty());
                } else {
                    panic!("expected edge");
                }
            }
            _ => panic!("expected graph"),
        }
    }

    #[test]
    fn parse_function_with_pre_post() {
        let source = r#"
function transfer(from: Account, to: Account, amount: Money) -> Result {
  pre: {
    from.balance >= amount
    amount > 0
  }

  post: {
    from.balance = from.balance - amount
  }

  synthesize(strategy: pessimistic_locking, optimize: latency)
}
"#;
        let program = parser::parse(source).expect("parse failed");
        assert_eq!(program.len(), 1);

        match &program[0].node {
            ast::Item::Function(f) => {
                assert_eq!(f.name, "transfer");
                assert_eq!(f.params.len(), 3);
                assert_eq!(f.params[0].name, "from");
                assert_eq!(f.params[1].name, "to");
                assert_eq!(f.params[2].name, "amount");
                assert!(f.return_type.is_some());
                // pre/post are now Vec<Spanned<Expr>> (not Option)
                assert_eq!(f.pre.len(), 2);
                assert_eq!(f.post.len(), 1);
                // body is FunctionBody::Synthesize
                match &f.body {
                    ast::FunctionBody::Synthesize(props) => {
                        assert_eq!(props.len(), 2);
                        assert_eq!(props[0].key, "strategy");
                        assert_eq!(props[1].key, "optimize");
                    }
                    _ => panic!("expected synthesize body"),
                }
            }
            _ => panic!("expected function"),
        }
    }

    #[test]
    fn parse_type_definition_refinement() {
        let source = r#"
type Nat = { n in Int | n >= 0 }
"#;
        let program = parser::parse(source).expect("parse failed");
        assert_eq!(program.len(), 1);

        match &program[0].node {
            ast::Item::TypeDef(td) => {
                assert_eq!(td.name, "Nat");
                // body is Spanned<TypeExpr> directly, TypeExpr::Refinement
                match &td.body.node {
                    ast::TypeExpr::Refinement { var, base, .. } => {
                        assert_eq!(var, "n");
                        match &base.node {
                            ast::TypeExpr::Named(name) => assert_eq!(name, "Int"),
                            _ => panic!("expected named type"),
                        }
                    }
                    _ => panic!("expected refinement type, got {:?}", td.body.node),
                }
            }
            _ => panic!("expected type def"),
        }
    }

    #[test]
    fn parse_type_definition_sum() {
        let source = r#"
type Result = Ok(value: T) | Err(message: String)
"#;
        let program = parser::parse(source).expect("parse failed");
        match &program[0].node {
            ast::Item::TypeDef(td) => {
                assert_eq!(td.name, "Result");
                // body is Spanned<TypeExpr>, TypeExpr::Sum
                match &td.body.node {
                    ast::TypeExpr::Sum(variants) => {
                        assert_eq!(variants.len(), 2);
                        assert_eq!(variants[0].name, "Ok");
                        assert_eq!(variants[1].name, "Err");
                    }
                    _ => panic!("expected sum type, got {:?}", td.body.node),
                }
            }
            _ => panic!("expected type def"),
        }
    }

    #[test]
    fn parse_let_binding_pipeline() {
        let source = r#"
let result = Users
  |> sigma(verified = true)
  |> limit(100)
"#;
        let program = parser::parse(source).expect("parse failed");
        assert_eq!(program.len(), 1);

        match &program[0].node {
            ast::Item::LetBinding(lb) => {
                assert_eq!(lb.name, "result");
                match &lb.value.node {
                    ast::Expr::Pipeline { left, right } => {
                        assert!(matches!(&right.node, ast::Expr::Call { .. }));
                        assert!(matches!(&left.node, ast::Expr::Pipeline { .. }));
                    }
                    _ => panic!("expected pipeline, got {:?}", lb.value.node),
                }
            }
            _ => panic!("expected let binding"),
        }
    }

    #[test]
    fn parse_solve_block() {
        let source = r#"
graph API {
  solve {
    goal: minimize(latency)
    constraint: availability > 0.999
    strategy: auto
  }
}
"#;
        let program = parser::parse(source).expect("parse failed");
        match &program[0].node {
            ast::Item::Graph(g) => {
                let solves: Vec<_> = g
                    .members
                    .iter()
                    .filter(|m| matches!(&m.node, ast::GraphMember::Solve(_)))
                    .collect();
                assert_eq!(solves.len(), 1);
                if let ast::GraphMember::Solve(s) = &solves[0].node {
                    assert!(s.goal.is_some());
                    assert_eq!(s.constraints.len(), 1);
                    assert!(s.strategy.is_some());
                }
            }
            _ => panic!("expected graph"),
        }
    }

    #[test]
    fn parse_do_block() {
        let source = r#"
let result = do {
  user <- find_user(id)
  order <- create_order(user, items)
  yield order
}
"#;
        let program = parser::parse(source).expect("parse failed");
        match &program[0].node {
            ast::Item::LetBinding(lb) => {
                match &lb.value.node {
                    ast::Expr::DoBlock(statements) => {
                        assert_eq!(statements.len(), 3);
                        // First is a bind (no try_unwrap in new AST)
                        match &statements[0].node {
                            ast::DoStatement::Bind { name, .. } => {
                                assert_eq!(name, "user");
                            }
                            _ => panic!("expected bind"),
                        }
                        // Last is yield
                        assert!(matches!(&statements[2].node, ast::DoStatement::Yield(_)));
                    }
                    _ => panic!("expected do block, got {:?}", lb.value.node),
                }
            }
            _ => panic!("expected let binding"),
        }
    }

    #[test]
    fn parse_quantifier_expression() {
        let source = r#"
let check = forall x in Request -> response_time(x) < 200.ms
"#;
        let program = parser::parse(source).expect("parse failed");
        match &program[0].node {
            ast::Item::LetBinding(lb) => {
                // New AST uses Expr::ForAll { var, domain, body }
                match &lb.value.node {
                    ast::Expr::ForAll { var, .. } => {
                        assert_eq!(var, "x");
                    }
                    _ => panic!("expected ForAll, got {:?}", lb.value.node),
                }
            }
            _ => panic!("expected let binding"),
        }
    }

    #[test]
    fn parse_unit_literals() {
        let source = r#"
let timeout = 200.ms
"#;
        let program = parser::parse(source).expect("parse failed");
        match &program[0].node {
            ast::Item::LetBinding(lb) => {
                // New AST uses Expr::WithUnit { value, unit }
                match &lb.value.node {
                    ast::Expr::WithUnit { unit, .. } => {
                        assert_eq!(unit, "ms");
                    }
                    _ => panic!("expected WithUnit, got {:?}", lb.value.node),
                }
            }
            _ => panic!("expected let binding"),
        }
    }

    #[test]
    fn parse_model_declaration() {
        let source = r#"
model UserBehavior {
  prior interest_vector: Dirichlet(alpha)
  observe clicks: data
  posterior = infer(method: variational)
}
"#;
        let program = parser::parse(source).expect("parse failed");
        match &program[0].node {
            ast::Item::Model(m) => {
                assert_eq!(m.name, "UserBehavior");
                // Model now has statements: Vec<Spanned<ModelStatement>>
                assert_eq!(m.statements.len(), 3);
                match &m.statements[0].node {
                    ast::ModelStatement::Prior { name, .. } => {
                        assert_eq!(name, "interest_vector");
                    }
                    _ => panic!("expected Prior"),
                }
                assert!(matches!(
                    &m.statements[1].node,
                    ast::ModelStatement::Observe { .. }
                ));
                assert!(matches!(
                    &m.statements[2].node,
                    ast::ModelStatement::Posterior(_)
                ));
            }
            _ => panic!("expected model"),
        }
    }

    #[test]
    fn parse_unicode_operators() {
        let source = r#"
let check = forall x in S -> x >= 0 and x <= 100
"#;
        let program = parser::parse(source).expect("parse failed");
        match &program[0].node {
            ast::Item::LetBinding(lb) => {
                assert!(matches!(&lb.value.node, ast::Expr::ForAll { .. }));
            }
            _ => panic!("expected let binding"),
        }
    }

    #[test]
    fn parse_lambda_expression() {
        let source = r#"
let f = fn(x: Int) -> x + 1
"#;
        let program = parser::parse(source).expect("parse failed");
        match &program[0].node {
            ast::Item::LetBinding(lb) => {
                // New AST: Lambda { params: Vec<Param>, body }
                match &lb.value.node {
                    ast::Expr::Lambda { params, .. } => {
                        assert_eq!(params.len(), 1);
                        assert_eq!(params[0].name, "x");
                    }
                    _ => panic!("expected lambda, got {:?}", lb.value.node),
                }
            }
            _ => panic!("expected let binding"),
        }
    }

    #[test]
    fn parse_import_whole_module() {
        let source = "import math\n";
        let program = parser::parse(source).expect("parse failed");
        assert_eq!(program.len(), 1);
        match &program[0].node {
            ast::Item::Import(imp) => {
                assert_eq!(imp.path, vec!["math"]);
                assert!(imp.names.is_none());
            }
            _ => panic!("expected import"),
        }
    }

    #[test]
    fn parse_import_dotted_path() {
        let source = "import std.math.trig\n";
        let program = parser::parse(source).expect("parse failed");
        match &program[0].node {
            ast::Item::Import(imp) => {
                assert_eq!(imp.path, vec!["std", "math", "trig"]);
                assert!(imp.names.is_none());
            }
            _ => panic!("expected import"),
        }
    }

    #[test]
    fn parse_import_selective() {
        let source = "import math.{sin, cos, tan}\n";
        let program = parser::parse(source).expect("parse failed");
        match &program[0].node {
            ast::Item::Import(imp) => {
                assert_eq!(imp.path, vec!["math"]);
                let names = imp.names.as_ref().unwrap();
                assert_eq!(names.len(), 3);
                assert_eq!(names[0].name, "sin");
                assert_eq!(names[1].name, "cos");
                assert_eq!(names[2].name, "tan");
                assert!(names.iter().all(|n| n.alias.is_none()));
            }
            _ => panic!("expected import"),
        }
    }

    #[test]
    fn parse_import_selective_with_alias() {
        let source = "import math.{sin as sine, cos}\n";
        let program = parser::parse(source).expect("parse failed");
        match &program[0].node {
            ast::Item::Import(imp) => {
                assert_eq!(imp.path, vec!["math"]);
                let names = imp.names.as_ref().unwrap();
                assert_eq!(names.len(), 2);
                assert_eq!(names[0].name, "sin");
                assert_eq!(names[0].alias.as_deref(), Some("sine"));
                assert_eq!(names[1].name, "cos");
                assert!(names[1].alias.is_none());
            }
            _ => panic!("expected import"),
        }
    }

    #[test]
    fn parse_import_nested_selective() {
        let source = "import std.collections.{List, Map}\n";
        let program = parser::parse(source).expect("parse failed");
        match &program[0].node {
            ast::Item::Import(imp) => {
                assert_eq!(imp.path, vec!["std", "collections"]);
                let names = imp.names.as_ref().unwrap();
                assert_eq!(names.len(), 2);
                assert_eq!(names[0].name, "List");
                assert_eq!(names[1].name, "Map");
            }
            _ => panic!("expected import"),
        }
    }

    #[test]
    fn roundtrip_import_printer() {
        let source = "import std.math.{sin as sine, cos}\n";
        let program = parser::parse(source).expect("parse failed");
        let output = printer::print_program(&program);
        assert!(output.contains("import std.math.{sin as sine, cos}"));
    }
}
