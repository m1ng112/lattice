//! Proof checking backends and dispatcher.
//!
//! Defines the [`ProofBackend`] trait that solver integrations
//! (Z3, Lean 4, etc.) implement, and the [`ProofChecker`] that
//! dispatches obligations to the first supporting backend.

use crate::obligation::ProofObligation;
use crate::status::ProofStatus;

/// Result of checking a single proof obligation.
#[derive(Debug, Clone)]
pub struct ProofResult {
    pub status: ProofStatus,
    pub duration_ms: u64,
    pub message: Option<String>,
    pub counterexample: Option<String>,
}

/// Trait for proof checking backends (Z3, Lean 4, manual, etc.).
pub trait ProofBackend {
    /// Human-readable name of this backend.
    fn name(&self) -> &str;

    /// Check a single obligation.
    fn check(&self, obligation: &ProofObligation) -> ProofResult;

    /// Whether this backend can handle the given obligation.
    fn supports(&self, obligation: &ProofObligation) -> bool;
}

/// The main proof checker that dispatches obligations to registered backends.
///
/// Backends are tried in registration order; the first backend that
/// [`supports`](ProofBackend::supports) an obligation handles it.
pub struct ProofChecker {
    backends: Vec<Box<dyn ProofBackend>>,
}

impl ProofChecker {
    pub fn new() -> Self {
        Self {
            backends: Vec::new(),
        }
    }

    /// Register a backend. Earlier backends have higher priority.
    pub fn add_backend(&mut self, backend: Box<dyn ProofBackend>) {
        self.backends.push(backend);
    }

    /// Check all obligations, returning each paired with its result.
    ///
    /// Obligations that no backend supports are marked [`ProofStatus::Skipped`].
    pub fn check_all(&self, obligations: &[ProofObligation]) -> Vec<(ProofObligation, ProofResult)> {
        obligations
            .iter()
            .map(|ob| {
                let result = self
                    .backends
                    .iter()
                    .find(|b| b.supports(ob))
                    .map(|b| b.check(ob))
                    .unwrap_or(ProofResult {
                        status: ProofStatus::Skipped,
                        duration_ms: 0,
                        message: Some("No supporting backend".into()),
                        counterexample: None,
                    });
                (ob.clone(), result)
            })
            .collect()
    }
}

impl Default for ProofChecker {
    fn default() -> Self {
        Self::new()
    }
}

/// A trivial backend that marks everything as [`ProofStatus::Unverified`].
///
/// Useful as a placeholder when no real solver is configured.
pub struct TrivialBackend;

impl ProofBackend for TrivialBackend {
    fn name(&self) -> &str {
        "trivial"
    }

    fn check(&self, _obligation: &ProofObligation) -> ProofResult {
        ProofResult {
            status: ProofStatus::Unverified,
            duration_ms: 0,
            message: Some("No solver configured".into()),
            counterexample: None,
        }
    }

    fn supports(&self, _obligation: &ProofObligation) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::obligation::{Condition, ObligationKind, ObligationSource};
    use lattice_parser::ast::{Expr, Span, Spanned};

    fn dummy_obligation(name: &str) -> ProofObligation {
        ProofObligation {
            id: format!("test_{name}"),
            name: name.to_string(),
            kind: ObligationKind::Precondition,
            source: ObligationSource {
                item_name: "test_fn".to_string(),
                item_kind: "function".to_string(),
                file: None,
            },
            condition: Condition::Expr(Spanned::dummy(Expr::BoolLit(true))),
            status: ProofStatus::Unverified,
            span: Span::dummy(),
        }
    }

    #[test]
    fn trivial_backend_marks_unverified() {
        let checker = {
            let mut c = ProofChecker::new();
            c.add_backend(Box::new(TrivialBackend));
            c
        };

        let obs = vec![dummy_obligation("a"), dummy_obligation("b")];
        let results = checker.check_all(&obs);

        assert_eq!(results.len(), 2);
        for (_, result) in &results {
            assert_eq!(result.status, ProofStatus::Unverified);
            assert_eq!(result.message.as_deref(), Some("No solver configured"));
        }
    }

    #[test]
    fn no_backends_skips_all() {
        let checker = ProofChecker::new();
        let obs = vec![dummy_obligation("x")];
        let results = checker.check_all(&obs);

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].1.status, ProofStatus::Skipped);
    }

    #[test]
    fn backend_priority_order() {
        struct AlwaysVerified;
        impl ProofBackend for AlwaysVerified {
            fn name(&self) -> &str {
                "always_verified"
            }
            fn check(&self, _: &ProofObligation) -> ProofResult {
                ProofResult {
                    status: ProofStatus::Verified,
                    duration_ms: 1,
                    message: None,
                    counterexample: None,
                }
            }
            fn supports(&self, _: &ProofObligation) -> bool {
                true
            }
        }

        // AlwaysVerified first, TrivialBackend second
        let mut checker = ProofChecker::new();
        checker.add_backend(Box::new(AlwaysVerified));
        checker.add_backend(Box::new(TrivialBackend));

        let obs = vec![dummy_obligation("z")];
        let results = checker.check_all(&obs);

        // Should use the first matching backend
        assert_eq!(results[0].1.status, ProofStatus::Verified);
    }

    #[test]
    fn selective_backend() {
        /// A backend that only handles postconditions.
        struct PostOnly;
        impl ProofBackend for PostOnly {
            fn name(&self) -> &str {
                "post_only"
            }
            fn check(&self, _: &ProofObligation) -> ProofResult {
                ProofResult {
                    status: ProofStatus::Verified,
                    duration_ms: 5,
                    message: Some("postcondition OK".into()),
                    counterexample: None,
                }
            }
            fn supports(&self, ob: &ProofObligation) -> bool {
                ob.kind == ObligationKind::Postcondition
            }
        }

        let mut checker = ProofChecker::new();
        checker.add_backend(Box::new(PostOnly));
        // No fallback — preconditions should be Skipped

        let mut pre = dummy_obligation("pre_ob");
        pre.kind = ObligationKind::Precondition;

        let mut post = dummy_obligation("post_ob");
        post.kind = ObligationKind::Postcondition;

        let results = checker.check_all(&[pre, post]);
        assert_eq!(results[0].1.status, ProofStatus::Skipped);
        assert_eq!(results[1].1.status, ProofStatus::Verified);
    }
}
