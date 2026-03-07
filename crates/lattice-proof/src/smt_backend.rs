//! SMT-LIB2 proof backend.
//!
//! Translates proof obligations to SMT-LIB2 format and invokes Z3
//! (or any compatible SMT solver) as a subprocess. Gracefully returns
//! `Unverified` if Z3 is not installed.

use std::io::Write;
use std::process::{Command, Stdio};
use std::time::Instant;

use crate::checker::{ProofBackend, ProofResult};
use crate::obligation::{CompareOp, Condition, ProofObligation};
use crate::status::ProofStatus;
use lattice_parser::ast::{BinOp, Expr};

/// SMT backend that shells out to Z3 (or a compatible solver).
pub struct SmtBackend {
    /// Path to the solver binary (default: "z3").
    solver_path: String,
    /// Timeout in milliseconds for solver invocations.
    timeout_ms: u64,
}

impl SmtBackend {
    pub fn new() -> Self {
        Self {
            solver_path: "z3".into(),
            timeout_ms: 5000,
        }
    }

    pub fn with_solver(mut self, path: &str) -> Self {
        self.solver_path = path.into();
        self
    }

    pub fn with_timeout(mut self, ms: u64) -> Self {
        self.timeout_ms = ms;
        self
    }

    /// Check if the solver binary is available.
    fn solver_available(&self) -> bool {
        Command::new(&self.solver_path)
            .arg("--version")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok()
    }

    /// Run an SMT-LIB2 script through the solver.
    fn run_solver(&self, script: &str) -> Result<String, String> {
        let mut child = Command::new(&self.solver_path)
            .args(["-in", "-smt2", &format!("-T:{}", self.timeout_ms / 1000)])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("failed to start solver: {}", e))?;

        if let Some(mut stdin) = child.stdin.take() {
            stdin.write_all(script.as_bytes())
                .map_err(|e| format!("failed to write to solver: {}", e))?;
        }

        let output = child
            .wait_with_output()
            .map_err(|e| format!("solver error: {}", e))?;

        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    }
}

impl Default for SmtBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl ProofBackend for SmtBackend {
    fn name(&self) -> &str {
        "smt"
    }

    fn supports(&self, _obligation: &ProofObligation) -> bool {
        // We handle any obligation, but may return Unverified if Z3 isn't available
        true
    }

    fn check(&self, obligation: &ProofObligation) -> ProofResult {
        let start = Instant::now();

        // Generate SMT-LIB2 script
        let mut gen = SmtGenerator::new();
        match gen.generate_check(&obligation.condition) {
            Ok(script) => {
                // Try to run the solver
                match self.run_solver(&script) {
                    Ok(output) => {
                        let duration = start.elapsed().as_millis() as u64;
                        parse_solver_output(&output, duration)
                    }
                    Err(msg) => ProofResult {
                        status: ProofStatus::Unverified,
                        duration_ms: start.elapsed().as_millis() as u64,
                        message: Some(format!("Solver unavailable: {}", msg)),
                        counterexample: None,
                    },
                }
            }
            Err(msg) => ProofResult {
                status: ProofStatus::Unverified,
                duration_ms: start.elapsed().as_millis() as u64,
                message: Some(format!("Cannot translate to SMT: {}", msg)),
                counterexample: None,
            },
        }
    }
}

fn parse_solver_output(output: &str, duration_ms: u64) -> ProofResult {
    let trimmed = output.trim();

    if trimmed.starts_with("unsat") {
        // The negation is unsatisfiable → original is valid
        ProofResult {
            status: ProofStatus::Verified,
            duration_ms,
            message: Some("Proved by SMT solver".into()),
            counterexample: None,
        }
    } else if trimmed.starts_with("sat") {
        // The negation is satisfiable → original has a counterexample
        // Extract the model if present
        let model = if let Some(model_start) = output.find("(model") {
            Some(output[model_start..].to_string())
        } else {
            None
        };
        ProofResult {
            status: ProofStatus::Failed {
                reason: "Counterexample found by SMT solver".into(),
            },
            duration_ms,
            message: Some("Disproved by SMT solver".into()),
            counterexample: model,
        }
    } else if trimmed.starts_with("unknown") || trimmed.starts_with("timeout") {
        ProofResult {
            status: ProofStatus::Unverified,
            duration_ms,
            message: Some("SMT solver returned unknown/timeout".into()),
            counterexample: None,
        }
    } else {
        ProofResult {
            status: ProofStatus::Unverified,
            duration_ms,
            message: Some(format!("Unexpected solver output: {}", trimmed)),
            counterexample: None,
        }
    }
}

