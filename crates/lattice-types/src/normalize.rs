//! Type-level computation via normalization for dependent types.
//!
//! Implements β-reduction, δ-reduction (definition unfolding),
//! ι-reduction (projections), and let-reduction.

use crate::dependent::Term;
use std::collections::HashMap;

/// Evaluation context holding named definitions for δ-reduction.
pub struct NormContext {
    definitions: HashMap<String, Term>,
}

impl Default for NormContext {
    fn default() -> Self {
        Self::new()
    }
}

impl NormContext {
    pub fn new() -> Self {
        Self {
            definitions: HashMap::new(),
        }
    }

    /// Register a definition: `name := term`.
    pub fn define(&mut self, name: String, term: Term) {
        self.definitions.insert(name, term);
    }

    /// Look up a definition.
    pub fn lookup(&self, name: &str) -> Option<&Term> {
        self.definitions.get(name)
    }

    /// Normalize a term to weak head normal form (WHNF).
    ///
    /// Only reduces the outermost redex — does not normalize under binders.
    pub fn whnf(&self, term: &Term) -> Term {
        match term {
            // δ-reduction: unfold definitions
            Term::Var(name) => {
                if let Some(def) = self.definitions.get(name) {
                    self.whnf(def)
                } else {
                    term.clone()
                }
            }

            // β-reduction: (λx.body) arg → body[x := arg]
            Term::App { func, arg } => {
                let func_whnf = self.whnf(func);
                match &func_whnf {
                    Term::Lambda { param, body, .. } => {
                        let result = substitute(body, param, arg);
                        self.whnf(&result)
                    }
                    _ => Term::App {
                        func: Box::new(func_whnf),
                        arg: arg.clone(),
                    },
                }
            }

            // ι-reduction: fst (a, b) → a
            Term::Fst(pair) => {
                let pair_whnf = self.whnf(pair);
                match &pair_whnf {
                    Term::Pair { first, .. } => self.whnf(first),
                    _ => Term::Fst(Box::new(pair_whnf)),
                }
            }

            // ι-reduction: snd (a, b) → b
            Term::Snd(pair) => {
                let pair_whnf = self.whnf(pair);
                match &pair_whnf {
                    Term::Pair { second, .. } => self.whnf(second),
                    _ => Term::Snd(Box::new(pair_whnf)),
                }
            }

            // Let reduction: let x = v in body → body[x := v]
            Term::Let {
                name, value, body, ..
            } => {
                let result = substitute(body, name, value);
                self.whnf(&result)
            }

            // Annotation erasure
            Term::Ann { term, .. } => self.whnf(term),

            // Everything else is already in WHNF
            _ => term.clone(),
        }
    }

    /// Fully normalize a term (normal form).
    ///
    /// Reduces under all binders and sub-expressions.
    pub fn normalize(&self, term: &Term) -> Term {
        let whnf = self.whnf(term);
        match &whnf {
            Term::Var(_) | Term::Universe(_) | Term::IntLit(_) => whnf,

            Term::Pi {
                param,
                param_type,
                body,
            } => Term::Pi {
                param: param.clone(),
                param_type: Box::new(self.normalize(param_type)),
                body: Box::new(self.normalize(body)),
            },

            Term::Lambda {
                param,
                param_type,
                body,
            } => Term::Lambda {
                param: param.clone(),
                param_type: Box::new(self.normalize(param_type)),
                body: Box::new(self.normalize(body)),
            },

            Term::App { func, arg } => Term::App {
                func: Box::new(self.normalize(func)),
                arg: Box::new(self.normalize(arg)),
            },

            Term::Sigma {
                param,
                param_type,
                body,
            } => Term::Sigma {
                param: param.clone(),
                param_type: Box::new(self.normalize(param_type)),
                body: Box::new(self.normalize(body)),
            },

            Term::Pair {
                first,
                second,
                sigma_type,
            } => Term::Pair {
                first: Box::new(self.normalize(first)),
                second: Box::new(self.normalize(second)),
                sigma_type: Box::new(self.normalize(sigma_type)),
            },

            Term::Fst(t) => Term::Fst(Box::new(self.normalize(t))),
            Term::Snd(t) => Term::Snd(Box::new(self.normalize(t))),

            Term::Eq { ty, left, right } => Term::Eq {
                ty: Box::new(self.normalize(ty)),
                left: Box::new(self.normalize(left)),
                right: Box::new(self.normalize(right)),
            },

            Term::Refl(t) => Term::Refl(Box::new(self.normalize(t))),

            Term::Let { .. } => {
                // whnf already reduced let, so this shouldn't happen
                unreachable!("let should be reduced by whnf")
            }

            Term::Ann { .. } => {
                // whnf already reduced annotation
                unreachable!("annotation should be reduced by whnf")
            }
        }
    }

