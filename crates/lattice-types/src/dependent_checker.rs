//! Dependent type checker for Lattice.
//!
//! Implements bidirectional type checking for the Calculus of Constructions
//! with Σ-types, propositional equality, and universe polymorphism.

use crate::dependent::{Term, Universe};
use crate::normalize::{self, NormContext};

/// Errors produced during dependent type checking.
#[derive(Debug, Clone, thiserror::Error)]
pub enum DependentTypeError {
    #[error("type mismatch: expected {expected}, found {found}")]
    Mismatch { expected: String, found: String },

    #[error("unbound variable: {name}")]
    UnboundVar { name: String },

    #[error("expected function type, got {found}")]
    NotFunction { found: String },

    #[error("expected Sigma type, got {found}")]
    NotSigma { found: String },

    #[error("universe inconsistency: {msg}")]
    UniverseError { msg: String },

    #[error("expected universe (Type), got {found}")]
    NotUniverse { found: String },
}

/// The dependent type checker.
///
/// Uses a typing context (env) for variable types and a normalization
/// context (ctx) for definitions and type-level computation.
pub struct DependentChecker {
    ctx: NormContext,
    /// Typing context: stack of (name, type) pairs.
    env: Vec<(String, Term)>,
    errors: Vec<DependentTypeError>,
}

impl Default for DependentChecker {
    fn default() -> Self {
        Self::new()
    }
}

impl DependentChecker {
    pub fn new() -> Self {
        Self {
            ctx: NormContext::new(),
            env: Vec::new(),
            errors: Vec::new(),
        }
    }

    /// Returns all accumulated errors.
    pub fn errors(&self) -> &[DependentTypeError] {
        &self.errors
    }

    /// Register a definition in the normalization context.
    pub fn define(&mut self, name: String, value: Term, ty: Term) {
        self.ctx.define(name.clone(), value);
        self.env.push((name, ty));
    }

    /// Infer the type of a term.
    pub fn infer(&mut self, term: &Term) -> Result<Term, DependentTypeError> {
        match term {
            Term::Var(name) => self.lookup(name),

            // Type_n : Type_{n+1}
            Term::Universe(u) => Ok(Term::Universe(u.succ())),

            // Pi formation: if A : Type_i and B : Type_j (with x:A), then Π(x:A).B : Type_max(i,j)
            Term::Pi {
                param,
                param_type,
                body,
            } => {
                let s1 = self.infer_universe(param_type)?;
                self.env.push((param.clone(), *param_type.clone()));
                let s2 = self.infer_universe(body)?;
                self.env.pop();
                Ok(Term::Universe(Universe::max(&s1, &s2)))
            }

            // Lambda: infer Π-type
            Term::Lambda {
                param,
                param_type,
                body,
            } => {
                // Ensure param_type is well-typed (lives in some universe)
                self.infer_universe(param_type)?;
                self.env.push((param.clone(), *param_type.clone()));
                let body_type = self.infer(body)?;
                self.env.pop();
                Ok(Term::Pi {
                    param: param.clone(),
                    param_type: param_type.clone(),
                    body: Box::new(body_type),
                })
            }

            // Application: if f : Π(x:A).B and a : A, then f a : B[x := a]
            Term::App { func, arg } => {
                let func_type = self.infer(func)?;
                let func_type = self.ctx.whnf(&func_type);
                match func_type {
                    Term::Pi {
                        param,
                        param_type,
                        body,
                    } => {
                        self.check(arg, &param_type)?;
                        Ok(normalize::substitute(&body, &param, arg))
                    }
                    _ => {
                        let err = DependentTypeError::NotFunction {
                            found: format!("{func_type}"),
                        };
                        self.errors.push(err.clone());
                        Err(err)
                    }
                }
            }

            // Sigma formation: if A : Type_i and B : Type_j (with x:A), then Σ(x:A).B : Type_max(i,j)
            Term::Sigma {
                param,
                param_type,
                body,
            } => {
                let s1 = self.infer_universe(param_type)?;
                self.env.push((param.clone(), *param_type.clone()));
                let s2 = self.infer_universe(body)?;
                self.env.pop();
                Ok(Term::Universe(Universe::max(&s1, &s2)))
            }

            // Pair: check against the annotated Sigma type
            Term::Pair {
                first,
                second,
                sigma_type,
            } => {
                let sigma_whnf = self.ctx.whnf(sigma_type);
                match &sigma_whnf {
                    Term::Sigma {
                        param,
                        param_type,
                        body,
                    } => {
                        self.check(first, param_type)?;
                        let second_type = normalize::substitute(body, param, first);
                        self.check(second, &second_type)?;
                        Ok(sigma_whnf)
                    }
                    _ => {
                        let err = DependentTypeError::NotSigma {
                            found: format!("{sigma_whnf}"),
                        };
                        self.errors.push(err.clone());
                        Err(err)
                    }
                }
            }

            // First projection: if p : Σ(x:A).B, then fst p : A
            Term::Fst(pair) => {
                let pair_type = self.infer(pair)?;
                let pair_type = self.ctx.whnf(&pair_type);
                match pair_type {
                    Term::Sigma { param_type, .. } => Ok(*param_type),
                    _ => {
                        let err = DependentTypeError::NotSigma {
                            found: format!("{pair_type}"),
                        };
                        self.errors.push(err.clone());
                        Err(err)
                    }
                }
            }

            // Second projection: if p : Σ(x:A).B, then snd p : B[x := fst p]
            Term::Snd(pair) => {
                let pair_type = self.infer(pair)?;
                let pair_type = self.ctx.whnf(&pair_type);
                match pair_type {
                    Term::Sigma { param, body, .. } => {
                        let fst = Term::Fst(pair.clone());
                        Ok(normalize::substitute(&body, &param, &fst))
                    }
                    _ => {
                        let err = DependentTypeError::NotSigma {
                            found: format!("{pair_type}"),
                        };
                        self.errors.push(err.clone());
                        Err(err)
                    }
                }
            }

            // Equality formation: if a : A and b : A, then Eq(A, a, b) : Type₀
            Term::Eq { ty, left, right } => {
                self.infer_universe(ty)?;
                self.check(left, ty)?;
                self.check(right, ty)?;
                Ok(Term::Universe(Universe::base()))
            }

            // Reflexivity: if a : A, then refl(a) : Eq(A, a, a)
            Term::Refl(term) => {
                let ty = self.infer(term)?;
                Ok(Term::Eq {
                    ty: Box::new(ty),
                    left: term.clone(),
                    right: term.clone(),
                })
            }

            // Integer literal: Int (treated as a base type name)
            Term::IntLit(_) => Ok(Term::Var("Int".into())),

            // Annotation: (e : A) — check e against A, return A
            Term::Ann { term, ty } => {
                self.infer_universe(ty)?;
                self.check(term, ty)?;
                Ok(*ty.clone())
            }

            // Let: let x: A = v in body — check v : A, infer body with x : A
            Term::Let {
                name,
                ty,
                value,
                body,
            } => {
                self.infer_universe(ty)?;
                self.check(value, ty)?;
                self.ctx.define(name.clone(), *value.clone());
                self.env.push((name.clone(), *ty.clone()));
                let result = self.infer(body)?;
                self.env.pop();
                Ok(result)
            }
        }
    }

