//! Core dependent type theory constructs for Lattice.
//!
//! Implements the Calculus of Constructions with:
//! - Universe hierarchy (Type₀ : Type₁ : Type₂ : ...)
//! - Π-types (dependent function types)
//! - Σ-types (dependent pair types)
//! - Propositional equality (Eq, Refl)
//! - Type-level computation via normalization

use serde::{Deserialize, Serialize};

/// Universe levels: Type₀ : Type₁ : Type₂ : ...
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Universe(pub u32);

impl Universe {
    pub fn base() -> Self {
        Universe(0)
    }

    pub fn succ(&self) -> Self {
        Universe(self.0 + 1)
    }

    pub fn max(a: &Self, b: &Self) -> Self {
        Universe(std::cmp::max(a.0, b.0))
    }
}

impl std::fmt::Display for Universe {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Type{}", self.0)
    }
}

/// Core terms of the dependent type theory (Calculus of Constructions).
///
/// Terms serve as both types and values — a hallmark of dependent type theory
/// where the distinction between the two is erased.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Term {
    /// Variable (named)
    Var(String),

    /// Universe/Sort: Type_n
    Universe(Universe),

    /// Pi type (dependent function): Π(x: A). B
    /// When x doesn't appear free in B, this is just A → B
    Pi {
        param: String,
        param_type: Box<Term>,
        body: Box<Term>,
    },

    /// Lambda: λ(x: A). body
    Lambda {
        param: String,
        param_type: Box<Term>,
        body: Box<Term>,
    },

    /// Application: f a
    App {
        func: Box<Term>,
        arg: Box<Term>,
    },

    /// Sigma type (dependent pair): Σ(x: A). B
    Sigma {
        param: String,
        param_type: Box<Term>,
        body: Box<Term>,
    },

    /// Pair constructor: (a, b) : Σ(x: A). B
    Pair {
        first: Box<Term>,
        second: Box<Term>,
        sigma_type: Box<Term>,
    },

    /// First projection: fst p
    Fst(Box<Term>),

    /// Second projection: snd p
    Snd(Box<Term>),

    /// Propositional equality: Eq(A, a, b)
    Eq {
        ty: Box<Term>,
        left: Box<Term>,
        right: Box<Term>,
    },

    /// Reflexivity: refl : Eq(A, a, a)
    Refl(Box<Term>),

    /// Let binding: let x: A = e in body
    Let {
        name: String,
        ty: Box<Term>,
        value: Box<Term>,
        body: Box<Term>,
    },

    /// Integer literal
    IntLit(i64),

    /// Type annotation: (e : A)
    Ann {
        term: Box<Term>,
        ty: Box<Term>,
    },
}

impl Term {
    /// Collect the set of free variables in this term.
    pub fn free_vars(&self) -> std::collections::HashSet<String> {
        use std::collections::HashSet;
        match self {
            Term::Var(name) => {
                let mut s = HashSet::new();
                s.insert(name.clone());
                s
            }
            Term::Universe(_) | Term::IntLit(_) => HashSet::new(),
            Term::Pi {
                param,
                param_type,
                body,
            }
            | Term::Lambda {
                param,
                param_type,
                body,
            }
            | Term::Sigma {
                param,
                param_type,
                body,
            } => {
                let mut fv = param_type.free_vars();
                let mut body_fv = body.free_vars();
                body_fv.remove(param);
                fv.extend(body_fv);
                fv
            }
            Term::App { func, arg } => {
                let mut fv = func.free_vars();
                fv.extend(arg.free_vars());
                fv
            }
            Term::Pair {
                first,
                second,
                sigma_type,
            } => {
                let mut fv = first.free_vars();
                fv.extend(second.free_vars());
                fv.extend(sigma_type.free_vars());
                fv
            }
            Term::Fst(t) | Term::Snd(t) | Term::Refl(t) => t.free_vars(),
            Term::Eq { ty, left, right } => {
                let mut fv = ty.free_vars();
                fv.extend(left.free_vars());
                fv.extend(right.free_vars());
                fv
            }
            Term::Let {
                name,
                ty,
                value,
                body,
            } => {
                let mut fv = ty.free_vars();
                fv.extend(value.free_vars());
                let mut body_fv = body.free_vars();
                body_fv.remove(name);
                fv.extend(body_fv);
                fv
            }
            Term::Ann { term, ty } => {
                let mut fv = term.free_vars();
                fv.extend(ty.free_vars());
                fv
            }
        }
    }
}