    /// Check definitional equality (after normalization).
    pub fn definitionally_equal(&self, a: &Term, b: &Term) -> bool {
        let a_nf = self.normalize(a);
        let b_nf = self.normalize(b);
        alpha_equal(&a_nf, &b_nf)
    }
}

/// Substitution: replace variable `name` with `replacement` in `term`.
///
/// Performs capture-avoiding substitution by renaming bound variables
/// when necessary to avoid shadowing.
pub fn substitute(term: &Term, name: &str, replacement: &Term) -> Term {
    match term {
        Term::Var(v) => {
            if v == name {
                replacement.clone()
            } else {
                term.clone()
            }
        }

        Term::Universe(_) | Term::IntLit(_) => term.clone(),

        Term::Pi {
            param,
            param_type,
            body,
        } => {
            let new_param_type = substitute(param_type, name, replacement);
            if param == name {
                // param shadows name — don't substitute in body
                Term::Pi {
                    param: param.clone(),
                    param_type: Box::new(new_param_type),
                    body: body.clone(),
                }
            } else if replacement.free_vars().contains(param) {
                // Capture-avoiding: rename the bound variable
                let fresh = fresh_name(param, &replacement.free_vars());
                let renamed_body = substitute(body, param, &Term::Var(fresh.clone()));
                Term::Pi {
                    param: fresh,
                    param_type: Box::new(new_param_type),
                    body: Box::new(substitute(&renamed_body, name, replacement)),
                }
            } else {
                Term::Pi {
                    param: param.clone(),
                    param_type: Box::new(new_param_type),
                    body: Box::new(substitute(body, name, replacement)),
                }
            }
        }

        Term::Lambda {
            param,
            param_type,
            body,
        } => {
            let new_param_type = substitute(param_type, name, replacement);
            if param == name {
                Term::Lambda {
                    param: param.clone(),
                    param_type: Box::new(new_param_type),
                    body: body.clone(),
                }
            } else if replacement.free_vars().contains(param) {
                let fresh = fresh_name(param, &replacement.free_vars());
                let renamed_body = substitute(body, param, &Term::Var(fresh.clone()));
                Term::Lambda {
                    param: fresh,
                    param_type: Box::new(new_param_type),
                    body: Box::new(substitute(&renamed_body, name, replacement)),
                }
            } else {
                Term::Lambda {
                    param: param.clone(),
                    param_type: Box::new(new_param_type),
                    body: Box::new(substitute(body, name, replacement)),
                }
            }
        }

        Term::App { func, arg } => Term::App {
            func: Box::new(substitute(func, name, replacement)),
            arg: Box::new(substitute(arg, name, replacement)),
        },

        Term::Sigma {
            param,
            param_type,
            body,
        } => {
            let new_param_type = substitute(param_type, name, replacement);
            if param == name {
                Term::Sigma {
                    param: param.clone(),
                    param_type: Box::new(new_param_type),
                    body: body.clone(),
                }
            } else if replacement.free_vars().contains(param) {
                let fresh = fresh_name(param, &replacement.free_vars());
                let renamed_body = substitute(body, param, &Term::Var(fresh.clone()));
                Term::Sigma {
                    param: fresh,
                    param_type: Box::new(new_param_type),
                    body: Box::new(substitute(&renamed_body, name, replacement)),
                }
            } else {
                Term::Sigma {
                    param: param.clone(),
                    param_type: Box::new(new_param_type),
                    body: Box::new(substitute(body, name, replacement)),
                }
            }
        }

        Term::Pair {
            first,
            second,
            sigma_type,
        } => Term::Pair {
            first: Box::new(substitute(first, name, replacement)),
            second: Box::new(substitute(second, name, replacement)),
            sigma_type: Box::new(substitute(sigma_type, name, replacement)),
        },

        Term::Fst(t) => Term::Fst(Box::new(substitute(t, name, replacement))),
        Term::Snd(t) => Term::Snd(Box::new(substitute(t, name, replacement))),

        Term::Eq { ty, left, right } => Term::Eq {
            ty: Box::new(substitute(ty, name, replacement)),
            left: Box::new(substitute(left, name, replacement)),
            right: Box::new(substitute(right, name, replacement)),
        },

        Term::Refl(t) => Term::Refl(Box::new(substitute(t, name, replacement))),

        Term::Let {
            name: let_name,
            ty,
            value,
            body,
        } => {
            let new_ty = substitute(ty, name, replacement);
            let new_value = substitute(value, name, replacement);
            if let_name == name {
                // let_name shadows name — don't substitute in body
                Term::Let {
                    name: let_name.clone(),
                    ty: Box::new(new_ty),
                    value: Box::new(new_value),
                    body: body.clone(),
                }
            } else if replacement.free_vars().contains(let_name) {
                let fresh = fresh_name(let_name, &replacement.free_vars());
                let renamed_body = substitute(body, let_name, &Term::Var(fresh.clone()));
                Term::Let {
                    name: fresh,
                    ty: Box::new(new_ty),
                    value: Box::new(new_value),
                    body: Box::new(substitute(&renamed_body, name, replacement)),
                }
            } else {
                Term::Let {
                    name: let_name.clone(),
                    ty: Box::new(new_ty),
                    value: Box::new(new_value),
                    body: Box::new(substitute(body, name, replacement)),
                }
            }
        }

        Term::Ann { term: t, ty } => Term::Ann {
            term: Box::new(substitute(t, name, replacement)),
            ty: Box::new(substitute(ty, name, replacement)),
        },
    }
}

