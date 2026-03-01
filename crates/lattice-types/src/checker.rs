//! Bidirectional type checker for Lattice programs.
//!
//! Implements synthesis (infer) and checking (verify) modes,
//! subtyping, structural equality, and unification for inference.

use std::collections::HashMap;

use crate::ast::{BinOp, Expr, Span};
use crate::environment::TypeEnv;
use crate::types::{PhysicalUnit, Type, TypeVarId};

/// Errors produced during type checking.
#[derive(Debug, Clone, thiserror::Error)]
pub enum TypeError {
    #[error("type mismatch at {span}: expected {expected}, found {found}")]
    Mismatch {
        expected: Type,
        found: Type,
        span: Span,
    },

    #[error("unbound variable `{name}` at {span}")]
    UnboundVariable { name: String, span: Span },

    #[error("not a function type: {type_} at {span}")]
    NotAFunction { type_: Type, span: Span },

    #[error("unit dimension mismatch at {span}: {left} vs {right}")]
    UnitMismatch {
        left: PhysicalUnit,
        right: PhysicalUnit,
        span: Span,
    },

    #[error("arity mismatch at {span}: expected {expected} args, found {found}")]
    ArityMismatch {
        expected: usize,
        found: usize,
        span: Span,
    },

    #[error("not a record type at {span}: {type_}")]
    NotARecord { type_: Type, span: Span },

    #[error("no field `{field}` in record type at {span}")]
    NoSuchField { field: String, span: Span },

    #[error("cannot apply operator to {left} and {right} at {span}")]
    InvalidOperands {
        left: Type,
        right: Type,
        span: Span,
    },

    #[error("unification failure: cannot unify {a} with {b}")]
    UnificationFailure { a: Type, b: Type },

    #[error("condition must be Bool, found {found} at {span}")]
    NonBoolCondition { found: Type, span: Span },

    #[error("if branches have different types at {span}: {then_type} vs {else_type}")]
    BranchMismatch {
        then_type: Type,
        else_type: Type,
        span: Span,
    },
}

/// The bidirectional type checker.
pub struct TypeChecker {
    pub env: TypeEnv,
    errors: Vec<TypeError>,
    next_var: u32,
    substitution: HashMap<TypeVarId, Type>,
}

impl TypeChecker {
    /// Create a new type checker with a default (built-in populated) environment.
    pub fn new() -> Self {
        Self {
            env: TypeEnv::new(),
            errors: Vec::new(),
            next_var: 0,
            substitution: HashMap::new(),
        }
    }

    /// Create a type checker with a specific environment.
    pub fn with_env(env: TypeEnv) -> Self {
        Self {
            env,
            errors: Vec::new(),
            next_var: 0,
            substitution: HashMap::new(),
        }
    }

    /// Returns all accumulated errors.
    pub fn errors(&self) -> &[TypeError] {
        &self.errors
    }

    /// Returns true if type checking produced no errors.
    pub fn is_ok(&self) -> bool {
        self.errors.is_empty()
    }

    /// Generate a fresh type variable.
    pub fn fresh_var(&mut self) -> Type {
        let id = TypeVarId(self.next_var);
        self.next_var += 1;
        Type::Var(id)
    }

    /// Apply the current substitution to a type, resolving type variables.
    pub fn apply_subst(&self, ty: &Type) -> Type {
        match ty {
            Type::Var(id) => {
                if let Some(resolved) = self.substitution.get(id) {
                    self.apply_subst(resolved)
                } else {
                    ty.clone()
                }
            }
            Type::Function { params, return_type } => Type::Function {
                params: params.iter().map(|p| self.apply_subst(p)).collect(),
                return_type: Box::new(self.apply_subst(return_type)),
            },
            Type::Stream(inner) => Type::Stream(Box::new(self.apply_subst(inner))),
            Type::Distribution(inner) => Type::Distribution(Box::new(self.apply_subst(inner))),
            Type::WithUnit { base, unit } => Type::WithUnit {
                base: Box::new(self.apply_subst(base)),
                unit: unit.clone(),
            },
            Type::Applied { constructor, args } => Type::Applied {
                constructor: constructor.clone(),
                args: args.iter().map(|a| self.apply_subst(a)).collect(),
            },
            Type::Product { fields } => Type::Product {
                fields: fields
                    .iter()
                    .map(|(n, t)| (n.clone(), self.apply_subst(t)))
                    .collect(),
            },
            Type::Sum { variants } => Type::Sum {
                variants: variants.clone(),
            },
            Type::Refinement { var, base, predicate } => Type::Refinement {
                var: var.clone(),
                base: Box::new(self.apply_subst(base)),
                predicate: predicate.clone(),
            },
            Type::DependentFunction { param, param_type, return_type } => {
                Type::DependentFunction {
                    param: param.clone(),
                    param_type: Box::new(self.apply_subst(param_type)),
                    return_type: Box::new(self.apply_subst(return_type)),
                }
            }
            _ => ty.clone(),
        }
    }

