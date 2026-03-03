//! Proof obligation extraction from the Lattice AST.
//!
//! Walks a parsed [`Program`] and collects every proof obligation
//! (pre/post conditions, invariants, explicit proof obligations,
//! refinement type predicates) into a flat list of [`ProofObligation`]s.

use crate::status::ProofStatus;
use lattice_parser::ast::*;

/// A proof obligation extracted from the AST.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ProofObligation {
    pub id: String,
    pub name: String,
    pub kind: ObligationKind,
    pub source: ObligationSource,
    pub condition: Condition,
    pub status: ProofStatus,
    pub span: Span,
}

/// The kind of proof obligation.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ObligationKind {
    Precondition,
    Postcondition,
    Invariant,
    /// Explicit `proof_obligations` block on a node.
    ProofObligation,
    /// Refinement type constraint: `{ x in T | predicate }`.
    TypeRefinement,
    /// Conservation law (e.g. balance preservation).
    Conservation,
    /// Pattern match exhaustiveness.
    Exhaustiveness,
}

/// Where the obligation originated.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ObligationSource {
    /// Name of the enclosing item (function / node / type).
    pub item_name: String,
    /// Kind of the enclosing item: `"function"`, `"node"`, `"type"`.
    pub item_kind: String,
    /// Source file, if known.
    pub file: Option<String>,
}

/// Represents a condition to be proved.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum Condition {
    /// An expression lifted straight from the AST.
    Expr(Spanned<Expr>),
    /// Universal quantification: forall var in domain -> body.
    ForAll {
        var: String,
        domain: Box<Condition>,
        body: Box<Condition>,
    },
    /// Existential quantification: exists var in domain -> body.
    Exists {
        var: String,
        domain: Box<Condition>,
        body: Box<Condition>,
    },
    /// Implication: antecedent => consequent.
    Implies {
        antecedent: Box<Condition>,
        consequent: Box<Condition>,
    },
    /// Conjunction: a /\ b /\ ...
    And(Vec<Condition>),
    /// Equality: a = b.
    Equals(Box<Condition>, Box<Condition>),
    /// Comparison: left op right.
    Compare {
        left: Box<Condition>,
        op: CompareOp,
        right: Box<Condition>,
    },
    /// Named reference to another obligation.
    Ref(String),
}

/// Comparison operators for [`Condition::Compare`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum CompareOp {
    Lt,
    Leq,
    Gt,
    Geq,
    Eq,
    Neq,
}

/// Extract all proof obligations from a parsed [`Program`].
pub fn extract_obligations(program: &Program) -> Vec<ProofObligation> {
    let mut obligations = Vec::new();
    let mut id_counter: usize = 0;

    for item in program {
        match &item.node {
            Item::Function(func) => {
                extract_function_obligations(func, &mut obligations, &mut id_counter);
            }
            Item::Graph(graph) => {
                extract_graph_obligations(graph, &mut obligations, &mut id_counter);
            }
            Item::TypeDef(td) => {
                extract_typedef_obligations(td, &mut obligations, &mut id_counter);
            }
            _ => {}
        }
    }

    obligations
}

fn next_id(counter: &mut usize) -> String {
    let id = format!("po_{counter}");
    *counter += 1;
    id
}

fn extract_function_obligations(
    func: &Function,
    obligations: &mut Vec<ProofObligation>,
    id: &mut usize,
) {
    let source = ObligationSource {
        item_name: func.name.clone(),
        item_kind: "function".to_string(),
        file: None,
    };

    // Preconditions
    for (i, pre) in func.pre.iter().enumerate() {
        obligations.push(ProofObligation {
            id: next_id(id),
            name: format!("{}.pre[{i}]", func.name),
            kind: ObligationKind::Precondition,
            source: source.clone(),
            condition: Condition::Expr(pre.clone()),
            status: ProofStatus::Unverified,
            span: pre.span.clone(),
        });
    }

    // Postconditions
    for (i, post) in func.post.iter().enumerate() {
        obligations.push(ProofObligation {
            id: next_id(id),
            name: format!("{}.post[{i}]", func.name),
            kind: ObligationKind::Postcondition,
            source: source.clone(),
            condition: Condition::Expr(post.clone()),
            status: ProofStatus::Unverified,
            span: post.span.clone(),
        });
    }

    // Invariants
    for (i, inv) in func.invariants.iter().enumerate() {
        obligations.push(ProofObligation {
            id: next_id(id),
            name: format!("{}.invariant[{i}]", func.name),
            kind: ObligationKind::Invariant,
            source: source.clone(),
            condition: Condition::Expr(inv.clone()),
            status: ProofStatus::Unverified,
            span: inv.span.clone(),
        });
    }
}