/// Alpha-equivalence: structurally equal up to variable renaming.
pub fn alpha_equal(a: &Term, b: &Term) -> bool {
    alpha_equal_inner(a, b, &mut Vec::new(), &mut Vec::new())
}

fn alpha_equal_inner(
    a: &Term,
    b: &Term,
    a_bindings: &mut Vec<String>,
    b_bindings: &mut Vec<String>,
) -> bool {
    match (a, b) {
        (Term::Var(x), Term::Var(y)) => {
            // Check if they refer to the same bound variable position
            let a_pos = a_bindings.iter().rev().position(|n| n == x);
            let b_pos = b_bindings.iter().rev().position(|n| n == y);
            match (a_pos, b_pos) {
                (Some(ai), Some(bi)) => ai == bi,
                (None, None) => x == y, // both free — must be same name
                _ => false,
            }
        }

        (Term::Universe(u1), Term::Universe(u2)) => u1 == u2,
        (Term::IntLit(n1), Term::IntLit(n2)) => n1 == n2,

        (
            Term::Pi {
                param: p1,
                param_type: t1,
                body: b1,
            },
            Term::Pi {
                param: p2,
                param_type: t2,
                body: b2,
            },
        )
        | (
            Term::Lambda {
                param: p1,
                param_type: t1,
                body: b1,
            },
            Term::Lambda {
                param: p2,
                param_type: t2,
                body: b2,
            },
        )
        | (
            Term::Sigma {
                param: p1,
                param_type: t1,
                body: b1,
            },
            Term::Sigma {
                param: p2,
                param_type: t2,
                body: b2,
            },
        ) => {
            if !alpha_equal_inner(t1, t2, a_bindings, b_bindings) {
                return false;
            }
            a_bindings.push(p1.clone());
            b_bindings.push(p2.clone());
            let result = alpha_equal_inner(b1, b2, a_bindings, b_bindings);
            a_bindings.pop();
            b_bindings.pop();
            result
        }

        (
            Term::App {
                func: f1,
                arg: a1,
            },
            Term::App {
                func: f2,
                arg: a2,
            },
        ) => {
            alpha_equal_inner(f1, f2, a_bindings, b_bindings)
                && alpha_equal_inner(a1, a2, a_bindings, b_bindings)
        }

        (
            Term::Pair {
                first: f1,
                second: s1,
                sigma_type: t1,
            },
            Term::Pair {
                first: f2,
                second: s2,
                sigma_type: t2,
            },
        ) => {
            alpha_equal_inner(f1, f2, a_bindings, b_bindings)
                && alpha_equal_inner(s1, s2, a_bindings, b_bindings)
                && alpha_equal_inner(t1, t2, a_bindings, b_bindings)
        }

        (Term::Fst(t1), Term::Fst(t2)) | (Term::Snd(t1), Term::Snd(t2)) => {
            alpha_equal_inner(t1, t2, a_bindings, b_bindings)
        }

        (
            Term::Eq {
                ty: t1,
                left: l1,
                right: r1,
            },
            Term::Eq {
                ty: t2,
                left: l2,
                right: r2,
            },
        ) => {
            alpha_equal_inner(t1, t2, a_bindings, b_bindings)
                && alpha_equal_inner(l1, l2, a_bindings, b_bindings)
                && alpha_equal_inner(r1, r2, a_bindings, b_bindings)
        }

        (Term::Refl(t1), Term::Refl(t2)) => {
            alpha_equal_inner(t1, t2, a_bindings, b_bindings)
        }

        (
            Term::Let {
                name: n1,
                ty: ty1,
                value: v1,
                body: b1,
            },
            Term::Let {
                name: n2,
                ty: ty2,
                value: v2,
                body: b2,
            },
        ) => {
            if !alpha_equal_inner(ty1, ty2, a_bindings, b_bindings) {
                return false;
            }
            if !alpha_equal_inner(v1, v2, a_bindings, b_bindings) {
                return false;
            }
            a_bindings.push(n1.clone());
            b_bindings.push(n2.clone());
            let result = alpha_equal_inner(b1, b2, a_bindings, b_bindings);
            a_bindings.pop();
            b_bindings.pop();
            result
        }

        (
            Term::Ann {
                term: t1,
                ty: ty1,
            },
            Term::Ann {
                term: t2,
                ty: ty2,
            },
        ) => {
            alpha_equal_inner(t1, t2, a_bindings, b_bindings)
                && alpha_equal_inner(ty1, ty2, a_bindings, b_bindings)
        }

        _ => false,
    }
}

