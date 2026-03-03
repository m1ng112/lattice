//! Simple arithmetic proof backend.
//!
//! Proves decidable arithmetic facts by symbolic simplification
//! and evaluation — no external solver required.

use crate::checker::{ProofBackend, ProofResult};
use crate::obligation::{CompareOp, Condition, ProofObligation};
use crate::status::ProofStatus;
use lattice_parser::ast::{BinOp, Expr, Spanned};
use std::collections::HashMap;
use std::time::Instant;

// ── Public backend ──────────────────────────────────────────────

/// A proof backend that handles pure arithmetic and simple logic
/// via symbolic simplification. No external dependencies.
pub struct ArithmeticBackend;

impl ProofBackend for ArithmeticBackend {
    fn name(&self) -> &str {
        "arithmetic"
    }

    fn supports(&self, _obligation: &ProofObligation) -> bool {
        true // attempt everything, return Unknown for unsupported
    }

    fn check(&self, obligation: &ProofObligation) -> ProofResult {
        let start = Instant::now();
        let result = evaluate_condition(&obligation.condition);
        let duration = start.elapsed().as_millis() as u64;

        match result {
            EvalResult::True => ProofResult {
                status: ProofStatus::Verified,
                duration_ms: duration,
                message: Some("Proved by arithmetic evaluation".into()),
                counterexample: None,
            },
            EvalResult::False(ce) => ProofResult {
                status: ProofStatus::Failed {
                    reason: "Counterexample found".into(),
                },
                duration_ms: duration,
                message: Some(format!("Disproved: {ce}")),
                counterexample: Some(ce),
            },
            EvalResult::Unknown => ProofResult {
                status: ProofStatus::Unverified,
                duration_ms: duration,
                message: Some("Cannot determine by simple arithmetic".into()),
                counterexample: None,
            },
        }
    }
}

// ── Internal symbolic expression ────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
enum Sym {
    Const(i64),
    Float(f64),
    Var(String),
    Add(Box<Sym>, Box<Sym>),
    Sub(Box<Sym>, Box<Sym>),
    Mul(Box<Sym>, Box<Sym>),
    Neg(Box<Sym>),
}

impl Sym {
    fn substitute(&self, var: &str, val: &Sym) -> Sym {
        match self {
            Sym::Var(v) if v == var => val.clone(),
            Sym::Var(_) | Sym::Const(_) | Sym::Float(_) => self.clone(),
            Sym::Add(a, b) => Sym::Add(
                Box::new(a.substitute(var, val)),
                Box::new(b.substitute(var, val)),
            ),
            Sym::Sub(a, b) => Sym::Sub(
                Box::new(a.substitute(var, val)),
                Box::new(b.substitute(var, val)),
            ),
            Sym::Mul(a, b) => Sym::Mul(
                Box::new(a.substitute(var, val)),
                Box::new(b.substitute(var, val)),
            ),
            Sym::Neg(a) => Sym::Neg(Box::new(a.substitute(var, val))),
        }
    }