    /// Check that a term has the expected type.
    pub fn check(&mut self, term: &Term, expected: &Term) -> Result<(), DependentTypeError> {
        let inferred = self.infer(term)?;
        if self.ctx.definitionally_equal(&inferred, expected) {
            Ok(())
        } else {
            let err = DependentTypeError::Mismatch {
                expected: format!("{expected}"),
                found: format!("{inferred}"),
            };
            self.errors.push(err.clone());
            Err(err)
        }
    }

    /// Look up a variable's type in the context.
    fn lookup(&mut self, name: &str) -> Result<Term, DependentTypeError> {
        for (n, ty) in self.env.iter().rev() {
            if n == name {
                return Ok(ty.clone());
            }
        }
        let err = DependentTypeError::UnboundVar {
            name: name.to_string(),
        };
        self.errors.push(err.clone());
        Err(err)
    }

    /// Infer the universe level of a term that should be a type.
    ///
    /// Returns the universe level, or an error if the term doesn't
    /// inhabit any universe.
    fn infer_universe(&mut self, term: &Term) -> Result<Universe, DependentTypeError> {
        let ty = self.infer(term)?;
        let ty = self.ctx.whnf(&ty);
        match ty {
            Term::Universe(u) => Ok(u),
            _ => {
                let err = DependentTypeError::NotUniverse {
                    found: format!("{ty}"),
                };
                self.errors.push(err.clone());
                Err(err)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dependent::{Term, Universe};

    fn int_type() -> Term {
        Term::Universe(Universe::base())
    }

    /// Helper: set up a checker with "Int : Type₀" in context.
    fn checker_with_int() -> DependentChecker {
        let mut tc = DependentChecker::new();
        // Int is a base type living in Type₀
        tc.env.push(("Int".into(), Term::Universe(Universe::base())));
        tc
    }

    #[test]
    fn infer_variable() {
        let mut tc = DependentChecker::new();
        tc.env.push(("x".into(), Term::Var("Int".into())));
        let ty = tc.infer(&Term::Var("x".into())).unwrap();
        assert_eq!(ty, Term::Var("Int".into()));
    }

    #[test]
    fn infer_unbound_variable() {
        let mut tc = DependentChecker::new();
        let result = tc.infer(&Term::Var("nope".into()));
        assert!(result.is_err());
    }

    #[test]
    fn universe_hierarchy() {
        let mut tc = DependentChecker::new();
        // Type₀ : Type₁
        let ty = tc.infer(&Term::Universe(Universe::base())).unwrap();
        assert_eq!(ty, Term::Universe(Universe(1)));

        // Type₁ : Type₂
        let ty = tc.infer(&Term::Universe(Universe(1))).unwrap();
        assert_eq!(ty, Term::Universe(Universe(2)));
    }

    #[test]
    fn infer_lambda_identity() {
        let mut tc = checker_with_int();
        // λ(x: Int). x  has type  Π(x: Int). Int
        let lam = Term::Lambda {
            param: "x".into(),
            param_type: Box::new(Term::Var("Int".into())),
            body: Box::new(Term::Var("x".into())),
        };
        let ty = tc.infer(&lam).unwrap();
        assert_eq!(
            ty,
            Term::Pi {
                param: "x".into(),
                param_type: Box::new(Term::Var("Int".into())),
                body: Box::new(Term::Var("Int".into())),
            }
        );
    }

    #[test]
    fn infer_application() {
        let mut tc = checker_with_int();
        // Define id: Π(x: Int). Int
        tc.env.push((
            "id".into(),
            Term::Pi {
                param: "x".into(),
                param_type: Box::new(Term::Var("Int".into())),
                body: Box::new(Term::Var("Int".into())),
            },
        ));

        // id 42 : Int
        let app = Term::App {
            func: Box::new(Term::Var("id".into())),
            arg: Box::new(Term::IntLit(42)),
        };
        let ty = tc.infer(&app).unwrap();
        assert_eq!(ty, Term::Var("Int".into()));
    }

    #[test]
    fn pi_type_formation() {
        let mut tc = checker_with_int();
        // Add Nat : Type₀ and Vector : Π(n: Nat). Type₀
        tc.env.push(("Nat".into(), int_type()));
        tc.env.push((
            "Vector".into(),
            Term::Pi {
                param: "n".into(),
                param_type: Box::new(Term::Var("Nat".into())),
                body: Box::new(int_type()),
            },
        ));

        // Π(n: Nat). Vector(n) is well-typed (it's a type)
        let pi = Term::Pi {
            param: "n".into(),
            param_type: Box::new(Term::Var("Nat".into())),
            body: Box::new(Term::App {
                func: Box::new(Term::Var("Vector".into())),
                arg: Box::new(Term::Var("n".into())),
            }),
        };
        let ty = tc.infer(&pi).unwrap();
        // Should be a universe
        match ty {
            Term::Universe(_) => {}
            other => panic!("expected Universe, got {other}"),
        }
    }

    #[test]
    fn sigma_type_formation() {
        let mut tc = checker_with_int();
        tc.env.push(("Nat".into(), int_type()));
        tc.env.push((
            "Vector".into(),
            Term::Pi {
                param: "n".into(),
                param_type: Box::new(Term::Var("Nat".into())),
                body: Box::new(int_type()),
            },
        ));

        // Σ(n: Nat). Vector(n) is well-typed
        let sigma = Term::Sigma {
            param: "n".into(),
            param_type: Box::new(Term::Var("Nat".into())),
            body: Box::new(Term::App {
                func: Box::new(Term::Var("Vector".into())),
                arg: Box::new(Term::Var("n".into())),
            }),
        };
        let ty = tc.infer(&sigma).unwrap();
        match ty {
            Term::Universe(_) => {}
            other => panic!("expected Universe, got {other}"),
        }
    }

    #[test]
    fn pair_and_projections() {
        let mut tc = checker_with_int();

        // Simple non-dependent pair: Σ(_: Int). Int
        let sigma = Term::Sigma {
            param: "_".into(),
            param_type: Box::new(Term::Var("Int".into())),
            body: Box::new(Term::Var("Int".into())),
        };

        let pair = Term::Pair {
            first: Box::new(Term::IntLit(1)),
            second: Box::new(Term::IntLit(2)),
            sigma_type: Box::new(sigma.clone()),
        };
        let ty = tc.infer(&pair).unwrap();
        // Type is the sigma type
        match ty {
            Term::Sigma { .. } => {}
            other => panic!("expected Sigma, got {other}"),
        }

        // fst pair : Int
        tc.env.push(("p".into(), sigma.clone()));
        let fst = Term::Fst(Box::new(Term::Var("p".into())));
        let fst_ty = tc.infer(&fst).unwrap();
        assert_eq!(fst_ty, Term::Var("Int".into()));

        // snd pair : Int
        let snd = Term::Snd(Box::new(Term::Var("p".into())));
        let snd_ty = tc.infer(&snd).unwrap();
        // For non-dependent Sigma, snd type is just the body
        assert_eq!(snd_ty, Term::Var("Int".into()));
    }

    #[test]
    fn equality_type() {
        let mut tc = checker_with_int();
        // Eq(Int, 1, 1) : Type₀
        let eq = Term::Eq {
            ty: Box::new(Term::Var("Int".into())),
            left: Box::new(Term::IntLit(1)),
            right: Box::new(Term::IntLit(1)),
        };
        let ty = tc.infer(&eq).unwrap();
        assert_eq!(ty, Term::Universe(Universe::base()));
    }

    #[test]
    fn reflexivity() {
        let mut tc = checker_with_int();
        // refl(1) : Eq(Int, 1, 1)
        let refl = Term::Refl(Box::new(Term::IntLit(1)));
        let ty = tc.infer(&refl).unwrap();
        assert_eq!(
            ty,
            Term::Eq {
                ty: Box::new(Term::Var("Int".into())),
                left: Box::new(Term::IntLit(1)),
                right: Box::new(Term::IntLit(1)),
            }
        );
    }

    #[test]
    fn int_literal_type() {
        let mut tc = DependentChecker::new();
        let ty = tc.infer(&Term::IntLit(42)).unwrap();
        assert_eq!(ty, Term::Var("Int".into()));
    }

    #[test]
    fn type_annotation() {
        let mut tc = checker_with_int();
        // (42 : Int) : Int
        let ann = Term::Ann {
            term: Box::new(Term::IntLit(42)),
            ty: Box::new(Term::Var("Int".into())),
        };
        let ty = tc.infer(&ann).unwrap();
        assert_eq!(ty, Term::Var("Int".into()));
    }

    #[test]
    fn let_binding() {
        let mut tc = checker_with_int();
        // let x: Int = 42 in x
        let expr = Term::Let {
            name: "x".into(),
            ty: Box::new(Term::Var("Int".into())),
            value: Box::new(Term::IntLit(42)),
            body: Box::new(Term::Var("x".into())),
        };
        let ty = tc.infer(&expr).unwrap();
        assert_eq!(ty, Term::Var("Int".into()));
    }

    #[test]
    fn type_mismatch_detected() {
        let mut tc = checker_with_int();
        // Add Bool : Type₀
        tc.env.push(("Bool".into(), int_type()));

        // Check 42 : Bool — should fail
        let result = tc.check(&Term::IntLit(42), &Term::Var("Bool".into()));
        assert!(result.is_err());
    }

    #[test]
    fn not_function_error() {
        let mut tc = checker_with_int();
        // Applying a non-function: 42 1
        let app = Term::App {
            func: Box::new(Term::IntLit(42)),
            arg: Box::new(Term::IntLit(1)),
        };
        let result = tc.infer(&app);
        assert!(result.is_err());
    }

    #[test]
    fn dependent_application_substitution() {
        let mut tc = checker_with_int();
        // mkPair : Π(n: Int). Int — for simplicity
        // mkPair n returns something of type Int (where the return type depends on n)
        tc.env.push((
            "mkPair".into(),
            Term::Pi {
                param: "n".into(),
                param_type: Box::new(Term::Var("Int".into())),
                body: Box::new(Term::Var("Int".into())),
            },
        ));

        // mkPair 5 : Int (after substituting n := 5 in Int, which is still Int)
        let app = Term::App {
            func: Box::new(Term::Var("mkPair".into())),
            arg: Box::new(Term::IntLit(5)),
        };
        let ty = tc.infer(&app).unwrap();
        assert_eq!(ty, Term::Var("Int".into()));
    }

    #[test]
    fn nested_pi_types() {
        let mut tc = checker_with_int();
        // Π(a: Int). Π(b: Int). Int  (curried binary function)
        let pi = Term::Pi {
            param: "a".into(),
            param_type: Box::new(Term::Var("Int".into())),
            body: Box::new(Term::Pi {
                param: "b".into(),
                param_type: Box::new(Term::Var("Int".into())),
                body: Box::new(Term::Var("Int".into())),
            }),
        };
        let ty = tc.infer(&pi).unwrap();
        match ty {
            Term::Universe(_) => {}
            other => panic!("expected Universe, got {other}"),
        }
    }
}