    // ── Synthesis (inference) ──────────────────────────────────────

    /// Synthesize (infer) the type of an expression.
    pub fn synthesize(&mut self, expr: &Expr) -> Result<Type, TypeError> {
        match expr {
            Expr::IntLit { .. } => Ok(Type::Int),
            Expr::FloatLit { .. } => Ok(Type::Float),
            Expr::StringLit { .. } => Ok(Type::String),
            Expr::BoolLit { .. } => Ok(Type::Bool),
            Expr::UnitLit { .. } => Ok(Type::Unit),

            Expr::Var { name, span } => self.env.lookup(name).cloned().ok_or_else(|| {
                let err = TypeError::UnboundVariable {
                    name: name.clone(),
                    span: *span,
                };
                self.errors.push(err.clone());
                err
            }),

            Expr::Let {
                name,
                annotation,
                value,
                body,
                span: _,
            } => {
                let val_type = if let Some(ann) = annotation {
                    self.check(value, ann)?;
                    ann.clone()
                } else {
                    self.synthesize(value)?
                };

                self.env.push_scope();
                self.env.bind(name.clone(), val_type);
                let body_type = self.synthesize(body);
                self.env.pop_scope();
                body_type
            }

            Expr::Lambda { params, body, span: _ } => {
                self.env.push_scope();
                for (name, ty) in params {
                    self.env.bind(name.clone(), ty.clone());
                }
                let ret = self.synthesize(body)?;
                self.env.pop_scope();

                Ok(Type::Function {
                    params: params.iter().map(|(_, ty)| ty.clone()).collect(),
                    return_type: Box::new(ret),
                })
            }

            Expr::Apply { func, args, span } => {
                let func_type = self.synthesize(func)?;
                let func_type = self.apply_subst(&func_type);
                self.check_application(&func_type, args, *span)
            }

            Expr::BinOp { op, lhs, rhs, span } => self.check_binop(*op, lhs, rhs, *span),

            Expr::Record { fields, span: _ } => {
                let mut field_types = Vec::new();
                for (name, expr) in fields {
                    let ty = self.synthesize(expr)?;
                    field_types.push((name.clone(), ty));
                }
                Ok(Type::Product {
                    fields: field_types,
                })
            }

            Expr::FieldAccess { expr, field, span } => {
                let expr_type = self.synthesize(expr)?;
                let expr_type = self.apply_subst(&expr_type);
                match &expr_type {
                    Type::Product { fields } => {
                        for (fname, ftype) in fields {
                            if fname == field {
                                return Ok(ftype.clone());
                            }
                        }
                        let err = TypeError::NoSuchField {
                            field: field.clone(),
                            span: *span,
                        };
                        self.errors.push(err.clone());
                        Err(err)
                    }
                    _ => {
                        let err = TypeError::NotARecord {
                            type_: expr_type,
                            span: *span,
                        };
                        self.errors.push(err.clone());
                        Err(err)
                    }
                }
            }

            Expr::WithUnit { expr, unit, span: _ } => {
                let inner = self.synthesize(expr)?;
                // Unit annotations require a numeric base type
                if !inner.is_numeric() {
                    let err = TypeError::Mismatch {
                        expected: Type::Float,
                        found: inner,
                        span: expr.span(),
                    };
                    self.errors.push(err.clone());
                    return Err(err);
                }
                Ok(Type::WithUnit {
                    base: Box::new(inner),
                    unit: unit.clone(),
                })
            }

            Expr::If {
                cond,
                then_branch,
                else_branch,
                span,
            } => {
                let cond_ty = self.synthesize(cond)?;
                if cond_ty != Type::Bool {
                    let err = TypeError::NonBoolCondition {
                        found: cond_ty,
                        span: cond.span(),
                    };
                    self.errors.push(err.clone());
                    return Err(err);
                }
                let then_ty = self.synthesize(then_branch)?;
                let else_ty = self.synthesize(else_branch)?;
                if self.types_equal(&then_ty, &else_ty) {
                    Ok(then_ty)
                } else {
                    let err = TypeError::BranchMismatch {
                        then_type: then_ty,
                        else_type: else_ty,
                        span: *span,
                    };
                    self.errors.push(err.clone());
                    Err(err)
                }
            }
        }
    }

