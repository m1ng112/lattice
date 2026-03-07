//! Bidirectional type checker for Lattice programs.
//!
//! Operates directly on the parser AST (`lattice_parser::ast`),
//! eliminating the need for a separate type-checker AST.

use std::collections::HashMap;

use lattice_parser::ast::{self, Spanned};

use crate::environment::TypeEnv;
use crate::types::{PhysicalUnit, Type, TypeVarId};

/// A local Span alias for error reporting, derived from the parser span.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

impl Span {
    pub fn dummy() -> Self {
        Self { start: 0, end: 0 }
    }
}

impl std::fmt::Display for Span {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}..{}", self.start, self.end)
    }
}

impl From<&ast::Span> for Span {
    fn from(s: &ast::Span) -> Self {
        Self {
            start: s.start,
            end: s.end,
        }
    }
}

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

    #[error("non-exhaustive match at {span}: missing {missing}")]
    NonExhaustiveMatch { missing: String, span: Span },
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
            Type::Array(inner) => Type::Array(Box::new(self.apply_subst(inner))),
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
            Type::Refinement {
                var,
                base,
                predicate,
            } => Type::Refinement {
                var: var.clone(),
                base: Box::new(self.apply_subst(base)),
                predicate: predicate.clone(),
            },
            Type::DependentFunction {
                param,
                param_type,
                return_type,
            } => Type::DependentFunction {
                param: param.clone(),
                param_type: Box::new(self.apply_subst(param_type)),
                return_type: Box::new(self.apply_subst(return_type)),
            },
            _ => ty.clone(),
        }
    }

    // ── Synthesis (inference) ──────────────────────────────────────

    /// Synthesize (infer) the type of an expression.
    pub fn synthesize(&mut self, expr: &Spanned<ast::Expr>) -> Result<Type, TypeError> {
        let span = Span::from(&expr.span);
        match &expr.node {
            ast::Expr::IntLit(_) => Ok(Type::Int),
            ast::Expr::FloatLit(_) => Ok(Type::Float),
            ast::Expr::StringLit(_) => Ok(Type::String),
            ast::Expr::BoolLit(_) => Ok(Type::Bool),

            ast::Expr::Ident(name) => {
                self.env.lookup(name).cloned().ok_or_else(|| {
                    let err = TypeError::UnboundVariable {
                        name: name.clone(),
                        span: span.clone(),
                    };
                    self.errors.push(err.clone());
                    err
                })
            }

            ast::Expr::Let {
                name,
                type_ann,
                value,
            } => {
                let val_type = if let Some(ann) = type_ann {
                    let ann_ty = convert_type_expr(&ann.node);
                    self.check(value, &ann_ty)?;
                    ann_ty
                } else {
                    self.synthesize(value)?
                };
                self.env.bind(name.clone(), val_type);
                Ok(Type::Unit)
            }

            ast::Expr::Lambda { params, body } => {
                self.env.push_scope();
                let param_types: Vec<Type> = params
                    .iter()
                    .map(|p| {
                        let ty = convert_type_expr(&p.type_expr.node);
                        self.env.bind(p.name.clone(), ty.clone());
                        ty
                    })
                    .collect();
                let ret = self.synthesize(body)?;
                self.env.pop_scope();
                Ok(Type::Function {
                    params: param_types,
                    return_type: Box::new(ret),
                })
            }

            ast::Expr::Call { func, args } => {
                let func_type = self.synthesize(func)?;
                let func_type = self.apply_subst(&func_type);
                self.check_application(&func_type, args, span)
            }

            ast::Expr::CallNamed { func, args } => {
                // Treat named args as positional for type checking
                let func_type = self.synthesize(func)?;
                let func_type = self.apply_subst(&func_type);
                let positional: Vec<Spanned<ast::Expr>> =
                    args.iter().map(|(_, e)| e.clone()).collect();
                self.check_application(&func_type, &positional, span)
            }

            ast::Expr::BinOp { left, op, right } => {
                self.check_binop(*op, left, right, span)
            }

            ast::Expr::Pipeline { left, right } => {
                let rhs_type = self.synthesize(right)?;
                let rhs_type = self.apply_subst(&rhs_type);
                self.check_application(&rhs_type, std::slice::from_ref(left.as_ref()), span)
            }

            ast::Expr::Record(fields) => {
                let mut field_types = Vec::new();
                for (name, val) in fields {
                    let ty = self.synthesize(val)?;
                    field_types.push((name.clone(), ty));
                }
                Ok(Type::Product {
                    fields: field_types,
                })
            }

            ast::Expr::Field { expr: inner, name } => {
                let expr_type = self.synthesize(inner)?;
                let expr_type = self.apply_subst(&expr_type);
                match &expr_type {
                    Type::Product { fields } => {
                        for (fname, ftype) in fields {
                            if fname == name {
                                return Ok(ftype.clone());
                            }
                        }
                        let err = TypeError::NoSuchField {
                            field: name.clone(),
                            span,
                        };
                        self.errors.push(err.clone());
                        Err(err)
                    }
                    _ => {
                        let err = TypeError::NotARecord {
                            type_: expr_type,
                            span,
                        };
                        self.errors.push(err.clone());
                        Err(err)
                    }
                }
            }

            ast::Expr::WithUnit {
                value: inner,
                unit,
            } => {
                let inner_ty = self.synthesize(inner)?;
                if !inner_ty.is_numeric() {
                    let err = TypeError::Mismatch {
                        expected: Type::Float,
                        found: inner_ty,
                        span: Span::from(&inner.span),
                    };
                    self.errors.push(err.clone());
                    return Err(err);
                }
                let pu = parse_physical_unit(unit);
                Ok(Type::WithUnit {
                    base: Box::new(inner_ty),
                    unit: pu,
                })
            }

            ast::Expr::If {
                cond,
                then_,
                else_,
            } => {
                let cond_ty = self.synthesize(cond)?;
                if cond_ty != Type::Bool {
                    let err = TypeError::NonBoolCondition {
                        found: cond_ty,
                        span: Span::from(&cond.span),
                    };
                    self.errors.push(err.clone());
                    return Err(err);
                }
                let then_ty = self.synthesize(then_)?;
                if let Some(else_expr) = else_ {
                    let else_ty = self.synthesize(else_expr)?;
                    if self.types_equal(&then_ty, &else_ty) {
                        Ok(then_ty)
                    } else {
                        let err = TypeError::BranchMismatch {
                            then_type: then_ty,
                            else_type: else_ty,
                            span,
                        };
                        self.errors.push(err.clone());
                        Err(err)
                    }
                } else {
                    // No else branch → Unit
                    Ok(Type::Unit)
                }
            }

            ast::Expr::Array(elements) => {
                if elements.is_empty() {
                    let elem_ty = self.fresh_var();
                    Ok(Type::Array(Box::new(elem_ty)))
                } else {
                    let first_ty = self.synthesize(&elements[0])?;
                    for elem in &elements[1..] {
                        let elem_ty = self.synthesize(elem)?;
                        if !self.types_equal(&first_ty, &elem_ty)
                            && !self.is_subtype(&elem_ty, &first_ty)
                        {
                            let err = TypeError::Mismatch {
                                expected: first_ty.clone(),
                                found: elem_ty,
                                span: span.clone(),
                            };
                            self.errors.push(err.clone());
                            return Err(err);
                        }
                    }
                    Ok(Type::Array(Box::new(first_ty)))
                }
            }

            ast::Expr::Index {
                expr: arr,
                index,
            } => {
                let arr_ty = self.synthesize(arr)?;
                let arr_ty = self.apply_subst(&arr_ty);
                let idx_ty = self.synthesize(index)?;
                if idx_ty != Type::Int {
                    let err = TypeError::Mismatch {
                        expected: Type::Int,
                        found: idx_ty,
                        span: Span::from(&index.span),
                    };
                    self.errors.push(err.clone());
                    return Err(err);
                }
                match arr_ty {
                    Type::Array(elem) => Ok(*elem),
                    _ => {
                        let err = TypeError::Mismatch {
                            expected: Type::Array(Box::new(self.fresh_var())),
                            found: arr_ty,
                            span,
                        };
                        self.errors.push(err.clone());
                        Err(err)
                    }
                }
            }

            ast::Expr::Match { expr: scrutinee, arms } => {
                let scrutinee_ty = self.synthesize(scrutinee)?;
                let scrutinee_ty = self.apply_subst(&scrutinee_ty);
                if arms.is_empty() {
                    return Ok(Type::Unit);
                }

                self.check_exhaustiveness(arms, &scrutinee_ty, span.clone());

                let first_ty = self.synthesize_match_arm(&arms[0])?;
                for arm in &arms[1..] {
                    let arm_ty = self.synthesize_match_arm(arm)?;
                    if !self.types_equal(&first_ty, &arm_ty)
                        && !self.is_subtype(&arm_ty, &first_ty)
                    {
                        let err = TypeError::BranchMismatch {
                            then_type: first_ty.clone(),
                            else_type: arm_ty,
                            span: span.clone(),
                        };
                        self.errors.push(err.clone());
                        return Err(err);
                    }
                }
                Ok(first_ty)
            }

            ast::Expr::Block(exprs) => {
                if exprs.is_empty() {
                    return Ok(Type::Unit);
                }
                let mut last_ty = Type::Unit;
                for e in exprs {
                    last_ty = self.synthesize(e)?;
                }
                Ok(last_ty)
            }

            ast::Expr::UnaryOp { op, operand } => {
                let operand_ty = self.synthesize(operand)?;
                match op {
                    ast::UnaryOp::Neg => {
                        if matches!(operand_ty, Type::Int | Type::Float) {
                            Ok(operand_ty)
                        } else {
                            let err = TypeError::Mismatch {
                                expected: Type::Int,
                                found: operand_ty,
                                span,
                            };
                            self.errors.push(err.clone());
                            Err(err)
                        }
                    }
                    ast::UnaryOp::Not => {
                        if operand_ty == Type::Bool {
                            Ok(Type::Bool)
                        } else {
                            let err = TypeError::Mismatch {
                                expected: Type::Bool,
                                found: operand_ty,
                                span,
                            };
                            self.errors.push(err.clone());
                            Err(err)
                        }
                    }
                }
            }

            ast::Expr::Ascription { expr: inner, type_expr } => {
                let ty = convert_type_expr(&type_expr.node);
                self.check(inner, &ty).map_err(|_| {
                    let actual = self.synthesize(inner).unwrap_or(Type::Unit);
                    let err = TypeError::Mismatch {
                        expected: ty.clone(),
                        found: actual,
                        span: span.clone(),
                    };
                    self.errors.push(err.clone());
                    err
                })?;
                Ok(ty)
            }

            ast::Expr::DoBlock(stmts) => {
                self.env.push_scope();
                let mut last_ty = Type::Unit;
                for stmt in stmts {
                    match &stmt.node {
                        ast::DoStatement::Let { name, expr: e } => {
                            let ty = self.synthesize(e)?;
                            self.env.bind(name.clone(), ty);
                            last_ty = Type::Unit;
                        }
                        ast::DoStatement::Bind { name, expr: e } => {
                            let ty = self.synthesize(e)?;
                            self.env.bind(name.clone(), ty);
                            last_ty = Type::Unit;
                        }
                        ast::DoStatement::Expr(e) => {
                            last_ty = self.synthesize(e)?;
                        }
                        ast::DoStatement::Yield(e) => {
                            last_ty = self.synthesize(e)?;
                        }
                    }
                }
                self.env.pop_scope();
                Ok(last_ty)
            }

            ast::Expr::Range { start, end } => {
                let start_ty = self.synthesize(start)?;
                let end_ty = self.synthesize(end)?;
                if start_ty != Type::Int {
                    let err = TypeError::Mismatch {
                        expected: Type::Int,
                        found: start_ty,
                        span: span.clone(),
                    };
                    self.errors.push(err.clone());
                    return Err(err);
                }
                if end_ty != Type::Int {
                    let err = TypeError::Mismatch {
                        expected: Type::Int,
                        found: end_ty,
                        span,
                    };
                    self.errors.push(err.clone());
                    return Err(err);
                }
                Ok(Type::Array(Box::new(Type::Int)))
            }

            ast::Expr::Slice {
                expr: arr,
                start: _,
                end: _,
            } => {
                let arr_ty = self.synthesize(arr)?;
                match arr_ty {
                    Type::Array(elem) => Ok(Type::Array(elem)),
                    _ => {
                        let err = TypeError::Mismatch {
                            expected: Type::Array(Box::new(Type::Unit)),
                            found: arr_ty,
                            span,
                        };
                        self.errors.push(err.clone());
                        Err(err)
                    }
                }
            }

            // Relational algebra: all return Array of records
            ast::Expr::Select { relation, .. }
            | ast::Expr::Project { relation, .. }
            | ast::Expr::GroupBy { relation, .. } => {
                let rel_ty = self.synthesize(relation)?;
                Ok(rel_ty) // preserve array type
            }

            ast::Expr::Join { left, .. } => {
                let left_ty = self.synthesize(left)?;
                Ok(left_ty) // preserve array type (simplified)
            }

            // Quantifiers return Bool
            ast::Expr::ForAll { body, var, domain, .. } => {
                self.env.push_scope();
                let _dom_ty = self.synthesize(domain)?;
                let fresh = self.fresh_var();
                self.env.bind(var.clone(), fresh);
                let _body_ty = self.synthesize(body)?;
                self.env.pop_scope();
                Ok(Type::Bool)
            }

            ast::Expr::Exists { body, var, domain, .. } => {
                self.env.push_scope();
                let _dom_ty = self.synthesize(domain)?;
                let fresh = self.fresh_var();
                self.env.bind(var.clone(), fresh);
                let _body_ty = self.synthesize(body)?;
                self.env.pop_scope();
                Ok(Type::Bool)
            }

            // Try: unwrap result type (simplified: return inner type)
            ast::Expr::Try(inner) => self.synthesize(inner),

            // Yield: same as inner
            ast::Expr::Yield(inner) => self.synthesize(inner),

            // Branch: probabilistic — return type of first arm
            ast::Expr::Branch { arms, .. } => {
                if arms.is_empty() {
                    Ok(Type::Unit)
                } else {
                    self.synthesize(&arms[0].body)
                }
            }

            // Synthesize block: opaque
            ast::Expr::Synthesize(_) => Ok(self.fresh_var()),
        }
    }

    /// Check whether a match expression is exhaustive.
    fn check_exhaustiveness(
        &mut self,
        arms: &[ast::MatchArm],
        scrutinee_ty: &Type,
        span: Span,
    ) {
        let has_catch_all = arms.iter().any(|arm| {
            matches!(
                arm.pattern.node,
                ast::Pattern::Wildcard | ast::Pattern::Ident(_)
            )
        });
        if has_catch_all {
            return;
        }

        let variants = match scrutinee_ty {
            Type::Sum { variants } => Some(variants.clone()),
            Type::Named(name) => self.env.lookup_type(name).and_then(|td| {
                if let Type::Sum { variants } = &td.body {
                    Some(variants.clone())
                } else {
                    None
                }
            }),
            _ => None,
        };

        if let Some(variants) = variants {
            let matched_constructors: std::collections::HashSet<&str> = arms
                .iter()
                .filter_map(|arm| {
                    if let ast::Pattern::Constructor(name, _) = &arm.pattern.node {
                        Some(name.as_str())
                    } else {
                        None
                    }
                })
                .collect();

            let missing: Vec<&str> = variants
                .iter()
                .filter(|v| !matched_constructors.contains(v.name.as_str()))
                .map(|v| v.name.as_str())
                .collect();

            if !missing.is_empty() {
                let err = TypeError::NonExhaustiveMatch {
                    missing: missing.join(", "),
                    span,
                };
                self.errors.push(err);
            }
        }
    }

    /// Synthesize the type of a match arm body, binding pattern variables.
    fn synthesize_match_arm(&mut self, arm: &ast::MatchArm) -> Result<Type, TypeError> {
        self.env.push_scope();
        self.bind_pattern_vars(&arm.pattern.node);
        let ty = self.synthesize(&arm.body);
        self.env.pop_scope();
        ty
    }

    /// Bind variables introduced by a pattern.
    fn bind_pattern_vars(&mut self, pattern: &ast::Pattern) {
        match pattern {
            ast::Pattern::Ident(name) => {
                let var = self.fresh_var();
                self.env.bind(name.clone(), var);
            }
            ast::Pattern::Constructor(_, sub_pats) => {
                for p in sub_pats {
                    self.bind_pattern_vars(&p.node);
                }
            }
            ast::Pattern::Record(fields) => {
                for (_, p) in fields {
                    self.bind_pattern_vars(&p.node);
                }
            }
            ast::Pattern::Wildcard | ast::Pattern::Literal(_) => {}
        }
    }

    // ── Checking ──────────────────────────────────────────────────

    /// Check that an expression has the expected type.
    pub fn check(
        &mut self,
        expr: &Spanned<ast::Expr>,
        expected: &Type,
    ) -> Result<(), TypeError> {
        let inferred = self.synthesize(expr)?;
        let inferred = self.apply_subst(&inferred);
        let expected = self.apply_subst(expected);

        if self.is_subtype(&inferred, &expected) {
            Ok(())
        } else {
            let err = TypeError::Mismatch {
                expected,
                found: inferred,
                span: Span::from(&expr.span),
            };
            self.errors.push(err.clone());
            Err(err)
        }
    }

    // ── Subtyping ─────────────────────────────────────────────────

    /// Check if `sub` is a subtype of `sup`.
    pub fn is_subtype(&self, sub: &Type, sup: &Type) -> bool {
        if self.types_equal(sub, sup) {
            return true;
        }

        match (sub, sup) {
            (Type::Int, Type::Float) => true,
            (Type::Refinement { base, .. }, sup) => self.is_subtype(base, sup),
            (
                Type::Function {
                    params: p1,
                    return_type: r1,
                },
                Type::Function {
                    params: p2,
                    return_type: r2,
                },
            ) if p1.len() == p2.len() => {
                let params_ok = p1
                    .iter()
                    .zip(p2.iter())
                    .all(|(sub_p, sup_p)| self.is_subtype(sup_p, sub_p));
                params_ok && self.is_subtype(r1, r2)
            }
            (
                Type::WithUnit {
                    base: b1, unit: u1, ..
                },
                Type::WithUnit {
                    base: b2, unit: u2, ..
                },
            ) => u1.same_dimension(u2) && self.is_subtype(b1, b2),
            (Type::Array(a), Type::Array(b)) => self.is_subtype(a, b),
            (Type::Stream(a), Type::Stream(b)) => self.is_subtype(a, b),
            (Type::Distribution(a), Type::Distribution(b)) => self.is_subtype(a, b),
            (
                Type::Applied {
                    constructor: c1,
                    args: a1,
                },
                Type::Applied {
                    constructor: c2,
                    args: a2,
                },
            ) if c1 == c2 && a1.len() == a2.len() => {
                a1.iter().zip(a2.iter()).all(|(x, y)| self.types_equal(x, y))
            }
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
                Type::Function {
                    params: p1,
                    return_type: r1,
                },
                Type::Function {
                    params: p2,
                    return_type: r2,
                },
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
            (Type::Array(a_inner), Type::Array(b_inner)) => {
                let inner = self.unify(a_inner, b_inner)?;
                Ok(Type::Array(Box::new(inner)))
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
                Type::Applied {
                    constructor: c1,
                    args: a1,
                },
                Type::Applied {
                    constructor: c2,
                    args: a2,
                },
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
        args: &[Spanned<ast::Expr>],
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
        op: ast::BinOp,
        lhs: &Spanned<ast::Expr>,
        rhs: &Spanned<ast::Expr>,
        span: Span,
    ) -> Result<Type, TypeError> {
        let lhs_type = self.synthesize(lhs)?;
        let rhs_type = self.synthesize(rhs)?;
        let lhs_type = self.apply_subst(&lhs_type);
        let rhs_type = self.apply_subst(&rhs_type);

        if is_arithmetic(op) {
            return self.check_arithmetic(op, &lhs_type, &rhs_type, span);
        }

        if is_comparison(op) {
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

        if is_logical(op) {
            if lhs_type != Type::Bool {
                let err = TypeError::Mismatch {
                    expected: Type::Bool,
                    found: lhs_type,
                    span: Span::from(&lhs.span),
                };
                self.errors.push(err.clone());
                return Err(err);
            }
            if rhs_type != Type::Bool {
                let err = TypeError::Mismatch {
                    expected: Type::Bool,
                    found: rhs_type,
                    span: Span::from(&rhs.span),
                };
                self.errors.push(err.clone());
                return Err(err);
            }
            return Ok(Type::Bool);
        }

        if op == ast::BinOp::Concat {
            if lhs_type != Type::String {
                let err = TypeError::Mismatch {
                    expected: Type::String,
                    found: lhs_type,
                    span: Span::from(&lhs.span),
                };
                self.errors.push(err.clone());
                return Err(err);
            }
            if rhs_type != Type::String {
                let err = TypeError::Mismatch {
                    expected: Type::String,
                    found: rhs_type,
                    span: Span::from(&rhs.span),
                };
                self.errors.push(err.clone());
                return Err(err);
            }
            return Ok(Type::String);
        }

        unreachable!("all BinOp variants covered")
    }

    fn check_arithmetic(
        &mut self,
        _op: ast::BinOp,
        lhs: &Type,
        rhs: &Type,
        span: Span,
    ) -> Result<Type, TypeError> {
        match (lhs, rhs) {
            (Type::Int, Type::Int) => Ok(Type::Int),
            (Type::Float, Type::Float) => Ok(Type::Float),
            (Type::Int, Type::Float) | (Type::Float, Type::Int) => Ok(Type::Float),
            (
                Type::WithUnit {
                    base: b1, unit: u1, ..
                },
                Type::WithUnit {
                    base: b2, unit: u2, ..
                },
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

// ── Dependent type bridge ─────────────────────────────────────────

use crate::dependent::{Term, Universe};
use crate::dependent_checker::DependentChecker;

impl TypeChecker {
    /// Attempt dependent type verification for a dependent type expression.
    ///
    /// Converts `TypeExpr::Dependent { name, params }` (e.g. `Tensor(3, 224, 224)`)
    /// into a dependent `Term` and verifies it using the dependent checker.
    /// Returns `Ok(Type::Applied { ... })` if valid.
    pub fn check_dependent_type(
        &mut self,
        name: &str,
        params: &[(String, Spanned<ast::TypeExpr>)],
        span: Span,
    ) -> Result<Type, TypeError> {
        let mut dc = DependentChecker::new();

        // Register the type constructor as living in Type₀
        let constructor_type = build_constructor_type(name, params);
        dc.define(
            name.to_string(),
            constructor_type.clone(),
            Term::Universe(Universe::base()),
        );

        // Build the applied term: Name(arg1, arg2, ...)
        let mut applied = Term::Var(name.to_string());
        for (param_name, param_te) in params {
            let arg = type_expr_to_term(&param_te.node, param_name);
            applied = Term::App {
                func: Box::new(applied),
                arg: Box::new(arg),
            };
        }

        // Attempt to infer the type of the applied term
        match dc.infer(&applied) {
            Ok(_inferred) => {
                // Valid dependent type application
                let args = params
                    .iter()
                    .map(|(_, te)| convert_type_expr(&te.node))
                    .collect();
                Ok(Type::Applied {
                    constructor: name.to_string(),
                    args,
                })
            }
            Err(e) => {
                let err = TypeError::Mismatch {
                    expected: Type::Named(name.to_string()),
                    found: Type::Named(format!("dependent type error: {e}")),
                    span,
                };
                self.errors.push(err.clone());
                Err(err)
            }
        }
    }
}

/// Build a Pi-type for a dependent type constructor.
/// E.g., `Tensor(n: Nat, m: Nat)` → `Π(n: Nat). Π(m: Nat). Type₀`
fn build_constructor_type(
    _name: &str,
    params: &[(String, Spanned<ast::TypeExpr>)],
) -> Term {
    let mut result = Term::Universe(Universe::base());
    for (param_name, param_te) in params.iter().rev() {
        let param_type = type_expr_to_term(&param_te.node, param_name);
        result = Term::Pi {
            param: param_name.clone(),
            param_type: Box::new(param_type),
            body: Box::new(result),
        };
    }
    result
}

/// Convert a parser TypeExpr to a dependent Term.
fn type_expr_to_term(te: &ast::TypeExpr, _hint_name: &str) -> Term {
    match te {
        ast::TypeExpr::Named(name) => match name.as_str() {
            "Int" | "Nat" => Term::Universe(Universe::base()),
            _ => Term::Var(name.clone()),
        },
        _ => Term::Universe(Universe::base()),
    }
}

// ── Free functions ────────────────────────────────────────────────

fn is_arithmetic(op: ast::BinOp) -> bool {
    matches!(
        op,
        ast::BinOp::Add | ast::BinOp::Sub | ast::BinOp::Mul | ast::BinOp::Div | ast::BinOp::Mod
    )
}

fn is_comparison(op: ast::BinOp) -> bool {
    matches!(
        op,
        ast::BinOp::Eq
            | ast::BinOp::Neq
            | ast::BinOp::Lt
            | ast::BinOp::Gt
            | ast::BinOp::Leq
            | ast::BinOp::Geq
    )
}

fn is_logical(op: ast::BinOp) -> bool {
    matches!(op, ast::BinOp::And | ast::BinOp::Or | ast::BinOp::Implies)
}

/// Convert a parser `TypeExpr` to our internal `Type`.
pub fn convert_type_expr(type_expr: &ast::TypeExpr) -> Type {
    match type_expr {
        ast::TypeExpr::Named(name) => match name.as_str() {
            "Int" => Type::Int,
            "Float" => Type::Float,
            "String" => Type::String,
            "Bool" => Type::Bool,
            "Unit" => Type::Unit,
            _ => Type::Named(name.clone()),
        },
        ast::TypeExpr::Function { params, ret } => Type::Function {
            params: params.iter().map(|p| convert_type_expr(&p.node)).collect(),
            return_type: Box::new(convert_type_expr(&ret.node)),
        },
        ast::TypeExpr::Record(fields) => Type::Product {
            fields: fields
                .iter()
                .map(|(n, t)| (n.clone(), convert_type_expr(&t.node)))
                .collect(),
        },
        ast::TypeExpr::Applied { name, args } => Type::Applied {
            constructor: name.clone(),
            args: args.iter().map(|a| convert_type_expr(&a.node)).collect(),
        },
        ast::TypeExpr::Stream(inner) => Type::Stream(Box::new(convert_type_expr(&inner.node))),
        ast::TypeExpr::Distribution(inner) => {
            Type::Distribution(Box::new(convert_type_expr(&inner.node)))
        }
        ast::TypeExpr::Dependent { name, params } => {
            // Dependent type application: Tensor(n: Nat, m: Nat)
            Type::Applied {
                constructor: name.clone(),
                args: params
                    .iter()
                    .map(|(_, t)| convert_type_expr(&t.node))
                    .collect(),
            }
        }
        ast::TypeExpr::Sum(variants) => Type::Sum {
            variants: variants
                .iter()
                .map(|v| crate::types::Variant {
                    name: v.name.clone(),
                    fields: if v.fields.is_empty() {
                        None
                    } else {
                        Some(
                            v.fields
                                .iter()
                                .map(|(n, t)| (n.clone(), convert_type_expr(&t.node)))
                                .collect(),
                        )
                    },
                })
                .collect(),
        },
        ast::TypeExpr::Refinement { var, base, .. } => Type::Refinement {
            var: var.clone(),
            base: Box::new(convert_type_expr(&base.node)),
            predicate: crate::types::Predicate::Bool(true), // simplified
        },
        ast::TypeExpr::Where { base, .. } => convert_type_expr(&base.node),
    }
}

/// Parse a unit string into a PhysicalUnit.
fn parse_physical_unit(unit: &str) -> PhysicalUnit {
    use crate::types::*;
    match unit {
        "ms" => PhysicalUnit::Duration(DurationUnit::Milliseconds),
        "s" => PhysicalUnit::Duration(DurationUnit::Seconds),
        "min" => PhysicalUnit::Duration(DurationUnit::Minutes),
        "hour" | "h" => PhysicalUnit::Duration(DurationUnit::Hours),
        "B" | "bytes" => PhysicalUnit::Size(SizeUnit::Bytes),
        "KiB" => PhysicalUnit::Size(SizeUnit::KiB),
        "MiB" => PhysicalUnit::Size(SizeUnit::MiB),
        "GiB" => PhysicalUnit::Size(SizeUnit::GiB),
        "bps" => PhysicalUnit::Bandwidth(BandwidthUnit::Bps),
        "Kbps" => PhysicalUnit::Bandwidth(BandwidthUnit::Kbps),
        "Mbps" => PhysicalUnit::Bandwidth(BandwidthUnit::Mbps),
        "Gbps" => PhysicalUnit::Bandwidth(BandwidthUnit::Gbps),
        "USD" => PhysicalUnit::Currency(CurrencyUnit::USD),
        "EUR" => PhysicalUnit::Currency(CurrencyUnit::EUR),
        "JPY" => PhysicalUnit::Currency(CurrencyUnit::JPY),
        "GBP" => PhysicalUnit::Currency(CurrencyUnit::GBP),
        _ => PhysicalUnit::Duration(DurationUnit::Seconds), // fallback
    }
}

// ── Tests ─────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::*;
    use lattice_parser::ast::{BinOp, Expr, Spanned};

    fn sp(expr: Expr) -> Spanned<Expr> {
        Spanned::dummy(expr)
    }

    // ── Synthesis tests ───────────────────────────────────────────

    #[test]
    fn synthesize_literals() {
        let mut tc = TypeChecker::new();
        assert_eq!(tc.synthesize(&sp(Expr::IntLit(42))).unwrap(), Type::Int);
        assert_eq!(tc.synthesize(&sp(Expr::FloatLit(3.14))).unwrap(), Type::Float);
        assert_eq!(
            tc.synthesize(&sp(Expr::StringLit("hi".into()))).unwrap(),
            Type::String
        );
        assert_eq!(
            tc.synthesize(&sp(Expr::BoolLit(true))).unwrap(),
            Type::Bool
        );
    }

    #[test]
    fn synthesize_variable_lookup() {
        let mut tc = TypeChecker::new();
        tc.env.bind("x".into(), Type::Int);
        assert_eq!(
            tc.synthesize(&sp(Expr::Ident("x".into()))).unwrap(),
            Type::Int
        );
    }

    #[test]
    fn synthesize_unbound_variable() {
        let mut tc = TypeChecker::new();
        assert!(tc.synthesize(&sp(Expr::Ident("nope".into()))).is_err());
        assert!(!tc.is_ok());
    }

    #[test]
    fn synthesize_let_binding() {
        let mut tc = TypeChecker::new();
        // let x = 42; then x is in scope
        let let_expr = sp(Expr::Let {
            name: "x".into(),
            type_ann: None,
            value: Box::new(sp(Expr::IntLit(42))),
        });
        assert_eq!(tc.synthesize(&let_expr).unwrap(), Type::Unit);
        // x should be bound now
        assert_eq!(
            tc.synthesize(&sp(Expr::Ident("x".into()))).unwrap(),
            Type::Int
        );
    }

    #[test]
    fn synthesize_let_with_annotation() {
        let mut tc = TypeChecker::new();
        let let_expr = sp(Expr::Let {
            name: "x".into(),
            type_ann: Some(Spanned::dummy(ast::TypeExpr::Named("Float".into()))),
            value: Box::new(sp(Expr::IntLit(42))),
        });
        assert_eq!(tc.synthesize(&let_expr).unwrap(), Type::Unit);
        // x should be Float due to annotation
        assert_eq!(
            tc.synthesize(&sp(Expr::Ident("x".into()))).unwrap(),
            Type::Float
        );
    }

    #[test]
    fn synthesize_lambda() {
        let mut tc = TypeChecker::new();
        let expr = sp(Expr::Lambda {
            params: vec![ast::Param {
                name: "x".into(),
                type_expr: Spanned::dummy(ast::TypeExpr::Named("Int".into())),
            }],
            body: Box::new(sp(Expr::Ident("x".into()))),
        });
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
        tc.env.bind(
            "f".into(),
            Type::Function {
                params: vec![Type::Int],
                return_type: Box::new(Type::Bool),
            },
        );
        let expr = sp(Expr::Call {
            func: Box::new(sp(Expr::Ident("f".into()))),
            args: vec![sp(Expr::IntLit(42))],
        });
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
        let expr = sp(Expr::Call {
            func: Box::new(sp(Expr::Ident("f".into()))),
            args: vec![sp(Expr::IntLit(42))],
        });
        assert!(tc.synthesize(&expr).is_err());
    }

    // ── BinOp tests ───────────────────────────────────────────────

    #[test]
    fn arithmetic_int() {
        let mut tc = TypeChecker::new();
        let expr = sp(Expr::BinOp {
            op: BinOp::Add,
            left: Box::new(sp(Expr::IntLit(1))),
            right: Box::new(sp(Expr::IntLit(2))),
        });
        assert_eq!(tc.synthesize(&expr).unwrap(), Type::Int);
    }

    #[test]
    fn arithmetic_int_float_widening() {
        let mut tc = TypeChecker::new();
        let expr = sp(Expr::BinOp {
            op: BinOp::Mul,
            left: Box::new(sp(Expr::IntLit(2))),
            right: Box::new(sp(Expr::FloatLit(3.0))),
        });
        assert_eq!(tc.synthesize(&expr).unwrap(), Type::Float);
    }

    #[test]
    fn comparison_returns_bool() {
        let mut tc = TypeChecker::new();
        let expr = sp(Expr::BinOp {
            op: BinOp::Lt,
            left: Box::new(sp(Expr::IntLit(1))),
            right: Box::new(sp(Expr::IntLit(2))),
        });
        assert_eq!(tc.synthesize(&expr).unwrap(), Type::Bool);
    }

    #[test]
    fn logical_ops() {
        let mut tc = TypeChecker::new();
        let expr = sp(Expr::BinOp {
            op: BinOp::And,
            left: Box::new(sp(Expr::BoolLit(true))),
            right: Box::new(sp(Expr::BoolLit(false))),
        });
        assert_eq!(tc.synthesize(&expr).unwrap(), Type::Bool);
    }

    #[test]
    fn logical_op_non_bool_error() {
        let mut tc = TypeChecker::new();
        let expr = sp(Expr::BinOp {
            op: BinOp::And,
            left: Box::new(sp(Expr::IntLit(1))),
            right: Box::new(sp(Expr::BoolLit(true))),
        });
        assert!(tc.synthesize(&expr).is_err());
    }

    // ── Pipeline tests ────────────────────────────────────────────

    #[test]
    fn pipeline_operator() {
        let mut tc = TypeChecker::new();
        tc.env.bind(
            "inc".into(),
            Type::Function {
                params: vec![Type::Int],
                return_type: Box::new(Type::Int),
            },
        );
        let expr = sp(Expr::Pipeline {
            left: Box::new(sp(Expr::IntLit(42))),
            right: Box::new(sp(Expr::Ident("inc".into()))),
        });
        assert_eq!(tc.synthesize(&expr).unwrap(), Type::Int);
    }

    // ── Record tests ──────────────────────────────────────────────

    #[test]
    fn record_construction_and_access() {
        let mut tc = TypeChecker::new();
        let record = sp(Expr::Record(vec![
            ("x".into(), sp(Expr::IntLit(1))),
            ("y".into(), sp(Expr::BoolLit(true))),
        ]));
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
        let expr = sp(Expr::Field {
            expr: Box::new(sp(Expr::Ident("r".into()))),
            name: "y".into(),
        });
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
        let expr = sp(Expr::Field {
            expr: Box::new(sp(Expr::Ident("r".into()))),
            name: "z".into(),
        });
        assert!(tc.synthesize(&expr).is_err());
    }

    // ── Unit type tests ───────────────────────────────────────────

    #[test]
    fn unit_literal_synthesis() {
        let mut tc = TypeChecker::new();
        let expr = sp(Expr::WithUnit {
            value: Box::new(sp(Expr::FloatLit(200.0))),
            unit: "ms".into(),
        });
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
        let expr = sp(Expr::BinOp {
            op: BinOp::Add,
            left: Box::new(sp(Expr::WithUnit {
                value: Box::new(sp(Expr::FloatLit(1.0))),
                unit: "s".into(),
            })),
            right: Box::new(sp(Expr::WithUnit {
                value: Box::new(sp(Expr::FloatLit(200.0))),
                unit: "ms".into(),
            })),
        });
        assert!(tc.synthesize(&expr).is_ok());
    }

    #[test]
    fn unit_different_dimension_error() {
        let mut tc = TypeChecker::new();
        let expr = sp(Expr::BinOp {
            op: BinOp::Add,
            left: Box::new(sp(Expr::WithUnit {
                value: Box::new(sp(Expr::FloatLit(1.0))),
                unit: "s".into(),
            })),
            right: Box::new(sp(Expr::WithUnit {
                value: Box::new(sp(Expr::FloatLit(1.0))),
                unit: "GiB".into(),
            })),
        });
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
        assert!(!tc.is_subtype(&Type::Int, &nat));
    }

    #[test]
    fn subtype_product_width() {
        let tc = TypeChecker::new();
        let sub = Type::Product {
            fields: vec![
                ("x".into(), Type::Int),
                ("y".into(), Type::Bool),
                ("z".into(), Type::String),
            ],
        };
        let sup = Type::Product {
            fields: vec![("x".into(), Type::Int), ("y".into(), Type::Bool)],
        };
        assert!(tc.is_subtype(&sub, &sup));
        assert!(!tc.is_subtype(&sup, &sub));
    }

    // ── Check tests ───────────────────────────────────────────────

    #[test]
    fn check_subtype_accepted() {
        let mut tc = TypeChecker::new();
        tc.check(&sp(Expr::IntLit(42)), &Type::Float).unwrap();
    }

    #[test]
    fn check_mismatch_rejected() {
        let mut tc = TypeChecker::new();
        assert!(tc.check(&sp(Expr::StringLit("hi".into())), &Type::Int).is_err());
    }

    // ── Unification tests ─────────────────────────────────────────

    #[test]
    fn unify_same_types() {
        let mut tc = TypeChecker::new();
        assert_eq!(tc.unify(&Type::Int, &Type::Int).unwrap(), Type::Int);
    }

    #[test]
    fn unify_int_float() {
        let mut tc = TypeChecker::new();
        assert_eq!(tc.unify(&Type::Int, &Type::Float).unwrap(), Type::Float);
    }

    #[test]
    fn unify_type_variable() {
        let mut tc = TypeChecker::new();
        let var = tc.fresh_var();
        let result = tc.unify(&var, &Type::String).unwrap();
        assert_eq!(result, Type::String);
    }

    #[test]
    fn unify_incompatible() {
        let mut tc = TypeChecker::new();
        assert!(tc.unify(&Type::Bool, &Type::String).is_err());
    }

    // ── If expression tests ───────────────────────────────────────

    #[test]
    fn if_expression_same_branches() {
        let mut tc = TypeChecker::new();
        let expr = sp(Expr::If {
            cond: Box::new(sp(Expr::BoolLit(true))),
            then_: Box::new(sp(Expr::IntLit(1))),
            else_: Some(Box::new(sp(Expr::IntLit(2)))),
        });
        assert_eq!(tc.synthesize(&expr).unwrap(), Type::Int);
    }

    #[test]
    fn if_expression_branch_mismatch() {
        let mut tc = TypeChecker::new();
        let expr = sp(Expr::If {
            cond: Box::new(sp(Expr::BoolLit(true))),
            then_: Box::new(sp(Expr::IntLit(1))),
            else_: Some(Box::new(sp(Expr::StringLit("hi".into())))),
        });
        assert!(tc.synthesize(&expr).is_err());
    }

    #[test]
    fn if_expression_non_bool_cond() {
        let mut tc = TypeChecker::new();
        let expr = sp(Expr::If {
            cond: Box::new(sp(Expr::IntLit(1))),
            then_: Box::new(sp(Expr::IntLit(2))),
            else_: Some(Box::new(sp(Expr::IntLit(3)))),
        });
        assert!(tc.synthesize(&expr).is_err());
    }

    // ── Let scoping ───────────────────────────────────────────────

    #[test]
    fn let_scoping() {
        let mut tc = TypeChecker::new();
        // let x = 42, then x is accessible
        tc.synthesize(&sp(Expr::Let {
            name: "x".into(),
            type_ann: None,
            value: Box::new(sp(Expr::IntLit(42))),
        }))
        .unwrap();
        assert_eq!(
            tc.synthesize(&sp(Expr::Ident("x".into()))).unwrap(),
            Type::Int
        );
    }

    // ── Array tests ───────────────────────────────────────────────

    #[test]
    fn synthesize_empty_array() {
        let mut tc = TypeChecker::new();
        let expr = sp(Expr::Array(vec![]));
        let ty = tc.synthesize(&expr).unwrap();
        assert!(matches!(ty, Type::Array(_)));
    }

    #[test]
    fn synthesize_array_literal() {
        let mut tc = TypeChecker::new();
        let expr = sp(Expr::Array(vec![sp(Expr::IntLit(1)), sp(Expr::IntLit(2))]));
        assert_eq!(
            tc.synthesize(&expr).unwrap(),
            Type::Array(Box::new(Type::Int))
        );
    }

    #[test]
    fn synthesize_array_mixed_types_error() {
        let mut tc = TypeChecker::new();
        let expr = sp(Expr::Array(vec![
            sp(Expr::IntLit(1)),
            sp(Expr::StringLit("oops".into())),
        ]));
        assert!(tc.synthesize(&expr).is_err());
    }

    #[test]
    fn synthesize_index_access() {
        let mut tc = TypeChecker::new();
        tc.env.bind("arr".into(), Type::Array(Box::new(Type::Int)));
        let expr = sp(Expr::Index {
            expr: Box::new(sp(Expr::Ident("arr".into()))),
            index: Box::new(sp(Expr::IntLit(0))),
        });
        assert_eq!(tc.synthesize(&expr).unwrap(), Type::Int);
    }

    // ── Match tests ───────────────────────────────────────────────

    #[test]
    fn synthesize_match_expression() {
        let mut tc = TypeChecker::new();
        tc.env.bind("x".into(), Type::Int);
        let expr = sp(Expr::Match {
            expr: Box::new(sp(Expr::Ident("x".into()))),
            arms: vec![ast::MatchArm {
                pattern: Spanned::dummy(ast::Pattern::Wildcard),
                guard: None,
                body: sp(Expr::IntLit(0)),
            }],
        });
        assert_eq!(tc.synthesize(&expr).unwrap(), Type::Int);
    }

    #[test]
    fn synthesize_match_with_binding() {
        let mut tc = TypeChecker::new();
        tc.env.bind("x".into(), Type::Int);
        let expr = sp(Expr::Match {
            expr: Box::new(sp(Expr::Ident("x".into()))),
            arms: vec![ast::MatchArm {
                pattern: Spanned::dummy(ast::Pattern::Ident("y".into())),
                guard: None,
                body: sp(Expr::Ident("y".into())),
            }],
        });
        // y gets a fresh var, so result is a type var
        assert!(tc.synthesize(&expr).is_ok());
    }

    #[test]
    fn synthesize_match_branch_mismatch() {
        let mut tc = TypeChecker::new();
        tc.env.bind("x".into(), Type::Int);
        let expr = sp(Expr::Match {
            expr: Box::new(sp(Expr::Ident("x".into()))),
            arms: vec![
                ast::MatchArm {
                    pattern: Spanned::dummy(ast::Pattern::Literal(sp(Expr::IntLit(0)))),
                    guard: None,
                    body: sp(Expr::IntLit(1)),
                },
                ast::MatchArm {
                    pattern: Spanned::dummy(ast::Pattern::Wildcard),
                    guard: None,
                    body: sp(Expr::StringLit("bad".into())),
                },
            ],
        });
        assert!(tc.synthesize(&expr).is_err());
    }

    // ── Block tests ───────────────────────────────────────────────

    #[test]
    fn synthesize_empty_block() {
        let mut tc = TypeChecker::new();
        assert_eq!(
            tc.synthesize(&sp(Expr::Block(vec![]))).unwrap(),
            Type::Unit
        );
    }

    #[test]
    fn synthesize_block() {
        let mut tc = TypeChecker::new();
        let expr = sp(Expr::Block(vec![
            sp(Expr::IntLit(1)),
            sp(Expr::StringLit("hello".into())),
        ]));
        assert_eq!(tc.synthesize(&expr).unwrap(), Type::String);
    }

    // ── Concat tests ──────────────────────────────────────────────

    #[test]
    fn synthesize_concat() {
        let mut tc = TypeChecker::new();
        let expr = sp(Expr::BinOp {
            op: BinOp::Concat,
            left: Box::new(sp(Expr::StringLit("a".into()))),
            right: Box::new(sp(Expr::StringLit("b".into()))),
        });
        assert_eq!(tc.synthesize(&expr).unwrap(), Type::String);
    }

    #[test]
    fn synthesize_concat_int_error() {
        let mut tc = TypeChecker::new();
        let expr = sp(Expr::BinOp {
            op: BinOp::Concat,
            left: Box::new(sp(Expr::IntLit(1))),
            right: Box::new(sp(Expr::StringLit("b".into()))),
        });
        assert!(tc.synthesize(&expr).is_err());
    }

    // ── Implies ───────────────────────────────────────────────────

    #[test]
    fn synthesize_implies() {
        let mut tc = TypeChecker::new();
        let expr = sp(Expr::BinOp {
            op: BinOp::Implies,
            left: Box::new(sp(Expr::BoolLit(true))),
            right: Box::new(sp(Expr::BoolLit(false))),
        });
        assert_eq!(tc.synthesize(&expr).unwrap(), Type::Bool);
    }

    // ── UnaryOp tests ─────────────────────────────────────────────

    #[test]
    fn synthesize_neg_int() {
        let mut tc = TypeChecker::new();
        let expr = sp(Expr::UnaryOp {
            op: ast::UnaryOp::Neg,
            operand: Box::new(sp(Expr::IntLit(7))),
        });
        assert_eq!(tc.synthesize(&expr).unwrap(), Type::Int);
    }

    #[test]
    fn synthesize_neg_float() {
        let mut tc = TypeChecker::new();
        let expr = sp(Expr::UnaryOp {
            op: ast::UnaryOp::Neg,
            operand: Box::new(sp(Expr::FloatLit(3.14))),
        });
        assert_eq!(tc.synthesize(&expr).unwrap(), Type::Float);
    }

    #[test]
    fn synthesize_neg_bool_error() {
        let mut tc = TypeChecker::new();
        let expr = sp(Expr::UnaryOp {
            op: ast::UnaryOp::Neg,
            operand: Box::new(sp(Expr::BoolLit(true))),
        });
        assert!(tc.synthesize(&expr).is_err());
    }

    #[test]
    fn synthesize_not_bool() {
        let mut tc = TypeChecker::new();
        let expr = sp(Expr::UnaryOp {
            op: ast::UnaryOp::Not,
            operand: Box::new(sp(Expr::BoolLit(true))),
        });
        assert_eq!(tc.synthesize(&expr).unwrap(), Type::Bool);
    }

    #[test]
    fn synthesize_not_int_error() {
        let mut tc = TypeChecker::new();
        let expr = sp(Expr::UnaryOp {
            op: ast::UnaryOp::Not,
            operand: Box::new(sp(Expr::IntLit(1))),
        });
        assert!(tc.synthesize(&expr).is_err());
    }

    // ── Ascription tests ──────────────────────────────────────────

    #[test]
    fn synthesize_ascription() {
        let mut tc = TypeChecker::new();
        let expr = sp(Expr::Ascription {
            expr: Box::new(sp(Expr::IntLit(42))),
            type_expr: Spanned::dummy(ast::TypeExpr::Named("Float".into())),
        });
        assert_eq!(tc.synthesize(&expr).unwrap(), Type::Float);
    }

    #[test]
    fn synthesize_ascription_mismatch() {
        let mut tc = TypeChecker::new();
        let expr = sp(Expr::Ascription {
            expr: Box::new(sp(Expr::StringLit("hi".into()))),
            type_expr: Spanned::dummy(ast::TypeExpr::Named("Int".into())),
        });
        assert!(tc.synthesize(&expr).is_err());
    }

    // ── DoBlock tests ─────────────────────────────────────────────

    #[test]
    fn synthesize_do_block() {
        let mut tc = TypeChecker::new();
        let expr = sp(Expr::DoBlock(vec![
            Spanned::dummy(ast::DoStatement::Let {
                name: "x".into(),
                expr: sp(Expr::IntLit(1)),
            }),
            Spanned::dummy(ast::DoStatement::Yield(sp(Expr::Ident("x".into())))),
        ]));
        assert_eq!(tc.synthesize(&expr).unwrap(), Type::Int);
    }

    // ── Range tests ───────────────────────────────────────────────

    #[test]
    fn synthesize_range() {
        let mut tc = TypeChecker::new();
        let expr = sp(Expr::Range {
            start: Box::new(sp(Expr::IntLit(0))),
            end: Box::new(sp(Expr::IntLit(10))),
        });
        assert_eq!(
            tc.synthesize(&expr).unwrap(),
            Type::Array(Box::new(Type::Int))
        );
    }

    #[test]
    fn synthesize_range_float_error() {
        let mut tc = TypeChecker::new();
        let expr = sp(Expr::Range {
            start: Box::new(sp(Expr::FloatLit(0.0))),
            end: Box::new(sp(Expr::IntLit(10))),
        });
        assert!(tc.synthesize(&expr).is_err());
    }

    // ── Slice tests ───────────────────────────────────────────────

    #[test]
    fn synthesize_slice() {
        let mut tc = TypeChecker::new();
        tc.env.bind("arr".into(), Type::Array(Box::new(Type::Int)));
        let expr = sp(Expr::Slice {
            expr: Box::new(sp(Expr::Ident("arr".into()))),
            start: Some(Box::new(sp(Expr::IntLit(0)))),
            end: Some(Box::new(sp(Expr::IntLit(3)))),
        });
        assert_eq!(
            tc.synthesize(&expr).unwrap(),
            Type::Array(Box::new(Type::Int))
        );
    }

    #[test]
    fn synthesize_slice_non_array_error() {
        let mut tc = TypeChecker::new();
        tc.env.bind("x".into(), Type::Int);
        let expr = sp(Expr::Slice {
            expr: Box::new(sp(Expr::Ident("x".into()))),
            start: None,
            end: None,
        });
        assert!(tc.synthesize(&expr).is_err());
    }

    // ── Exhaustiveness tests ──────────────────────────────────────

    #[test]
    fn exhaustive_match_all_constructors() {
        let mut tc = TypeChecker::new();
        use crate::environment::TypeDef;
        tc.env.register_type(TypeDef {
            name: "Bool2".into(),
            params: vec![],
            body: Type::Sum {
                variants: vec![
                    crate::types::Variant {
                        name: "True".into(),
                        fields: None,
                    },
                    crate::types::Variant {
                        name: "False".into(),
                        fields: None,
                    },
                ],
            },
        });
        tc.env.bind("x".into(), Type::Named("Bool2".into()));
        let expr = sp(Expr::Match {
            expr: Box::new(sp(Expr::Ident("x".into()))),
            arms: vec![
                ast::MatchArm {
                    pattern: Spanned::dummy(ast::Pattern::Constructor("True".into(), vec![])),
                    guard: None,
                    body: sp(Expr::IntLit(1)),
                },
                ast::MatchArm {
                    pattern: Spanned::dummy(ast::Pattern::Constructor("False".into(), vec![])),
                    guard: None,
                    body: sp(Expr::IntLit(0)),
                },
            ],
        });
        assert!(tc.synthesize(&expr).is_ok());
        assert!(tc.is_ok()); // No non-exhaustive warning
    }

    #[test]
    fn non_exhaustive_match_missing_constructor() {
        let mut tc = TypeChecker::new();
        use crate::environment::TypeDef;
        tc.env.register_type(TypeDef {
            name: "Bool2".into(),
            params: vec![],
            body: Type::Sum {
                variants: vec![
                    crate::types::Variant {
                        name: "True".into(),
                        fields: None,
                    },
                    crate::types::Variant {
                        name: "False".into(),
                        fields: None,
                    },
                ],
            },
        });
        tc.env.bind("x".into(), Type::Named("Bool2".into()));
        let expr = sp(Expr::Match {
            expr: Box::new(sp(Expr::Ident("x".into()))),
            arms: vec![ast::MatchArm {
                pattern: Spanned::dummy(ast::Pattern::Constructor("True".into(), vec![])),
                guard: None,
                body: sp(Expr::IntLit(1)),
            }],
        });
        assert!(tc.synthesize(&expr).is_ok()); // synthesis still succeeds
        assert!(!tc.is_ok()); // but warns about non-exhaustive
    }

    #[test]
    fn exhaustive_match_with_wildcard() {
        let mut tc = TypeChecker::new();
        tc.env.bind("x".into(), Type::Int);
        let expr = sp(Expr::Match {
            expr: Box::new(sp(Expr::Ident("x".into()))),
            arms: vec![
                ast::MatchArm {
                    pattern: Spanned::dummy(ast::Pattern::Literal(sp(Expr::IntLit(0)))),
                    guard: None,
                    body: sp(Expr::StringLit("zero".into())),
                },
                ast::MatchArm {
                    pattern: Spanned::dummy(ast::Pattern::Wildcard),
                    guard: None,
                    body: sp(Expr::StringLit("other".into())),
                },
            ],
        });
        assert!(tc.synthesize(&expr).is_ok());
        assert!(tc.is_ok());
    }

    #[test]
    fn exhaustive_match_with_ident_catch_all() {
        let mut tc = TypeChecker::new();
        tc.env.bind("x".into(), Type::Int);
        let expr = sp(Expr::Match {
            expr: Box::new(sp(Expr::Ident("x".into()))),
            arms: vec![ast::MatchArm {
                pattern: Spanned::dummy(ast::Pattern::Ident("n".into())),
                guard: None,
                body: sp(Expr::Ident("n".into())),
            }],
        });
        assert!(tc.synthesize(&expr).is_ok());
        assert!(tc.is_ok());
    }

    #[test]
    fn non_exhaustive_match_result_missing_err() {
        let mut tc = TypeChecker::new();
        use crate::environment::TypeDef;
        tc.env.register_type(TypeDef {
            name: "Result".into(),
            params: vec![],
            body: Type::Sum {
                variants: vec![
                    crate::types::Variant {
                        name: "Ok".into(),
                        fields: Some(vec![("value".into(), Type::Int)]),
                    },
                    crate::types::Variant {
                        name: "Err".into(),
                        fields: Some(vec![("error".into(), Type::String)]),
                    },
                ],
            },
        });
        tc.env.bind("x".into(), Type::Named("Result".into()));
        let expr = sp(Expr::Match {
            expr: Box::new(sp(Expr::Ident("x".into()))),
            arms: vec![ast::MatchArm {
                pattern: Spanned::dummy(ast::Pattern::Constructor(
                    "Ok".into(),
                    vec![Spanned::dummy(ast::Pattern::Ident("v".into()))],
                )),
                guard: None,
                body: sp(Expr::IntLit(1)),
            }],
        });
        assert!(tc.synthesize(&expr).is_ok());
        // Should report missing "Err"
        let non_exhaustive = tc
            .errors()
            .iter()
            .any(|e| matches!(e, TypeError::NonExhaustiveMatch { .. }));
        assert!(non_exhaustive);
    }

    // ── Dependent type bridge tests ───────────────────────────────

    #[test]
    fn dependent_type_check_tensor() {
        let mut tc = TypeChecker::new();
        let result = tc.check_dependent_type(
            "Tensor",
            &[
                (
                    "n".into(),
                    Spanned::dummy(ast::TypeExpr::Named("Nat".into())),
                ),
                (
                    "m".into(),
                    Spanned::dummy(ast::TypeExpr::Named("Nat".into())),
                ),
            ],
            Span::dummy(),
        );
        assert!(result.is_ok());
        let ty = result.unwrap();
        assert!(matches!(ty, Type::Applied { constructor, .. } if constructor == "Tensor"));
    }

    #[test]
    fn convert_dependent_type_expr() {
        let te = ast::TypeExpr::Dependent {
            name: "Vector".into(),
            params: vec![("n".into(), Spanned::dummy(ast::TypeExpr::Named("Nat".into())))],
        };
        let ty = convert_type_expr(&te);
        assert!(matches!(ty, Type::Applied { constructor, args } if constructor == "Vector" && args.len() == 1));
    }

    #[test]
    fn convert_sum_type_expr() {
        let te = ast::TypeExpr::Sum(vec![
            ast::Variant {
                name: "Some".into(),
                fields: vec![("value".into(), Spanned::dummy(ast::TypeExpr::Named("Int".into())))],
            },
            ast::Variant {
                name: "None".into(),
                fields: vec![],
            },
        ]);
        let ty = convert_type_expr(&te);
        if let Type::Sum { variants } = ty {
            assert_eq!(variants.len(), 2);
            assert_eq!(variants[0].name, "Some");
            assert!(variants[0].fields.is_some());
            assert_eq!(variants[1].name, "None");
            assert!(variants[1].fields.is_none());
        } else {
            panic!("expected Sum type");
        }
    }

    #[test]
    fn convert_function_type_expr() {
        let te = ast::TypeExpr::Function {
            params: vec![Spanned::dummy(ast::TypeExpr::Named("Int".into()))],
            ret: Box::new(Spanned::dummy(ast::TypeExpr::Named("Bool".into()))),
        };
        let ty = convert_type_expr(&te);
        assert_eq!(
            ty,
            Type::Function {
                params: vec![Type::Int],
                return_type: Box::new(Type::Bool),
            }
        );
    }
}