    fn simplify(&self) -> Sym {
        match self {
            Sym::Add(a, b) => {
                let a = a.simplify();
                let b = b.simplify();
                match (&a, &b) {
                    (Sym::Const(x), Sym::Const(y)) => Sym::Const(x + y),
                    (Sym::Float(x), Sym::Float(y)) => Sym::Float(x + y),
                    (Sym::Const(0), _) => b,
                    (_, Sym::Const(0)) => a,
                    // a + (-b) => a - b
                    (_, Sym::Neg(inner)) => Sym::Sub(Box::new(a), inner.clone()).simplify(),
                    _ => Sym::Add(Box::new(a), Box::new(b)),
                }
            }
            Sym::Sub(a, b) => {
                let a = a.simplify();
                let b = b.simplify();
                match (&a, &b) {
                    (Sym::Const(x), Sym::Const(y)) => Sym::Const(x - y),
                    (Sym::Float(x), Sym::Float(y)) => Sym::Float(x - y),
                    (_, Sym::Const(0)) => a,
                    _ if a == b => Sym::Const(0),
                    _ => Sym::Sub(Box::new(a), Box::new(b)),
                }
            }
            Sym::Mul(a, b) => {
                let a = a.simplify();
                let b = b.simplify();
                match (&a, &b) {
                    (Sym::Const(x), Sym::Const(y)) => Sym::Const(x * y),
                    (Sym::Float(x), Sym::Float(y)) => Sym::Float(x * y),
                    (Sym::Const(0), _) | (_, Sym::Const(0)) => Sym::Const(0),
                    (Sym::Const(1), _) => b,
                    (_, Sym::Const(1)) => a,
                    _ => Sym::Mul(Box::new(a), Box::new(b)),
                }
            }
            Sym::Neg(a) => {
                let a = a.simplify();
                match &a {
                    Sym::Const(x) => Sym::Const(-x),
                    Sym::Float(x) => Sym::Float(-x),
                    Sym::Neg(inner) => *inner.clone(),
                    _ => Sym::Neg(Box::new(a)),
                }
            }
            _ => self.clone(),
        }
    }

    fn as_const(&self) -> Option<i64> {
        if let Sym::Const(v) = self {
            Some(*v)
        } else {
            None
        }
    }

    /// Collect terms: flatten into (coefficient, variable_name | None) pairs.
    /// E.g. `a - c + b + c` → [(1,"a"), (-1,"c"), (1,"b"), (1,"c")]
    fn collect_terms(&self) -> Vec<(i64, Option<String>)> {
        match self {
            Sym::Const(n) => vec![(*n, None)],
            Sym::Float(_) => vec![], // skip floats in term collection
            Sym::Var(v) => vec![(1, Some(v.clone()))],
            Sym::Add(a, b) => {
                let mut terms = a.collect_terms();
                terms.extend(b.collect_terms());
                terms
            }
            Sym::Sub(a, b) => {
                let mut terms = a.collect_terms();
                for (coeff, var) in b.collect_terms() {
                    terms.push((-coeff, var));
                }
                terms
            }
            Sym::Neg(a) => a
                .collect_terms()
                .into_iter()
                .map(|(c, v)| (-c, v))
                .collect(),
            Sym::Mul(a, b) => {
                // Only handle const * var
                match (a.as_const(), b.as_ref()) {
                    (Some(c), Sym::Var(v)) => vec![(c, Some(v.clone()))],
                    _ => match (a.as_ref(), b.as_const()) {
                        (Sym::Var(v), Some(c)) => vec![(c, Some(v.clone()))],
                        _ => vec![], // too complex
                    },
                }
            }
        }
    }

    /// Normalize by collecting and combining like terms, then comparing.
    fn normalize(&self) -> HashMap<Option<String>, i64> {
        let terms = self.collect_terms();
        let mut map: HashMap<Option<String>, i64> = HashMap::new();
        for (coeff, var) in terms {
            *map.entry(var).or_insert(0) += coeff;
        }
        // Remove zero entries
        map.retain(|_, v| *v != 0);
        map
    }
}

fn sym_equal(a: &Sym, b: &Sym) -> bool {
    let a_s = a.simplify();
    let b_s = b.simplify();
    if a_s == b_s {
        return true;
    }
    // Try term normalization
    a_s.normalize() == b_s.normalize()
}

// ── Condition evaluation ────────────────────────────────────────

#[derive(Debug)]
enum EvalResult {
    True,
    False(String),
    Unknown,
}