// ── SMT-LIB2 Generator ──────────────────────────────────────────

struct SmtGenerator {
    declarations: Vec<String>,
    assertions: Vec<String>,
    var_counter: usize,
}

impl SmtGenerator {
    fn new() -> Self {
        Self {
            declarations: Vec::new(),
            assertions: Vec::new(),
            var_counter: 0,
        }
    }

    /// Generate a complete SMT-LIB2 script to check validity of a condition.
    /// We assert the negation and check for unsatisfiability.
    fn generate_check(&mut self, condition: &Condition) -> Result<String, String> {
        let smt_expr = self.condition_to_smt(condition)?;

        let mut script = String::new();
        script.push_str("(set-logic ALL)\n");

        for decl in &self.declarations {
            script.push_str(decl);
            script.push('\n');
        }

        // Assert the negation of the condition
        script.push_str(&format!("(assert (not {}))\n", smt_expr));
        script.push_str("(check-sat)\n");
        script.push_str("(exit)\n");

        Ok(script)
    }

    fn declare_var(&mut self, name: &str) {
        let decl = format!("(declare-const {} Int)", sanitize_name(name));
        if !self.declarations.contains(&decl) {
            self.declarations.push(decl);
        }
    }

    fn fresh_var(&mut self) -> String {
        let name = format!("__smt_v{}", self.var_counter);
        self.var_counter += 1;
        name
    }

    fn condition_to_smt(&mut self, cond: &Condition) -> Result<String, String> {
        match cond {
            Condition::Expr(spanned) => self.expr_to_smt(&spanned.node),

            Condition::Compare { left, op, right } => {
                let l = self.condition_to_smt(left)?;
                let r = self.condition_to_smt(right)?;
                let op_str = match op {
                    CompareOp::Lt => "<",
                    CompareOp::Leq => "<=",
                    CompareOp::Gt => ">",
                    CompareOp::Geq => ">=",
                    CompareOp::Eq => "=",
                    CompareOp::Neq => return Ok(format!("(not (= {} {}))", l, r)),
                };
                Ok(format!("({} {} {})", op_str, l, r))
            }

            Condition::Equals(a, b) => {
                let l = self.condition_to_smt(a)?;
                let r = self.condition_to_smt(b)?;
                Ok(format!("(= {} {})", l, r))
            }

            Condition::And(conditions) => {
                if conditions.is_empty() {
                    return Ok("true".into());
                }
                let parts: Result<Vec<String>, String> = conditions
                    .iter()
                    .map(|c| self.condition_to_smt(c))
                    .collect();
                let parts = parts?;
                if parts.len() == 1 {
                    Ok(parts.into_iter().next().unwrap())
                } else {
                    Ok(format!("(and {})", parts.join(" ")))
                }
            }

            Condition::Implies { antecedent, consequent } => {
                let ante = self.condition_to_smt(antecedent)?;
                let cons = self.condition_to_smt(consequent)?;
                Ok(format!("(=> {} {})", ante, cons))
            }

            Condition::ForAll { var, domain: _, body } => {
                self.declare_var(var);
                let body_smt = self.condition_to_smt(body)?;
                let svar = sanitize_name(var);
                // Remove the declare-const since we'll use forall binding
                self.declarations.retain(|d| !d.contains(&format!("(declare-const {} ", svar)));
                Ok(format!("(forall (({} Int)) {})", svar, body_smt))
            }

            Condition::Exists { var, domain: _, body } => {
                self.declare_var(var);
                let body_smt = self.condition_to_smt(body)?;
                let svar = sanitize_name(var);
                self.declarations.retain(|d| !d.contains(&format!("(declare-const {} ", svar)));
                Ok(format!("(exists (({} Int)) {})", svar, body_smt))
            }

            Condition::Ref(name) => {
                Err(format!("cannot translate Ref({}) to SMT", name))
            }
        }
    }