    // ── Checking ──────────────────────────────────────────────────

    /// Check that an expression has the expected type.
    pub fn check(&mut self, expr: &Expr, expected: &Type) -> Result<(), TypeError> {
        let inferred = self.synthesize(expr)?;
        let inferred = self.apply_subst(&inferred);
        let expected = self.apply_subst(expected);

        if self.is_subtype(&inferred, &expected) {
            Ok(())
        } else {
            let err = TypeError::Mismatch {
                expected,
                found: inferred,
                span: expr.span(),
            };
            self.errors.push(err.clone());
            Err(err)
        }
    }

    // ── Subtyping ─────────────────────────────────────────────────

    /// Check if `sub` is a subtype of `sup`.
    ///
    /// Subtyping rules:
    /// - A type is a subtype of itself (reflexivity).
    /// - `Refinement { base: T, .. }` is a subtype of `T`.
    /// - `Int` is a subtype of `Float` (numeric widening).
    /// - Function types are contravariant in params, covariant in return.
    /// - `WithUnit` types with the same dimension are compatible.
    pub fn is_subtype(&self, sub: &Type, sup: &Type) -> bool {
        // Reflexivity
        if self.types_equal(sub, sup) {
            return true;
        }

        match (sub, sup) {
            // Int widens to Float
            (Type::Int, Type::Float) => true,

            // Refinement type is subtype of its base
            (Type::Refinement { base, .. }, sup) => self.is_subtype(base, sup),

            // A base type is NOT a subtype of its refinement (needs proof)
            // but a refinement with the same base can be subtype of another refinement
            // with the same base (if predicates imply — we conservatively reject)

            // Function subtyping (contravariant params, covariant return)
            (
                Type::Function { params: p1, return_type: r1 },
                Type::Function { params: p2, return_type: r2 },
            ) if p1.len() == p2.len() => {
                // Contravariant in parameters
                let params_ok = p1
                    .iter()
                    .zip(p2.iter())
                    .all(|(sub_p, sup_p)| self.is_subtype(sup_p, sub_p));
                // Covariant in return type
                params_ok && self.is_subtype(r1, r2)
            }

            // WithUnit: same dimension is compatible
            (
                Type::WithUnit { base: b1, unit: u1 },
                Type::WithUnit { base: b2, unit: u2 },
            ) => u1.same_dimension(u2) && self.is_subtype(b1, b2),

            // Stream covariance
            (Type::Stream(a), Type::Stream(b)) => self.is_subtype(a, b),

            // Distribution covariance
            (Type::Distribution(a), Type::Distribution(b)) => self.is_subtype(a, b),

            // Applied type: same constructor, check args
            (
                Type::Applied { constructor: c1, args: a1 },
                Type::Applied { constructor: c2, args: a2 },
            ) if c1 == c2 && a1.len() == a2.len() => {
                // Invariant for now (conservative)
                a1.iter().zip(a2.iter()).all(|(x, y)| self.types_equal(x, y))
            }

            // Product subtyping: sub has at least all fields of sup with compatible types
            (Type::Product { fields: sub_fields }, Type::Product { fields: sup_fields }) => {
                sup_fields.iter().all(|(name, sup_ty)| {
                    sub_fields
                        .iter()
                        .any(|(n, t)| n == name && self.is_subtype(t, sup_ty))
                })
            }

            _ => false,
        }
    }

    /// Structural type equality.
    pub fn types_equal(&self, a: &Type, b: &Type) -> bool {
        let a = self.apply_subst(a);
        let b = self.apply_subst(b);
        a == b
    }

    // ── Unification ───────────────────────────────────────────────