fn extract_graph_obligations(
    graph: &Graph,
    obligations: &mut Vec<ProofObligation>,
    id: &mut usize,
) {
    for member in &graph.members {
        if let GraphMember::Node(node) = &member.node {
            let source = ObligationSource {
                item_name: node.name.clone(),
                item_kind: "node".to_string(),
                file: None,
            };

            for field in &node.fields {
                match field {
                    NodeField::Pre(pres) => {
                        for (i, pre) in pres.iter().enumerate() {
                            obligations.push(ProofObligation {
                                id: next_id(id),
                                name: format!("{}.pre[{i}]", node.name),
                                kind: ObligationKind::Precondition,
                                source: source.clone(),
                                condition: Condition::Expr(pre.clone()),
                                status: ProofStatus::Unverified,
                                span: pre.span.clone(),
                            });
                        }
                    }
                    NodeField::Post(posts) => {
                        for (i, post) in posts.iter().enumerate() {
                            obligations.push(ProofObligation {
                                id: next_id(id),
                                name: format!("{}.post[{i}]", node.name),
                                kind: ObligationKind::Postcondition,
                                source: source.clone(),
                                condition: Condition::Expr(post.clone()),
                                status: ProofStatus::Unverified,
                                span: post.span.clone(),
                            });
                        }
                    }
                    NodeField::ProofObligations(pos) => {
                        for po in pos {
                            obligations.push(ProofObligation {
                                id: next_id(id),
                                name: format!("{}.proof.{}", node.name, po.name),
                                kind: ObligationKind::ProofObligation,
                                source: source.clone(),
                                condition: Condition::Expr(po.expr.clone()),
                                status: ProofStatus::Unverified,
                                span: po.expr.span.clone(),
                            });
                        }
                    }
                    _ => {}
                }
            }
        }
    }
}

fn extract_typedef_obligations(
    td: &TypeDef,
    obligations: &mut Vec<ProofObligation>,
    id: &mut usize,
) {
    if let TypeExpr::Refinement {
        var: _,
        base: _,
        predicate,
    } = &td.body.node
    {
        let source = ObligationSource {
            item_name: td.name.clone(),
            item_kind: "type".to_string(),
            file: None,
        };

        obligations.push(ProofObligation {
            id: next_id(id),
            name: format!("{}.refinement", td.name),
            kind: ObligationKind::TypeRefinement,
            source,
            condition: Condition::Expr(*predicate.clone()),
            status: ProofStatus::Unverified,
            span: predicate.span.clone(),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lattice_parser::parser;

    #[test]
    fn extract_function_pre_post() {
        let src = r#"
function transfer(from: Account, to: Account, amount: Money) -> Result {
  pre: {
    from.balance >= amount
    amount > 0
  }
  post: {
    from.balance = from.balance - amount
  }
  synthesize(strategy: pessimistic_locking)
}
"#;
        let program = parser::parse(src).unwrap();
        let obs = extract_obligations(&program);

        assert_eq!(obs.len(), 3);

        assert_eq!(obs[0].kind, ObligationKind::Precondition);
        assert_eq!(obs[0].name, "transfer.pre[0]");
        assert_eq!(obs[0].source.item_kind, "function");

        assert_eq!(obs[1].kind, ObligationKind::Precondition);
        assert_eq!(obs[1].name, "transfer.pre[1]");

        assert_eq!(obs[2].kind, ObligationKind::Postcondition);
        assert_eq!(obs[2].name, "transfer.post[0]");
    }

    #[test]
    fn extract_function_invariants() {
        let src = r#"
function check(x: Int) -> Bool {
  pre: { x > 0 }
  invariant: { x >= 0 }
  synthesize(strategy: auto)
}
"#;
        let program = parser::parse(src).unwrap();
        let obs = extract_obligations(&program);

        assert_eq!(obs.len(), 2);
        assert_eq!(obs[0].kind, ObligationKind::Precondition);
        assert_eq!(obs[1].kind, ObligationKind::Invariant);
        assert_eq!(obs[1].name, "check.invariant[0]");
    }

    #[test]
    fn extract_refinement_type() {
        let src = r#"
type Nat = { n in Int | n >= 0 }
"#;
        let program = parser::parse(src).unwrap();
        let obs = extract_obligations(&program);

        assert_eq!(obs.len(), 1);
        assert_eq!(obs[0].kind, ObligationKind::TypeRefinement);
        assert_eq!(obs[0].name, "Nat.refinement");
        assert_eq!(obs[0].source.item_kind, "type");
    }

    #[test]
    fn empty_program_produces_no_obligations() {
        let obs = extract_obligations(&vec![]);
        assert!(obs.is_empty());
    }

    #[test]
    fn extract_node_proof_obligations() {
        let src = r#"
graph Payment {
  node Validator {
    input: Transaction
    output: Result

    pre: { input.amount > 0 }
    post: { output != null }
    proof_obligations: {
      balance_preserved: input.total = output.total
    }
  }
}
"#;
        let program = parser::parse(src).unwrap();
        let obs = extract_obligations(&program);

        // pre + post + 1 proof obligation = 3
        assert_eq!(obs.len(), 3);
        assert_eq!(obs[0].kind, ObligationKind::Precondition);
        assert_eq!(obs[0].name, "Validator.pre[0]");
        assert_eq!(obs[1].kind, ObligationKind::Postcondition);
        assert_eq!(obs[1].name, "Validator.post[0]");
        assert_eq!(obs[2].kind, ObligationKind::ProofObligation);
        assert_eq!(obs[2].name, "Validator.proof.balance_preserved");
    }

    #[test]
    fn obligation_ids_are_unique() {
        let src = r#"
function a(x: Int) -> Int {
  pre: { x > 0 }
  post: { result > 0 }
  synthesize(strategy: auto)
}
function b(y: Int) -> Int {
  pre: { y >= 0 }
  synthesize(strategy: auto)
}
"#;
        let program = parser::parse(src).unwrap();
        let obs = extract_obligations(&program);

        let ids: Vec<&str> = obs.iter().map(|o| o.id.as_str()).collect();
        let mut deduped = ids.clone();
        deduped.sort();
        deduped.dedup();
        assert_eq!(ids.len(), deduped.len(), "obligation IDs must be unique");
    }
}