impl std::fmt::Display for Term {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Term::Var(name) => write!(f, "{name}"),
            Term::Universe(u) => write!(f, "{u}"),
            Term::Pi {
                param,
                param_type,
                body,
            } => {
                if body.free_vars().contains(param) {
                    write!(f, "Π({param}: {param_type}). {body}")
                } else {
                    write!(f, "{param_type} → {body}")
                }
            }
            Term::Lambda {
                param,
                param_type,
                body,
            } => write!(f, "λ({param}: {param_type}). {body}"),
            Term::App { func, arg } => write!(f, "({func} {arg})"),
            Term::Sigma {
                param,
                param_type,
                body,
            } => {
                if body.free_vars().contains(param) {
                    write!(f, "Σ({param}: {param_type}). {body}")
                } else {
                    write!(f, "{param_type} × {body}")
                }
            }
            Term::Pair {
                first, second, ..
            } => write!(f, "({first}, {second})"),
            Term::Fst(t) => write!(f, "fst {t}"),
            Term::Snd(t) => write!(f, "snd {t}"),
            Term::Eq { ty, left, right } => write!(f, "Eq({ty}, {left}, {right})"),
            Term::Refl(t) => write!(f, "refl({t})"),
            Term::Let {
                name,
                ty,
                value,
                body,
            } => write!(f, "let {name}: {ty} = {value} in {body}"),
            Term::IntLit(n) => write!(f, "{n}"),
            Term::Ann { term, ty } => write!(f, "({term} : {ty})"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn universe_base_and_succ() {
        let u0 = Universe::base();
        assert_eq!(u0.0, 0);
        let u1 = u0.succ();
        assert_eq!(u1.0, 1);
        let u2 = u1.succ();
        assert_eq!(u2.0, 2);
    }

    #[test]
    fn universe_max() {
        let u1 = Universe(1);
        let u3 = Universe(3);
        assert_eq!(Universe::max(&u1, &u3), Universe(3));
        assert_eq!(Universe::max(&u3, &u1), Universe(3));
    }

    #[test]
    fn universe_display() {
        assert_eq!(Universe(0).to_string(), "Type0");
        assert_eq!(Universe(2).to_string(), "Type2");
    }

    #[test]
    fn free_vars_var() {
        let t = Term::Var("x".into());
        let fv = t.free_vars();
        assert!(fv.contains("x"));
        assert_eq!(fv.len(), 1);
    }

    #[test]
    fn free_vars_lambda_binds() {
        // λ(x: Int). x — x is bound, no free vars (Int is a Var here for the test)
        let t = Term::Lambda {
            param: "x".into(),
            param_type: Box::new(Term::Var("Int".into())),
            body: Box::new(Term::Var("x".into())),
        };
        let fv = t.free_vars();
        assert!(!fv.contains("x"));
        assert!(fv.contains("Int"));
    }

    #[test]
    fn free_vars_app() {
        let t = Term::App {
            func: Box::new(Term::Var("f".into())),
            arg: Box::new(Term::Var("x".into())),
        };
        let fv = t.free_vars();
        assert!(fv.contains("f"));
        assert!(fv.contains("x"));
    }

    #[test]
    fn term_display_pi_dependent() {
        let t = Term::Pi {
            param: "n".into(),
            param_type: Box::new(Term::Var("Nat".into())),
            body: Box::new(Term::App {
                func: Box::new(Term::Var("Vector".into())),
                arg: Box::new(Term::Var("n".into())),
            }),
        };
        assert_eq!(t.to_string(), "Π(n: Nat). (Vector n)");
    }

    #[test]
    fn term_display_pi_non_dependent() {
        // When param doesn't appear in body, show as arrow
        let t = Term::Pi {
            param: "x".into(),
            param_type: Box::new(Term::Var("Int".into())),
            body: Box::new(Term::Var("Bool".into())),
        };
        assert_eq!(t.to_string(), "Int → Bool");
    }
}