    /// Unify two types, binding type variables as needed.
    pub fn unify(&mut self, a: &Type, b: &Type) -> Result<Type, TypeError> {
        let a = self.apply_subst(a);
        let b = self.apply_subst(b);

        if a == b {
            return Ok(a);
        }

        match (&a, &b) {
            (Type::Var(id), _) => {
                self.substitution.insert(*id, b.clone());
                Ok(b)
            }
            (_, Type::Var(id)) => {
                self.substitution.insert(*id, a.clone());
                Ok(a)
            }
            (Type::Int, Type::Float) | (Type::Float, Type::Int) => Ok(Type::Float),
            (
                Type::Function { params: p1, return_type: r1 },
                Type::Function { params: p2, return_type: r2 },
            ) if p1.len() == p2.len() => {
                let params: Result<Vec<_>, _> = p1
                    .iter()
                    .zip(p2.iter())
                    .map(|(a, b)| self.unify(a, b))
                    .collect();
                let ret = self.unify(r1, r2)?;
                Ok(Type::Function {
                    params: params?,
                    return_type: Box::new(ret),
                })
            }
            (Type::Stream(a_inner), Type::Stream(b_inner)) => {
                let inner = self.unify(a_inner, b_inner)?;
                Ok(Type::Stream(Box::new(inner)))
            }
            (Type::Distribution(a_inner), Type::Distribution(b_inner)) => {
                let inner = self.unify(a_inner, b_inner)?;
                Ok(Type::Distribution(Box::new(inner)))
            }
            (
                Type::Applied { constructor: c1, args: a1 },
                Type::Applied { constructor: c2, args: a2 },
            ) if c1 == c2 && a1.len() == a2.len() => {
                let args: Result<Vec<_>, _> = a1
                    .iter()
                    .zip(a2.iter())
                    .map(|(a, b)| self.unify(a, b))
                    .collect();
                Ok(Type::Applied {
                    constructor: c1.clone(),
                    args: args?,
                })
            }
            _ => {
                let err = TypeError::UnificationFailure { a, b };
                self.errors.push(err.clone());
                Err(err)
            }
        }
    }

    // ── Helpers ───────────────────────────────────────────────────

    fn check_application(
        &mut self,
        func_type: &Type,
        args: &[Expr],
        span: Span,
    ) -> Result<Type, TypeError> {
        match func_type {
            Type::Function { params, return_type } => {
                if params.len() != args.len() {
                    let err = TypeError::ArityMismatch {
                        expected: params.len(),
                        found: args.len(),
                        span,
                    };
                    self.errors.push(err.clone());
                    return Err(err);
                }
                for (param_ty, arg) in params.iter().zip(args.iter()) {
                    self.check(arg, param_ty)?;
                }
                Ok(*return_type.clone())
            }
            Type::DependentFunction {
                param: _,
                param_type,
                return_type,
            } => {
                if args.len() != 1 {
                    let err = TypeError::ArityMismatch {
                        expected: 1,
                        found: args.len(),
                        span,
                    };
                    self.errors.push(err.clone());
                    return Err(err);
                }
                self.check(&args[0], param_type)?;
                // In a full implementation, we'd substitute the arg value
                // into the return type. For now, return as-is.
                Ok(*return_type.clone())
            }
            _ => {
                let err = TypeError::NotAFunction {
                    type_: func_type.clone(),
                    span,
                };
                self.errors.push(err.clone());
                Err(err)
            }
        }
    }

    fn check_binop(
        &mut self,
        op: BinOp,
        lhs: &Expr,
        rhs: &Expr,
        span: Span,
    ) -> Result<Type, TypeError> {
        // Pipeline: a |> f  ≡  f(a)
        if op == BinOp::Pipe {
            let rhs_type = self.synthesize(rhs)?;
            let rhs_type = self.apply_subst(&rhs_type);
            return self.check_application(&rhs_type, &[lhs.clone()], span);
        }

        let lhs_type = self.synthesize(lhs)?;
        let rhs_type = self.synthesize(rhs)?;
        let lhs_type = self.apply_subst(&lhs_type);
        let rhs_type = self.apply_subst(&rhs_type);

        if op.is_arithmetic() {
            return self.check_arithmetic(op, &lhs_type, &rhs_type, span);
        }

        if op.is_comparison() {
            // Both sides must be the same type (or compatible via subtyping)
            if self.is_subtype(&lhs_type, &rhs_type) || self.is_subtype(&rhs_type, &lhs_type) {
                return Ok(Type::Bool);
            }
            let err = TypeError::InvalidOperands {
                left: lhs_type,
                right: rhs_type,
                span,
            };
            self.errors.push(err.clone());
            return Err(err);
        }

        if op.is_logical() {
            if lhs_type != Type::Bool {
                let err = TypeError::Mismatch {
                    expected: Type::Bool,
                    found: lhs_type,
                    span: lhs.span(),
                };
                self.errors.push(err.clone());
                return Err(err);
            }
            if rhs_type != Type::Bool {
                let err = TypeError::Mismatch {
                    expected: Type::Bool,
                    found: rhs_type,
                    span: rhs.span(),
                };
                self.errors.push(err.clone());
                return Err(err);
            }
            return Ok(Type::Bool);
        }

        unreachable!("all BinOp variants covered")
    }