/// Generate a fresh variable name that doesn't collide with any in `avoid`.
fn fresh_name(
    base: &str,
    avoid: &std::collections::HashSet<String>,
) -> String {
    let mut candidate = format!("{base}'");
    while avoid.contains(&candidate) {
        candidate.push('\'');
    }
    candidate
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dependent::Term;

    #[test]
    fn beta_reduction_identity() {
        let ctx = NormContext::new();
        // (λ(x: Int). x) 42 → 42
        let term = Term::App {
            func: Box::new(Term::Lambda {
                param: "x".into(),
                param_type: Box::new(Term::Var("Int".into())),
                body: Box::new(Term::Var("x".into())),
            }),
            arg: Box::new(Term::IntLit(42)),
        };
        let result = ctx.normalize(&term);
        assert_eq!(result, Term::IntLit(42));
    }

    #[test]
    fn beta_reduction_nested() {
        let ctx = NormContext::new();
        // (λ(x: Int). (λ(y: Int). x)) 1 2 → 1
        // First apply outer: (λ(y: Int). 1) 2 → 1
        let term = Term::App {
            func: Box::new(Term::App {
                func: Box::new(Term::Lambda {
                    param: "x".into(),
                    param_type: Box::new(Term::Var("Int".into())),
                    body: Box::new(Term::Lambda {
                        param: "y".into(),
                        param_type: Box::new(Term::Var("Int".into())),
                        body: Box::new(Term::Var("x".into())),
                    }),
                }),
                arg: Box::new(Term::IntLit(1)),
            }),
            arg: Box::new(Term::IntLit(2)),
        };
        let result = ctx.normalize(&term);
        assert_eq!(result, Term::IntLit(1));
    }

    #[test]
    fn delta_reduction() {
        let mut ctx = NormContext::new();
        ctx.define("myval".into(), Term::IntLit(99));
        let term = Term::Var("myval".into());
        let result = ctx.normalize(&term);
        assert_eq!(result, Term::IntLit(99));
    }

    #[test]
    fn let_reduction() {
        let ctx = NormContext::new();
        // let x: Int = 5 in x → 5
        let term = Term::Let {
            name: "x".into(),
            ty: Box::new(Term::Var("Int".into())),
            value: Box::new(Term::IntLit(5)),
            body: Box::new(Term::Var("x".into())),
        };
        let result = ctx.normalize(&term);
        assert_eq!(result, Term::IntLit(5));
    }

    #[test]
    fn fst_projection() {
        let ctx = NormContext::new();
        let sigma = Term::Sigma {
            param: "x".into(),
            param_type: Box::new(Term::Var("Int".into())),
            body: Box::new(Term::Var("Int".into())),
        };
        let term = Term::Fst(Box::new(Term::Pair {
            first: Box::new(Term::IntLit(1)),
            second: Box::new(Term::IntLit(2)),
            sigma_type: Box::new(sigma),
        }));
        let result = ctx.normalize(&term);
        assert_eq!(result, Term::IntLit(1));
    }

    #[test]
    fn snd_projection() {
        let ctx = NormContext::new();
        let sigma = Term::Sigma {
            param: "x".into(),
            param_type: Box::new(Term::Var("Int".into())),
            body: Box::new(Term::Var("Int".into())),
        };
        let term = Term::Snd(Box::new(Term::Pair {
            first: Box::new(Term::IntLit(1)),
            second: Box::new(Term::IntLit(2)),
            sigma_type: Box::new(sigma),
        }));
        let result = ctx.normalize(&term);
        assert_eq!(result, Term::IntLit(2));
    }

    #[test]
    fn substitution_simple() {
        // x[x := 42] → 42
        let result = substitute(&Term::Var("x".into()), "x", &Term::IntLit(42));
        assert_eq!(result, Term::IntLit(42));
    }

    #[test]
    fn substitution_avoids_free_var() {
        // y[x := 42] → y
        let result = substitute(&Term::Var("y".into()), "x", &Term::IntLit(42));
        assert_eq!(result, Term::Var("y".into()));
    }

    #[test]
    fn substitution_under_binder_shadow() {
        // (λ(x: Int). x)[x := 42] → (λ(x: Int). x)  — x is shadowed
        let lam = Term::Lambda {
            param: "x".into(),
            param_type: Box::new(Term::Var("Int".into())),
            body: Box::new(Term::Var("x".into())),
        };
        let result = substitute(&lam, "x", &Term::IntLit(42));
        assert_eq!(result, lam);
    }

    #[test]
    fn substitution_capture_avoiding() {
        // (λ(y: Int). x)[x := y] should rename y to avoid capture
        let lam = Term::Lambda {
            param: "y".into(),
            param_type: Box::new(Term::Var("Int".into())),
            body: Box::new(Term::Var("x".into())),
        };
        let result = substitute(&lam, "x", &Term::Var("y".into()));
        // The body should now contain y (the free var), and the parameter should be renamed
        match &result {
            Term::Lambda { param, body, .. } => {
                assert_ne!(param, "y"); // was renamed to avoid capture
                assert_eq!(**body, Term::Var("y".into())); // the replacement
            }
            other => panic!("expected Lambda, got {other:?}"),
        }
    }

    #[test]
    fn alpha_equal_identical() {
        let t = Term::Lambda {
            param: "x".into(),
            param_type: Box::new(Term::Var("Int".into())),
            body: Box::new(Term::Var("x".into())),
        };
        assert!(alpha_equal(&t, &t));
    }

    #[test]
    fn alpha_equal_renamed_binder() {
        // λ(x: Int). x  ≡α  λ(y: Int). y
        let t1 = Term::Lambda {
            param: "x".into(),
            param_type: Box::new(Term::Var("Int".into())),
            body: Box::new(Term::Var("x".into())),
        };
        let t2 = Term::Lambda {
            param: "y".into(),
            param_type: Box::new(Term::Var("Int".into())),
            body: Box::new(Term::Var("y".into())),
        };
        assert!(alpha_equal(&t1, &t2));
    }

    #[test]
    fn alpha_not_equal_different_structure() {
        let t1 = Term::Var("x".into());
        let t2 = Term::IntLit(42);
        assert!(!alpha_equal(&t1, &t2));
    }

    #[test]
    fn alpha_equal_free_vars_must_match() {
        // λ(x: Int). y  ≢α  λ(x: Int). z  (different free vars)
        let t1 = Term::Lambda {
            param: "x".into(),
            param_type: Box::new(Term::Var("Int".into())),
            body: Box::new(Term::Var("y".into())),
        };
        let t2 = Term::Lambda {
            param: "x".into(),
            param_type: Box::new(Term::Var("Int".into())),
            body: Box::new(Term::Var("z".into())),
        };
        assert!(!alpha_equal(&t1, &t2));
    }

    #[test]
    fn definitional_equality_after_reduction() {
        let mut ctx = NormContext::new();
        ctx.define("id".into(), Term::Lambda {
            param: "x".into(),
            param_type: Box::new(Term::Var("Int".into())),
            body: Box::new(Term::Var("x".into())),
        });

        // id 42 =def= 42
        let a = Term::App {
            func: Box::new(Term::Var("id".into())),
            arg: Box::new(Term::IntLit(42)),
        };
        let b = Term::IntLit(42);
        assert!(ctx.definitionally_equal(&a, &b));
    }

    #[test]
    fn normalize_under_pi() {
        let mut ctx = NormContext::new();
        ctx.define("T".into(), Term::Var("Int".into()));

        // Π(x: T). T  normalizes to  Π(x: Int). Int
        let term = Term::Pi {
            param: "x".into(),
            param_type: Box::new(Term::Var("T".into())),
            body: Box::new(Term::Var("T".into())),
        };
        let result = ctx.normalize(&term);
        assert_eq!(
            result,
            Term::Pi {
                param: "x".into(),
                param_type: Box::new(Term::Var("Int".into())),
                body: Box::new(Term::Var("Int".into())),
            }
        );
    }

    #[test]
    fn annotation_erasure() {
        let ctx = NormContext::new();
        // (42 : Int) → 42
        let term = Term::Ann {
            term: Box::new(Term::IntLit(42)),
            ty: Box::new(Term::Var("Int".into())),
        };
        let result = ctx.normalize(&term);
        assert_eq!(result, Term::IntLit(42));
    }

    #[test]
    fn whnf_does_not_reduce_under_lambda() {
        let ctx = NormContext::new();
        // λ(x: Int). (λ(y: Int). y) x  — whnf should NOT reduce the body
        let inner_app = Term::App {
            func: Box::new(Term::Lambda {
                param: "y".into(),
                param_type: Box::new(Term::Var("Int".into())),
                body: Box::new(Term::Var("y".into())),
            }),
            arg: Box::new(Term::Var("x".into())),
        };
        let term = Term::Lambda {
            param: "x".into(),
            param_type: Box::new(Term::Var("Int".into())),
            body: Box::new(inner_app.clone()),
        };
        let whnf = ctx.whnf(&term);
        // whnf should return the lambda as-is (not reduce the body)
        assert_eq!(whnf, term);

        // But full normalization should reduce the body
        let nf = ctx.normalize(&term);
        assert_eq!(
            nf,
            Term::Lambda {
                param: "x".into(),
                param_type: Box::new(Term::Var("Int".into())),
                body: Box::new(Term::Var("x".into())),
            }
        );
    }
}