    fn expr_to_smt(&mut self, expr: &Expr) -> Result<String, String> {
        match expr {
            Expr::IntLit(n) => {
                if *n < 0 {
                    Ok(format!("(- {})", -n))
                } else {
                    Ok(n.to_string())
                }
            }
            Expr::FloatLit(f) => Ok(f.to_string()),
            Expr::BoolLit(true) => Ok("true".into()),
            Expr::BoolLit(false) => Ok("false".into()),
            Expr::Ident(name) => {
                self.declare_var(name);
                Ok(sanitize_name(name))
            }
            Expr::BinOp { left, op, right } => {
                let l = self.expr_to_smt(&left.node)?;
                let r = self.expr_to_smt(&right.node)?;
                match op {
                    BinOp::Add => Ok(format!("(+ {} {})", l, r)),
                    BinOp::Sub => Ok(format!("(- {} {})", l, r)),
                    BinOp::Mul => Ok(format!("(* {} {})", l, r)),
                    BinOp::Div => Ok(format!("(div {} {})", l, r)),
                    BinOp::Mod => Ok(format!("(mod {} {})", l, r)),
                    BinOp::Eq => Ok(format!("(= {} {})", l, r)),
                    BinOp::Neq => Ok(format!("(not (= {} {}))", l, r)),
                    BinOp::Lt => Ok(format!("(< {} {})", l, r)),
                    BinOp::Gt => Ok(format!("(> {} {})", l, r)),
                    BinOp::Leq => Ok(format!("(<= {} {})", l, r)),
                    BinOp::Geq => Ok(format!("(>= {} {})", l, r)),
                    BinOp::And => Ok(format!("(and {} {})", l, r)),
                    BinOp::Or => Ok(format!("(or {} {})", l, r)),
                    BinOp::Implies => Ok(format!("(=> {} {})", l, r)),
                    _ => Err(format!("unsupported binary operator {:?}", op)),
                }
            }
            Expr::UnaryOp { op, operand } => {
                let inner = self.expr_to_smt(&operand.node)?;
                match op {
                    lattice_parser::ast::UnaryOp::Neg => Ok(format!("(- {})", inner)),
                    lattice_parser::ast::UnaryOp::Not => Ok(format!("(not {})", inner)),
                }
            }
            Expr::Field { expr, name } => {
                // Treat a.b as a single variable "a.b"
                if let Expr::Ident(base) = &expr.node {
                    let full_name = format!("{}.{}", base, name);
                    self.declare_var(&full_name);
                    Ok(sanitize_name(&full_name))
                } else {
                    Err("complex field access not supported in SMT".into())
                }
            }
            Expr::If { cond, then_, else_ } => {
                let c = self.expr_to_smt(&cond.node)?;
                let t = self.expr_to_smt(&then_.node)?;
                if let Some(e) = else_ {
                    let e = self.expr_to_smt(&e.node)?;
                    Ok(format!("(ite {} {} {})", c, t, e))
                } else {
                    Err("if without else not supported in SMT".into())
                }
            }
            Expr::ForAll { var, domain: _, body } => {
                let body_smt = self.expr_to_smt(&body.node)?;
                let svar = sanitize_name(var);
                Ok(format!("(forall (({} Int)) {})", svar, body_smt))
            }
            Expr::Exists { var, domain: _, body } => {
                let body_smt = self.expr_to_smt(&body.node)?;
                let svar = sanitize_name(var);
                Ok(format!("(exists (({} Int)) {})", svar, body_smt))
            }
            _ => Err(format!("unsupported expression in SMT translation: {:?}", std::mem::discriminant(expr))),
        }
    }
}