    fn check_arithmetic(
        &mut self,
        _op: BinOp,
        lhs: &Type,
        rhs: &Type,
        span: Span,
    ) -> Result<Type, TypeError> {
        match (lhs, rhs) {
            // Same numeric type
            (Type::Int, Type::Int) => Ok(Type::Int),
            (Type::Float, Type::Float) => Ok(Type::Float),
            // Int + Float → Float (widening)
            (Type::Int, Type::Float) | (Type::Float, Type::Int) => Ok(Type::Float),

            // WithUnit arithmetic: same dimension OK, different dimension error
            (
                Type::WithUnit { base: b1, unit: u1 },
                Type::WithUnit { base: b2, unit: u2 },
            ) => {
                if !u1.same_dimension(u2) {
                    let err = TypeError::UnitMismatch {
                        left: u1.clone(),
                        right: u2.clone(),
                        span,
                    };
                    self.errors.push(err.clone());
                    return Err(err);
                }
                // Result has the left unit (in practice, we'd convert)
                let base = if self.is_subtype(b1, b2) {
                    *b2.clone()
                } else {
                    *b1.clone()
                };
                Ok(Type::WithUnit {
                    base: Box::new(base),
                    unit: u1.clone(),
                })
            }

            _ => {
                let err = TypeError::InvalidOperands {
                    left: lhs.clone(),
                    right: rhs.clone(),
                    span,
                };
                self.errors.push(err.clone());
                Err(err)
            }
        }
    }
}

impl Default for TypeChecker {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::Span;
    use crate::types::*;

    fn s() -> Span {
        Span::dummy()
    }

    // ── Synthesis tests ───────────────────────────────────────────

    #[test]
    fn synthesize_literals() {
        let mut tc = TypeChecker::new();
        assert_eq!(
            tc.synthesize(&Expr::IntLit { value: 42, span: s() }).unwrap(),
            Type::Int
        );
        assert_eq!(
            tc.synthesize(&Expr::FloatLit { value: 3.14, span: s() }).unwrap(),
            Type::Float
        );
        assert_eq!(
            tc.synthesize(&Expr::StringLit { value: "hi".into(), span: s() }).unwrap(),
            Type::String
        );
        assert_eq!(
            tc.synthesize(&Expr::BoolLit { value: true, span: s() }).unwrap(),
            Type::Bool
        );
        assert_eq!(
            tc.synthesize(&Expr::UnitLit { span: s() }).unwrap(),
            Type::Unit
        );
    }

    #[test]
    fn synthesize_variable_lookup() {
        let mut tc = TypeChecker::new();
        tc.env.bind("x".into(), Type::Int);

        let result = tc.synthesize(&Expr::Var { name: "x".into(), span: s() });
        assert_eq!(result.unwrap(), Type::Int);
    }

    #[test]
    fn synthesize_unbound_variable() {
        let mut tc = TypeChecker::new();
        let result = tc.synthesize(&Expr::Var { name: "nope".into(), span: s() });
        assert!(result.is_err());
        assert!(!tc.is_ok());
    }

    #[test]
    fn synthesize_let_binding() {
        let mut tc = TypeChecker::new();

        // let x = 42 in x
        let expr = Expr::Let {
            name: "x".into(),
            annotation: None,
            value: Box::new(Expr::IntLit { value: 42, span: s() }),
            body: Box::new(Expr::Var { name: "x".into(), span: s() }),
            span: s(),
        };

        assert_eq!(tc.synthesize(&expr).unwrap(), Type::Int);
    }

    #[test]
    fn synthesize_let_with_annotation() {
        let mut tc = TypeChecker::new();

        // let x: Float = 42 in x   -- Int checks against Float via subtyping
        let expr = Expr::Let {
            name: "x".into(),
            annotation: Some(Type::Float),
            value: Box::new(Expr::IntLit { value: 42, span: s() }),
            body: Box::new(Expr::Var { name: "x".into(), span: s() }),
            span: s(),
        };

        assert_eq!(tc.synthesize(&expr).unwrap(), Type::Float);
    }

    #[test]
    fn synthesize_lambda() {
        let mut tc = TypeChecker::new();

        // fn(x: Int) -> x
        let expr = Expr::Lambda {
            params: vec![("x".into(), Type::Int)],
            body: Box::new(Expr::Var { name: "x".into(), span: s() }),
            span: s(),
        };

        let ty = tc.synthesize(&expr).unwrap();
        assert_eq!(
            ty,
            Type::Function {
                params: vec![Type::Int],
                return_type: Box::new(Type::Int),
            }
        );
    }