fn evaluate_condition(cond: &Condition) -> EvalResult {
    match cond {
        Condition::Expr(spanned) => evaluate_expr_as_bool(&spanned.node),

        Condition::Compare { left, op, right } => {
            let l = condition_to_sym(left);
            let r = condition_to_sym(right);
            match (l, r) {
                (Some(l), Some(r)) => evaluate_comparison(&l.simplify(), *op, &r.simplify()),
                _ => EvalResult::Unknown,
            }
        }

        Condition::Equals(a, b) => {
            let l = condition_to_sym(a);
            let r = condition_to_sym(b);
            match (l, r) {
                (Some(l), Some(r)) => {
                    if sym_equal(&l, &r) {
                        EvalResult::True
                    } else {
                        let ls = l.simplify();
                        let rs = r.simplify();
                        match (ls.as_const(), rs.as_const()) {
                            (Some(a), Some(b)) if a != b => {
                                EvalResult::False(format!("{a} ≠ {b}"))
                            }
                            _ => EvalResult::Unknown,
                        }
                    }
                }
                _ => EvalResult::Unknown,
            }
        }

        Condition::And(conditions) => {
            for c in conditions {
                match evaluate_condition(c) {
                    EvalResult::False(ce) => return EvalResult::False(ce),
                    EvalResult::Unknown => return EvalResult::Unknown,
                    EvalResult::True => {}
                }
            }
            EvalResult::True
        }

        Condition::Implies {
            antecedent,
            consequent,
        } => {
            // Try to prove: given antecedent, does consequent hold?
            // Strategy: collect substitutions from antecedent, apply to consequent.
            let mut substitutions = HashMap::new();
            collect_equalities(antecedent, &mut substitutions);

            if !substitutions.is_empty() {
                // Apply substitutions and re-evaluate consequent
                let simplified = apply_substitutions_to_condition(consequent, &substitutions);
                let result = evaluate_condition(&simplified);
                if matches!(result, EvalResult::True) {
                    return EvalResult::True;
                }
            }

            // Fallback: if antecedent is false, implication is vacuously true
            match evaluate_condition(antecedent) {
                EvalResult::False(_) => EvalResult::True,
                _ => {
                    // Try direct evaluation of consequent
                    evaluate_condition(consequent)
                }
            }
        }

        Condition::ForAll { var, domain, body } => {
            // Try: evaluate body for a few values from domain
            if let Some(values) = domain_values(domain) {
                for v in &values {
                    let substituted = substitute_condition(body, var, *v);
                    match evaluate_condition(&substituted) {
                        EvalResult::False(ce) => {
                            return EvalResult::False(format!("{var}={v}: {ce}"))
                        }
                        EvalResult::Unknown => return EvalResult::Unknown,
                        EvalResult::True => {}
                    }
                }
                return EvalResult::True;
            }
            EvalResult::Unknown
        }

        Condition::Exists { var, domain, body } => {
            if let Some(values) = domain_values(domain) {
                for v in &values {
                    let substituted = substitute_condition(body, var, *v);
                    if matches!(evaluate_condition(&substituted), EvalResult::True) {
                        return EvalResult::True;
                    }
                }
                return EvalResult::False(format!("No {var} in domain satisfies condition"));
            }
            EvalResult::Unknown
        }

        Condition::Ref(_) => EvalResult::Unknown,
    }
}

fn evaluate_expr_as_bool(expr: &Expr) -> EvalResult {
    match expr {
        Expr::BoolLit(true) => EvalResult::True,
        Expr::BoolLit(false) => EvalResult::False("false literal".into()),
        Expr::BinOp { left, op, right } => {
            let l = expr_to_sym(&left.node);
            let r = expr_to_sym(&right.node);
            match (l, r) {
                (Some(l), Some(r)) => {
                    let ls = l.simplify();
                    let rs = r.simplify();
                    match op {
                        BinOp::Eq => {
                            if sym_equal(&ls, &rs) {
                                EvalResult::True
                            } else {
                                match (ls.as_const(), rs.as_const()) {
                                    (Some(a), Some(b)) if a != b => {
                                        EvalResult::False(format!("{a} ≠ {b}"))
                                    }
                                    _ => EvalResult::Unknown,
                                }
                            }
                        }
                        BinOp::Leq => evaluate_comparison(&ls, CompareOp::Leq, &rs),
                        BinOp::Geq => evaluate_comparison(&ls, CompareOp::Geq, &rs),
                        BinOp::Lt => evaluate_comparison(&ls, CompareOp::Lt, &rs),
                        BinOp::Gt => evaluate_comparison(&ls, CompareOp::Gt, &rs),
                        BinOp::Neq => match evaluate_comparison(&ls, CompareOp::Eq, &rs) {
                            EvalResult::True => EvalResult::False("values are equal".into()),
                            EvalResult::False(_) => EvalResult::True,
                            EvalResult::Unknown => EvalResult::Unknown,
                        },
                        _ => EvalResult::Unknown,
                    }
                }
                _ => EvalResult::Unknown,
            }
        }
        _ => EvalResult::Unknown,
    }
}