/// Sanitize a Lattice identifier for use in SMT-LIB2.
/// Replaces dots with underscores and ensures valid SMT symbol syntax.
fn sanitize_name(name: &str) -> String {
    let sanitized = name.replace('.', "_dot_");
    if sanitized.chars().next().map_or(true, |c| c.is_numeric()) {
        format!("v_{}", sanitized)
    } else {
        sanitized
    }
}

// ── Public utilities ─────────────────────────────────────────────

/// Generate an SMT-LIB2 script for a proof obligation (for debugging/display).
pub fn obligation_to_smt(obligation: &ProofObligation) -> Result<String, String> {
    let mut gen = SmtGenerator::new();
    gen.generate_check(&obligation.condition)
}

// ── Tests ────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::obligation::{ObligationKind, ObligationSource};
    use lattice_parser::ast::{Span, Spanned};

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
        let ae = match *a { Condition::Expr(e) => e, _ => panic!() };
        let be = match *b { Condition::Expr(e) => e, _ => panic!() };
        Box::new(Condition::Expr(Spanned::dummy(Expr::BinOp {
            left: Box::new(ae),
            op: BinOp::Add,
            right: Box::new(be),
        })))
    }

    fn sub_cond(a: Box<Condition>, b: Box<Condition>) -> Box<Condition> {
        let ae = match *a { Condition::Expr(e) => e, _ => panic!() };
        let be = match *b { Condition::Expr(e) => e, _ => panic!() };
        Box::new(Condition::Expr(Spanned::dummy(Expr::BinOp {
            left: Box::new(ae),
            op: BinOp::Sub,
            right: Box::new(be),
        })))
    }

    #[test]
    fn smt_generate_simple_comparison() {
        let ob = make_obligation(Condition::Compare {
            left: const_cond(5),
            op: CompareOp::Gt,
            right: const_cond(3),
        });
        let script = obligation_to_smt(&ob).unwrap();
        assert!(script.contains("(assert (not (> 5 3)))"));
        assert!(script.contains("(check-sat)"));
    }

    #[test]
    fn smt_generate_variable_comparison() {
        let ob = make_obligation(Condition::Compare {
            left: var_cond("x"),
            op: CompareOp::Geq,
            right: const_cond(0),
        });
        let script = obligation_to_smt(&ob).unwrap();
        assert!(script.contains("(declare-const x Int)"));
        assert!(script.contains("(assert (not (>= x 0)))"));
    }

    #[test]
    fn smt_generate_equality() {
        let ob = make_obligation(Condition::Equals(var_cond("a"), var_cond("a")));
        let script = obligation_to_smt(&ob).unwrap();
        assert!(script.contains("(assert (not (= a a)))"));
    }

    #[test]
    fn smt_generate_conjunction() {
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
        let script = obligation_to_smt(&ob).unwrap();
        assert!(script.contains("(and (> 5 3) (<= 10 10))"));
    }

    #[test]
    fn smt_generate_implication() {
        let ob = make_obligation(Condition::Implies {
            antecedent: Box::new(Condition::Compare {
                left: var_cond("x"),
                op: CompareOp::Gt,
                right: const_cond(0),
            }),
            consequent: Box::new(Condition::Compare {
                left: var_cond("x"),
                op: CompareOp::Geq,
                right: const_cond(0),
            }),
        });
        let script = obligation_to_smt(&ob).unwrap();
        assert!(script.contains("(=> (> x 0) (>= x 0))"));
    }

    #[test]
    fn smt_generate_conservation_law() {
        // from' = from - amount ∧ to' = to + amount → from' + to' = from + to
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
        let script = obligation_to_smt(&ob).unwrap();
        assert!(script.contains("(set-logic ALL)"));
        assert!(script.contains("(declare-const from_prime Int)"));
        assert!(script.contains("(declare-const from Int)"));
        assert!(script.contains("(assert (not (=>"));
        assert!(script.contains("(check-sat)"));
    }

    #[test]
    fn smt_generate_forall() {
        let ob = make_obligation(Condition::ForAll {
            var: "x".into(),
            domain: Box::new(Condition::Expr(Spanned::dummy(Expr::Ident("Nat".into())))),
            body: Box::new(Condition::Compare {
                left: var_cond("x"),
                op: CompareOp::Geq,
                right: const_cond(0),
            }),
        });
        let script = obligation_to_smt(&ob).unwrap();
        assert!(script.contains("(forall ((x Int))"));
    }

    #[test]
    fn smt_generate_field_access() {
        let ob = make_obligation(Condition::Expr(Spanned::dummy(Expr::BinOp {
            left: Box::new(Spanned::dummy(Expr::Field {
                expr: Box::new(Spanned::dummy(Expr::Ident("account".into()))),
                name: "balance".into(),
            })),
            op: BinOp::Geq,
            right: Box::new(Spanned::dummy(Expr::IntLit(0))),
        })));
        let script = obligation_to_smt(&ob).unwrap();
        assert!(script.contains("account_dot_balance"));
    }

    #[test]
    fn smt_generate_negated_constant() {
        let ob = make_obligation(Condition::Compare {
            left: const_cond(-5),
            op: CompareOp::Lt,
            right: const_cond(0),
        });
        let script = obligation_to_smt(&ob).unwrap();
        assert!(script.contains("(- 5)"));
    }

    #[test]
    fn smt_parse_output_unsat() {
        let result = parse_solver_output("unsat\n", 10);
        assert_eq!(result.status, ProofStatus::Verified);
    }

    #[test]
    fn smt_parse_output_sat() {
        let result = parse_solver_output("sat\n", 10);
        assert!(matches!(result.status, ProofStatus::Failed { .. }));
    }

    #[test]
    fn smt_parse_output_unknown() {
        let result = parse_solver_output("unknown\n", 10);
        assert_eq!(result.status, ProofStatus::Unverified);
    }

    #[test]
    fn smt_backend_graceful_without_z3() {
        // If Z3 isn't installed, we should get Unverified, not a crash
        let backend = SmtBackend::new().with_solver("nonexistent_solver_binary");
        let ob = make_obligation(Condition::Compare {
            left: const_cond(5),
            op: CompareOp::Gt,
            right: const_cond(3),
        });
        let result = backend.check(&ob);
        assert_eq!(result.status, ProofStatus::Unverified);
        assert!(result.message.as_ref().unwrap().contains("unavailable"));
    }

    #[test]
    fn smt_generate_boolean_ops() {
        let ob = make_obligation(Condition::Expr(Spanned::dummy(Expr::BinOp {
            left: Box::new(Spanned::dummy(Expr::BinOp {
                left: Box::new(Spanned::dummy(Expr::Ident("a".into()))),
                op: BinOp::Gt,
                right: Box::new(Spanned::dummy(Expr::IntLit(0))),
            })),
            op: BinOp::And,
            right: Box::new(Spanned::dummy(Expr::BinOp {
                left: Box::new(Spanned::dummy(Expr::Ident("b".into()))),
                op: BinOp::Lt,
                right: Box::new(Spanned::dummy(Expr::IntLit(10))),
            })),
        })));
        let script = obligation_to_smt(&ob).unwrap();
        assert!(script.contains("(and (> a 0) (< b 10))"));
    }

    #[test]
    fn smt_generate_unary_ops() {
        let ob = make_obligation(Condition::Expr(Spanned::dummy(Expr::UnaryOp {
            op: lattice_parser::ast::UnaryOp::Not,
            operand: Box::new(Spanned::dummy(Expr::BoolLit(false))),
        })));
        let script = obligation_to_smt(&ob).unwrap();
        assert!(script.contains("(not false)"));
    }
}