    #[test]
    fn synthesize_application() {
        let mut tc = TypeChecker::new();

        // Bind f: (Int) -> Bool
        tc.env.bind(
            "f".into(),
            Type::Function {
                params: vec![Type::Int],
                return_type: Box::new(Type::Bool),
            },
        );

        // f(42)
        let expr = Expr::Apply {
            func: Box::new(Expr::Var { name: "f".into(), span: s() }),
            args: vec![Expr::IntLit { value: 42, span: s() }],
            span: s(),
        };

        assert_eq!(tc.synthesize(&expr).unwrap(), Type::Bool);
    }

    #[test]
    fn synthesize_application_arity_mismatch() {
        let mut tc = TypeChecker::new();

        tc.env.bind(
            "f".into(),
            Type::Function {
                params: vec![Type::Int, Type::Int],
                return_type: Box::new(Type::Bool),
            },
        );

        // f(42) — needs 2 args
        let expr = Expr::Apply {
            func: Box::new(Expr::Var { name: "f".into(), span: s() }),
            args: vec![Expr::IntLit { value: 42, span: s() }],
            span: s(),
        };

        assert!(tc.synthesize(&expr).is_err());
    }

    // ── BinOp tests ───────────────────────────────────────────────

    #[test]
    fn arithmetic_int() {
        let mut tc = TypeChecker::new();
        let expr = Expr::BinOp {
            op: BinOp::Add,
            lhs: Box::new(Expr::IntLit { value: 1, span: s() }),
            rhs: Box::new(Expr::IntLit { value: 2, span: s() }),
            span: s(),
        };
        assert_eq!(tc.synthesize(&expr).unwrap(), Type::Int);
    }

    #[test]
    fn arithmetic_int_float_widening() {
        let mut tc = TypeChecker::new();
        let expr = Expr::BinOp {
            op: BinOp::Mul,
            lhs: Box::new(Expr::IntLit { value: 2, span: s() }),
            rhs: Box::new(Expr::FloatLit { value: 3.0, span: s() }),
            span: s(),
        };
        assert_eq!(tc.synthesize(&expr).unwrap(), Type::Float);
    }

    #[test]
    fn comparison_returns_bool() {
        let mut tc = TypeChecker::new();
        let expr = Expr::BinOp {
            op: BinOp::Lt,
            lhs: Box::new(Expr::IntLit { value: 1, span: s() }),
            rhs: Box::new(Expr::IntLit { value: 2, span: s() }),
            span: s(),
        };
        assert_eq!(tc.synthesize(&expr).unwrap(), Type::Bool);
    }

    #[test]
    fn logical_ops() {
        let mut tc = TypeChecker::new();
        let expr = Expr::BinOp {
            op: BinOp::And,
            lhs: Box::new(Expr::BoolLit { value: true, span: s() }),
            rhs: Box::new(Expr::BoolLit { value: false, span: s() }),
            span: s(),
        };
        assert_eq!(tc.synthesize(&expr).unwrap(), Type::Bool);
    }

    #[test]
    fn logical_op_non_bool_error() {
        let mut tc = TypeChecker::new();
        let expr = Expr::BinOp {
            op: BinOp::And,
            lhs: Box::new(Expr::IntLit { value: 1, span: s() }),
            rhs: Box::new(Expr::BoolLit { value: true, span: s() }),
            span: s(),
        };
        assert!(tc.synthesize(&expr).is_err());
    }

    // ── Pipeline tests ────────────────────────────────────────────

    #[test]
    fn pipeline_operator() {
        let mut tc = TypeChecker::new();

        // Bind inc: (Int) -> Int
        tc.env.bind(
            "inc".into(),
            Type::Function {
                params: vec![Type::Int],
                return_type: Box::new(Type::Int),
            },
        );

        // 42 |> inc
        let expr = Expr::BinOp {
            op: BinOp::Pipe,
            lhs: Box::new(Expr::IntLit { value: 42, span: s() }),
            rhs: Box::new(Expr::Var { name: "inc".into(), span: s() }),
            span: s(),
        };

        assert_eq!(tc.synthesize(&expr).unwrap(), Type::Int);
    }

    // ── Record tests ──────────────────────────────────────────────

    #[test]
    fn record_construction_and_access() {
        let mut tc = TypeChecker::new();

        // { x: 1, y: true }
        let record = Expr::Record {
            fields: vec![
                ("x".into(), Expr::IntLit { value: 1, span: s() }),
                ("y".into(), Expr::BoolLit { value: true, span: s() }),
            ],
            span: s(),
        };

        let ty = tc.synthesize(&record).unwrap();
        assert_eq!(
            ty,
            Type::Product {
                fields: vec![("x".into(), Type::Int), ("y".into(), Type::Bool)]
            }
        );
    }