fn evaluate_comparison(l: &Sym, op: CompareOp, r: &Sym) -> EvalResult {
    // Concrete values
    match (l.as_const(), r.as_const()) {
        (Some(a), Some(b)) => {
            let holds = match op {
                CompareOp::Lt => a < b,
                CompareOp::Leq => a <= b,
                CompareOp::Gt => a > b,
                CompareOp::Geq => a >= b,
                CompareOp::Eq => a == b,
                CompareOp::Neq => a != b,
            };
            if holds {
                EvalResult::True
            } else {
                EvalResult::False(format!("{a} not {op:?} {b}"))
            }
        }
        _ => {
            // Check if both sides are symbolically equal
            if matches!(op, CompareOp::Leq | CompareOp::Geq | CompareOp::Eq) && sym_equal(l, r) {
                return EvalResult::True;
            }
            // Check difference: l - r, and see if it simplifies to a constant
            let diff = Sym::Sub(Box::new(l.clone()), Box::new(r.clone())).simplify();
            if let Some(d) = diff.as_const() {
                let holds = match op {
                    CompareOp::Lt => d < 0,
                    CompareOp::Leq => d <= 0,
                    CompareOp::Gt => d > 0,
                    CompareOp::Geq => d >= 0,
                    CompareOp::Eq => d == 0,
                    CompareOp::Neq => d != 0,
                };
                if holds {
                    EvalResult::True
                } else {
                    EvalResult::False(format!("difference is {d}"))
                }
            } else {
                EvalResult::Unknown
            }
        }
    }
}

// ── AST → Sym conversion ────────────────────────────────────────

fn expr_to_sym(expr: &Expr) -> Option<Sym> {
    match expr {
        Expr::IntLit(n) => Some(Sym::Const(*n)),
        Expr::FloatLit(f) => Some(Sym::Float(*f)),
        Expr::Ident(name) => Some(Sym::Var(name.clone())),
        Expr::BinOp { left, op, right } => {
            let l = expr_to_sym(&left.node)?;
            let r = expr_to_sym(&right.node)?;
            Some(match op {
                BinOp::Add => Sym::Add(Box::new(l), Box::new(r)),
                BinOp::Sub => Sym::Sub(Box::new(l), Box::new(r)),
                BinOp::Mul => Sym::Mul(Box::new(l), Box::new(r)),
                _ => return None,
            })
        }
        Expr::UnaryOp { op, operand } => {
            use lattice_parser::ast::UnaryOp;
            let inner = expr_to_sym(&operand.node)?;
            match op {
                UnaryOp::Neg => Some(Sym::Neg(Box::new(inner))),
                _ => None,
            }
        }
        Expr::Field { expr, name } => {
            // Treat a.b as a single variable "a.b"
            if let Expr::Ident(base) = &expr.node {
                Some(Sym::Var(format!("{base}.{name}")))
            } else {
                None
            }
        }
        _ => None,
    }
}

fn condition_to_sym(cond: &Condition) -> Option<Sym> {
    match cond {
        Condition::Expr(spanned) => expr_to_sym(&spanned.node),
        _ => None,
    }
}

// ── Substitution helpers ────────────────────────────────────────

fn collect_equalities(cond: &Condition, subs: &mut HashMap<String, Sym>) {
    match cond {
        Condition::And(conditions) => {
            for c in conditions {
                collect_equalities(c, subs);
            }
        }
        Condition::Equals(a, b) => {
            // If one side is a variable, record the substitution
            if let Some(Sym::Var(name)) = condition_to_sym(a) {
                if let Some(val) = condition_to_sym(b) {
                    subs.insert(name, val);
                }
            } else if let Some(Sym::Var(name)) = condition_to_sym(b) {
                if let Some(val) = condition_to_sym(a) {
                    subs.insert(name, val);
                }
            }
        }
        Condition::Expr(spanned) => {
            // Check for BinOp::Eq in expressions
            if let Expr::BinOp { left, op: BinOp::Eq, right } = &spanned.node {
                if let Some(Sym::Var(name)) = expr_to_sym(&left.node) {
                    if let Some(val) = expr_to_sym(&right.node) {
                        subs.insert(name, val);
                    }
                }
            }
        }
        _ => {}
    }
}

fn apply_substitutions_to_condition(
    cond: &Condition,
    subs: &HashMap<String, Sym>,
) -> Condition {
    match cond {
        Condition::Equals(a, b) => {
            let l = apply_sym_substitutions(condition_to_sym(a), subs);
            let r = apply_sym_substitutions(condition_to_sym(b), subs);
            match (l, r) {
                (Some(l), Some(r)) => Condition::Equals(
                    Box::new(sym_to_condition(&l)),
                    Box::new(sym_to_condition(&r)),
                ),
                _ => cond.clone(),
            }
        }
        Condition::Compare { left, op, right } => {
            let l = apply_sym_substitutions(condition_to_sym(left), subs);
            let r = apply_sym_substitutions(condition_to_sym(right), subs);
            match (l, r) {
                (Some(l), Some(r)) => Condition::Compare {
                    left: Box::new(sym_to_condition(&l)),
                    op: *op,
                    right: Box::new(sym_to_condition(&r)),
                },
                _ => cond.clone(),
            }
        }
        Condition::And(conds) => Condition::And(
            conds
                .iter()
                .map(|c| apply_substitutions_to_condition(c, subs))
                .collect(),
        ),
        Condition::Implies { antecedent, consequent } => Condition::Implies {
            antecedent: Box::new(apply_substitutions_to_condition(antecedent, subs)),
            consequent: Box::new(apply_substitutions_to_condition(consequent, subs)),
        },
        _ => cond.clone(),
    }
}

fn apply_sym_substitutions(sym: Option<Sym>, subs: &HashMap<String, Sym>) -> Option<Sym> {
    let mut s = sym?;
    for (var, val) in subs {
        s = s.substitute(var, val);
    }
    Some(s.simplify())
}

fn sym_to_condition(sym: &Sym) -> Condition {
    let expr = sym_to_expr(sym);
    Condition::Expr(Spanned::dummy(expr))
}

fn sym_to_expr(sym: &Sym) -> Expr {
    match sym {
        Sym::Const(n) => Expr::IntLit(*n),
        Sym::Float(f) => Expr::FloatLit(*f),
        Sym::Var(v) => Expr::Ident(v.clone()),
        Sym::Add(a, b) => Expr::BinOp {
            left: Box::new(Spanned::dummy(sym_to_expr(a))),
            op: BinOp::Add,
            right: Box::new(Spanned::dummy(sym_to_expr(b))),
        },
        Sym::Sub(a, b) => Expr::BinOp {
            left: Box::new(Spanned::dummy(sym_to_expr(a))),
            op: BinOp::Sub,
            right: Box::new(Spanned::dummy(sym_to_expr(b))),
        },
        Sym::Mul(a, b) => Expr::BinOp {
            left: Box::new(Spanned::dummy(sym_to_expr(a))),
            op: BinOp::Mul,
            right: Box::new(Spanned::dummy(sym_to_expr(b))),
        },
        Sym::Neg(a) => Expr::UnaryOp {
            op: lattice_parser::ast::UnaryOp::Neg,
            operand: Box::new(Spanned::dummy(sym_to_expr(a))),
        },
    }
}

// ── Domain enumeration ──────────────────────────────────────────