    #[test]
    fn field_access() {
        let mut tc = TypeChecker::new();
        tc.env.bind(
            "r".into(),
            Type::Product {
                fields: vec![("x".into(), Type::Int), ("y".into(), Type::Bool)],
            },
        );

        let expr = Expr::FieldAccess {
            expr: Box::new(Expr::Var { name: "r".into(), span: s() }),
            field: "y".into(),
            span: s(),
        };

        assert_eq!(tc.synthesize(&expr).unwrap(), Type::Bool);
    }

    #[test]
    fn field_access_no_such_field() {
        let mut tc = TypeChecker::new();
        tc.env.bind(
            "r".into(),
            Type::Product {
                fields: vec![("x".into(), Type::Int)],
            },
        );

        let expr = Expr::FieldAccess {
            expr: Box::new(Expr::Var { name: "r".into(), span: s() }),
            field: "z".into(),
            span: s(),
        };

        assert!(tc.synthesize(&expr).is_err());
    }

    // ── Unit type tests ───────────────────────────────────────────

    #[test]
    fn unit_literal_synthesis() {
        let mut tc = TypeChecker::new();

        // 200.ms → Float.Duration(Milliseconds)
        let expr = Expr::WithUnit {
            expr: Box::new(Expr::FloatLit { value: 200.0, span: s() }),
            unit: PhysicalUnit::Duration(DurationUnit::Milliseconds),
            span: s(),
        };

        let ty = tc.synthesize(&expr).unwrap();
        assert_eq!(
            ty,
            Type::WithUnit {
                base: Box::new(Type::Float),
                unit: PhysicalUnit::Duration(DurationUnit::Milliseconds),
            }
        );
    }

    #[test]
    fn unit_same_dimension_addition() {
        let mut tc = TypeChecker::new();

        // 1.s + 200.ms  → OK (same dimension: Duration)
        let expr = Expr::BinOp {
            op: BinOp::Add,
            lhs: Box::new(Expr::WithUnit {
                expr: Box::new(Expr::FloatLit { value: 1.0, span: s() }),
                unit: PhysicalUnit::Duration(DurationUnit::Seconds),
                span: s(),
            }),
            rhs: Box::new(Expr::WithUnit {
                expr: Box::new(Expr::FloatLit { value: 200.0, span: s() }),
                unit: PhysicalUnit::Duration(DurationUnit::Milliseconds),
                span: s(),
            }),
            span: s(),
        };

        assert!(tc.synthesize(&expr).is_ok());
    }

    #[test]
    fn unit_different_dimension_error() {
        let mut tc = TypeChecker::new();

        // 1.s + 1.GiB  → Error (Duration + Size)
        let expr = Expr::BinOp {
            op: BinOp::Add,
            lhs: Box::new(Expr::WithUnit {
                expr: Box::new(Expr::FloatLit { value: 1.0, span: s() }),
                unit: PhysicalUnit::Duration(DurationUnit::Seconds),
                span: s(),
            }),
            rhs: Box::new(Expr::WithUnit {
                expr: Box::new(Expr::FloatLit { value: 1.0, span: s() }),
                unit: PhysicalUnit::Size(SizeUnit::GiB),
                span: s(),
            }),
            span: s(),
        };

        assert!(tc.synthesize(&expr).is_err());
    }

    // ── Subtyping tests ───────────────────────────────────────────

    #[test]
    fn subtype_reflexivity() {
        let tc = TypeChecker::new();
        assert!(tc.is_subtype(&Type::Int, &Type::Int));
        assert!(tc.is_subtype(&Type::String, &Type::String));
    }

    #[test]
    fn subtype_int_float() {
        let tc = TypeChecker::new();
        assert!(tc.is_subtype(&Type::Int, &Type::Float));
        assert!(!tc.is_subtype(&Type::Float, &Type::Int));
    }

    #[test]
    fn subtype_refinement_to_base() {
        let tc = TypeChecker::new();

        // Nat (refinement of Int) is subtype of Int
        let nat = Type::Refinement {
            var: "n".into(),
            base: Box::new(Type::Int),
            predicate: Predicate::Comparison {
                left: Box::new(PredicateExpr::Var("n".into())),
                op: ComparisonOp::Ge,
                right: Box::new(PredicateExpr::IntLit(0)),
            },
        };

        assert!(tc.is_subtype(&nat, &Type::Int));
        // Nat → Float also works via transitivity through Int
        assert!(tc.is_subtype(&nat, &Type::Float));
        // Int is NOT a subtype of Nat
        assert!(!tc.is_subtype(&Type::Int, &nat));
    }