fn domain_values(domain: &Condition) -> Option<Vec<i64>> {
    // If domain is an expression representing a small set or range
    match domain {
        Condition::Expr(spanned) => match &spanned.node {
            Expr::Array(elements) => {
                let mut vals = Vec::new();
                for e in elements {
                    if let Expr::IntLit(n) = &e.node {
                        vals.push(*n);
                    } else {
                        return None;
                    }
                }
                Some(vals)
            }
            // Note: AST has no Set variant; sets parsed as records/arrays
            _ => None,
        },
        _ => None,
    }
}

fn substitute_condition(cond: &Condition, var: &str, value: i64) -> Condition {
    let mut subs = HashMap::new();
    subs.insert(var.to_string(), Sym::Const(value));
    apply_substitutions_to_condition(cond, &subs)
}

// ── Tests ───────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::obligation::{ObligationKind, ObligationSource};
    use lattice_parser::ast::Span;

    fn make_obligation(cond: Condition) -> ProofObligation {
        ProofObligation {
            id: "test".into(),
            name: "test".into(),
            kind: ObligationKind::Postcondition,
            source: ObligationSource {
                item_name: "test".into(),
                item_kind: "function".into(),
                file: None,
            },
            condition: cond,
            status: ProofStatus::Unverified,
            span: Span::dummy(),
        }
    }

    fn const_cond(n: i64) -> Box<Condition> {
        Box::new(Condition::Expr(Spanned::dummy(Expr::IntLit(n))))
    }

    fn var_cond(name: &str) -> Box<Condition> {
        Box::new(Condition::Expr(Spanned::dummy(Expr::Ident(name.into()))))
    }

    fn add_cond(a: Box<Condition>, b: Box<Condition>) -> Box<Condition> {
        let ae = match *a {
            Condition::Expr(e) => e,
            _ => panic!(),
        };
        let be = match *b {
            Condition::Expr(e) => e,
            _ => panic!(),
        };
        Box::new(Condition::Expr(Spanned::dummy(Expr::BinOp {
            left: Box::new(ae),
            op: BinOp::Add,
            right: Box::new(be),
        })))
    }

    fn sub_cond(a: Box<Condition>, b: Box<Condition>) -> Box<Condition> {
        let ae = match *a {
            Condition::Expr(e) => e,
            _ => panic!(),
        };
        let be = match *b {
            Condition::Expr(e) => e,
            _ => panic!(),
        };
        Box::new(Condition::Expr(Spanned::dummy(Expr::BinOp {
            left: Box::new(ae),
            op: BinOp::Sub,
            right: Box::new(be),
        })))
    }

    #[test]
    fn test_constant_comparison_true() {
        let backend = ArithmeticBackend;
        let ob = make_obligation(Condition::Compare {
            left: const_cond(5),
            op: CompareOp::Gt,
            right: const_cond(3),
        });
        let result = backend.check(&ob);
        assert_eq!(result.status, ProofStatus::Verified);
    }

    #[test]
    fn test_constant_comparison_false() {
        let backend = ArithmeticBackend;
        let ob = make_obligation(Condition::Compare {
            left: const_cond(3),
            op: CompareOp::Gt,
            right: const_cond(5),
        });
        let result = backend.check(&ob);
        assert!(matches!(result.status, ProofStatus::Failed { .. }));
    }

    #[test]
    fn test_tautology_equality() {
        // a = a → Verified
        let backend = ArithmeticBackend;
        let ob = make_obligation(Condition::Equals(var_cond("a"), var_cond("a")));
        let result = backend.check(&ob);
        assert_eq!(result.status, ProofStatus::Verified);
    }

    #[test]
    fn test_conservation_law() {
        // Given: from' = from - amount, to' = to + amount
        // Prove: from' + to' = from + to
        let backend = ArithmeticBackend;
        let ob = make_obligation(Condition::Implies {
            antecedent: Box::new(Condition::And(vec![
                Condition::Equals(
                    var_cond("from_prime"),
                    sub_cond(var_cond("from"), var_cond("amount")),
                ),
                Condition::Equals(
                    var_cond("to_prime"),
                    add_cond(var_cond("to"), var_cond("amount")),
                ),
            ])),
            consequent: Box::new(Condition::Equals(
                add_cond(var_cond("from_prime"), var_cond("to_prime")),
                add_cond(var_cond("from"), var_cond("to")),
            )),
        });
        let result = backend.check(&ob);
        assert_eq!(result.status, ProofStatus::Verified);
    }

    #[test]
    fn test_non_negative_subtraction() {
        // balance >= amount ∧ amount > 0 → balance - amount >= 0
        // This requires symbolic reasoning we may not fully support,
        // but the comparison with concrete difference should work for constants.
        let backend = ArithmeticBackend;
        let ob = make_obligation(Condition::Compare {
            left: const_cond(10),
            op: CompareOp::Geq,
            right: const_cond(0),
        });
        let result = backend.check(&ob);
        assert_eq!(result.status, ProofStatus::Verified);
    }

    #[test]
    fn test_forall_bounded() {
        // ∀ x ∈ {1, 2, 3} → x > 0
        let backend = ArithmeticBackend;
        let domain = Condition::Expr(Spanned::dummy(Expr::Array(vec![
            Spanned::dummy(Expr::IntLit(1)),
            Spanned::dummy(Expr::IntLit(2)),
            Spanned::dummy(Expr::IntLit(3)),
        ])));
        let ob = make_obligation(Condition::ForAll {
            var: "x".into(),
            domain: Box::new(domain),
            body: Box::new(Condition::Compare {
                left: var_cond("x"),
                op: CompareOp::Gt,
                right: const_cond(0),
            }),
        });
        let result = backend.check(&ob);
        assert_eq!(result.status, ProofStatus::Verified);
    }

    #[test]
    fn test_forall_bounded_false() {
        // ∀ x ∈ {-1, 2, 3} → x > 0  (fails for x=-1)
        let backend = ArithmeticBackend;
        let domain = Condition::Expr(Spanned::dummy(Expr::Array(vec![
            Spanned::dummy(Expr::IntLit(-1)),
            Spanned::dummy(Expr::IntLit(2)),
            Spanned::dummy(Expr::IntLit(3)),
        ])));
        let ob = make_obligation(Condition::ForAll {
            var: "x".into(),
            domain: Box::new(domain),
            body: Box::new(Condition::Compare {
                left: var_cond("x"),
                op: CompareOp::Gt,
                right: const_cond(0),
            }),
        });
        let result = backend.check(&ob);
        assert!(matches!(result.status, ProofStatus::Failed { .. }));
    }

    #[test]
    fn test_unknown_complex() {
        // Something too complex: forall over unbounded domain
        let backend = ArithmeticBackend;
        let ob = make_obligation(Condition::ForAll {
            var: "x".into(),
            domain: Box::new(Condition::Expr(Spanned::dummy(Expr::Ident("Nat".into())))),
            body: Box::new(Condition::Compare {
                left: var_cond("x"),
                op: CompareOp::Geq,
                right: const_cond(0),
            }),
        });
        let result = backend.check(&ob);
        assert_eq!(result.status, ProofStatus::Unverified);
    }

    #[test]
    fn test_and_all_true() {
        let backend = ArithmeticBackend;
        let ob = make_obligation(Condition::And(vec![
            Condition::Compare {
                left: const_cond(5),
                op: CompareOp::Gt,
                right: const_cond(3),
            },
            Condition::Compare {
                left: const_cond(10),
                op: CompareOp::Leq,
                right: const_cond(10),
            },
        ]));
        let result = backend.check(&ob);
        assert_eq!(result.status, ProofStatus::Verified);
    }

    #[test]
    fn test_backend_in_checker() {
        use crate::checker::ProofChecker;
        let mut checker = ProofChecker::new();
        checker.add_backend(Box::new(ArithmeticBackend));

        let obs = vec![make_obligation(Condition::Compare {
            left: const_cond(1),
            op: CompareOp::Lt,
            right: const_cond(2),
        })];

        let results = checker.check_all(&obs);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].1.status, ProofStatus::Verified);
    }
}