    #[test]
    fn subtype_product_width() {
        let tc = TypeChecker::new();

        // { x: Int, y: Bool } is subtype of { x: Int }
        let sub = Type::Product {
            fields: vec![("x".into(), Type::Int), ("y".into(), Type::Bool)],
        };
        let sup = Type::Product {
            fields: vec![("x".into(), Type::Int)],
        };

        assert!(tc.is_subtype(&sub, &sup));
        assert!(!tc.is_subtype(&sup, &sub));
    }

    // ── Unification tests ─────────────────────────────────────────

    #[test]
    fn unify_same_types() {
        let mut tc = TypeChecker::new();
        assert_eq!(tc.unify(&Type::Int, &Type::Int).unwrap(), Type::Int);
    }

    #[test]
    fn unify_type_variable() {
        let mut tc = TypeChecker::new();
        let var = tc.fresh_var();

        let result = tc.unify(&var, &Type::String).unwrap();
        assert_eq!(result, Type::String);

        // The variable should now be resolved
        assert_eq!(tc.apply_subst(&var), Type::String);
    }

    #[test]
    fn unify_int_float() {
        let mut tc = TypeChecker::new();
        let result = tc.unify(&Type::Int, &Type::Float).unwrap();
        assert_eq!(result, Type::Float);
    }

    #[test]
    fn unify_incompatible() {
        let mut tc = TypeChecker::new();
        assert!(tc.unify(&Type::String, &Type::Bool).is_err());
    }

    // ── If expression tests ───────────────────────────────────────

    #[test]
    fn if_expression_same_branches() {
        let mut tc = TypeChecker::new();
        let expr = Expr::If {
            cond: Box::new(Expr::BoolLit { value: true, span: s() }),
            then_branch: Box::new(Expr::IntLit { value: 1, span: s() }),
            else_branch: Box::new(Expr::IntLit { value: 2, span: s() }),
            span: s(),
        };
        assert_eq!(tc.synthesize(&expr).unwrap(), Type::Int);
    }

    #[test]
    fn if_expression_non_bool_cond() {
        let mut tc = TypeChecker::new();
        let expr = Expr::If {
            cond: Box::new(Expr::IntLit { value: 1, span: s() }),
            then_branch: Box::new(Expr::IntLit { value: 1, span: s() }),
            else_branch: Box::new(Expr::IntLit { value: 2, span: s() }),
            span: s(),
        };
        assert!(tc.synthesize(&expr).is_err());
    }

    #[test]
    fn if_expression_branch_mismatch() {
        let mut tc = TypeChecker::new();
        let expr = Expr::If {
            cond: Box::new(Expr::BoolLit { value: true, span: s() }),
            then_branch: Box::new(Expr::IntLit { value: 1, span: s() }),
            else_branch: Box::new(Expr::StringLit { value: "no".into(), span: s() }),
            span: s(),
        };
        assert!(tc.synthesize(&expr).is_err());
    }

    // ── Scoping tests ─────────────────────────────────────────────

    #[test]
    fn let_scoping() {
        let mut tc = TypeChecker::new();
        tc.env.bind("x".into(), Type::Bool);

        // let x = 42 in x  → Int (shadows outer x: Bool)
        let expr = Expr::Let {
            name: "x".into(),
            annotation: None,
            value: Box::new(Expr::IntLit { value: 42, span: s() }),
            body: Box::new(Expr::Var { name: "x".into(), span: s() }),
            span: s(),
        };

        assert_eq!(tc.synthesize(&expr).unwrap(), Type::Int);

        // After the let, x is Bool again
        assert_eq!(tc.env.lookup("x"), Some(&Type::Bool));
    }

    // ── Check mode tests ──────────────────────────────────────────

    #[test]
    fn check_subtype_accepted() {
        let mut tc = TypeChecker::new();

        // Check that an Int literal is acceptable where Float is expected
        let expr = Expr::IntLit { value: 42, span: s() };
        assert!(tc.check(&expr, &Type::Float).is_ok());
    }

    #[test]
    fn check_mismatch_rejected() {
        let mut tc = TypeChecker::new();

        // Check that a String literal is NOT acceptable where Int is expected
        let expr = Expr::StringLit { value: "hi".into(), span: s() };
        assert!(tc.check(&expr, &Type::Int).is_err());
    }
}
