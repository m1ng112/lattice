//! Type environment for name resolution and scope management.
//!
//! Supports nested lexical scopes with inner-to-outer lookup
//! and built-in type pre-population.

use std::collections::HashMap;

use crate::types::{Type, Variant};

/// A type definition (named type alias or ADT).
#[derive(Debug, Clone)]
pub struct TypeDef {
    pub name: String,
    /// Type parameters (e.g., `T` in `Option<T>`).
    pub params: Vec<String>,
    /// The underlying type.
    pub body: Type,
}

/// A single lexical scope.
#[derive(Debug, Clone, Default)]
struct Scope {
    bindings: HashMap<String, Type>,
    type_defs: HashMap<String, TypeDef>,
}

/// A type environment mapping names to their types across nested scopes.
#[derive(Debug, Clone)]
pub struct TypeEnv {
    scopes: Vec<Scope>,
}

impl Default for TypeEnv {
    fn default() -> Self {
        Self::new()
    }
}

impl TypeEnv {
    /// Create a new environment pre-populated with built-in types.
    pub fn new() -> Self {
        let mut env = Self {
            scopes: vec![Scope::default()],
        };
        env.register_builtins();
        env
    }

    /// Create an empty environment with no built-ins (for testing).
    pub fn empty() -> Self {
        Self {
            scopes: vec![Scope::default()],
        }
    }

    /// Push a new lexical scope.
    pub fn push_scope(&mut self) {
        self.scopes.push(Scope::default());
    }

    /// Pop the innermost scope. Panics if only the global scope remains.
    pub fn pop_scope(&mut self) {
        assert!(self.scopes.len() > 1, "cannot pop the global scope");
        self.scopes.pop();
    }

    /// Bind a variable name to a type in the current (innermost) scope.
    pub fn bind(&mut self, name: String, ty: Type) {
        self.current_scope_mut().bindings.insert(name, ty);
    }

    /// Look up a variable's type, searching from innermost to outermost scope.
    pub fn lookup(&self, name: &str) -> Option<&Type> {
        for scope in self.scopes.iter().rev() {
            if let Some(ty) = scope.bindings.get(name) {
                return Some(ty);
            }
        }
        None
    }

    /// Register a named type definition in the current scope.
    pub fn register_type(&mut self, def: TypeDef) {
        self.current_scope_mut()
            .type_defs
            .insert(def.name.clone(), def);
    }

    /// Look up a type definition by name, searching inner to outer.
    pub fn lookup_type(&self, name: &str) -> Option<&TypeDef> {
        for scope in self.scopes.iter().rev() {
            if let Some(def) = scope.type_defs.get(name) {
                return Some(def);
            }
        }
        None
    }

    /// Returns the current scope depth (1 = global only).
    pub fn depth(&self) -> usize {
        self.scopes.len()
    }

    fn current_scope_mut(&mut self) -> &mut Scope {
        self.scopes.last_mut().expect("at least one scope must exist")
    }

    fn register_builtins(&mut self) {
        // Option<T> = Some(T) | None
        self.register_type(TypeDef {
            name: "Option".into(),
            params: vec!["T".into()],
            body: Type::Sum {
                variants: vec![
                    Variant {
                        name: "Some".into(),
                        fields: Some(vec![("value".into(), Type::Named("T".into()))]),
                    },
                    Variant {
                        name: "None".into(),
                        fields: None,
                    },
                ],
            },
        });

        // Result<T, E> = Ok(T) | Err(E)
        self.register_type(TypeDef {
            name: "Result".into(),
            params: vec!["T".into(), "E".into()],
            body: Type::Sum {
                variants: vec![
                    Variant {
                        name: "Ok".into(),
                        fields: Some(vec![("value".into(), Type::Named("T".into()))]),
                    },
                    Variant {
                        name: "Err".into(),
                        fields: Some(vec![("error".into(), Type::Named("E".into()))]),
                    },
                ],
            },
        });

        // List<T>
        self.register_type(TypeDef {
            name: "List".into(),
            params: vec!["T".into()],
            body: Type::Named("List".into()),
        });

        // Nat = { n ∈ Int | n >= 0 }
        self.register_type(TypeDef {
            name: "Nat".into(),
            params: vec![],
            body: Type::Refinement {
                var: "n".into(),
                base: Box::new(Type::Int),
                predicate: crate::types::Predicate::Comparison {
                    left: Box::new(crate::types::PredicateExpr::Var("n".into())),
                    op: crate::types::ComparisonOp::Ge,
                    right: Box::new(crate::types::PredicateExpr::IntLit(0)),
                },
            },
        });

        // Positive = { n ∈ Int | n > 0 }
        self.register_type(TypeDef {
            name: "Positive".into(),
            params: vec![],
            body: Type::Refinement {
                var: "n".into(),
                base: Box::new(Type::Int),
                predicate: crate::types::Predicate::Comparison {
                    left: Box::new(crate::types::PredicateExpr::Var("n".into())),
                    op: crate::types::ComparisonOp::Gt,
                    right: Box::new(crate::types::PredicateExpr::IntLit(0)),
                },
            },
        });

        // Percentage = { x ∈ Float | 0 <= x <= 1 }
        self.register_type(TypeDef {
            name: "Percentage".into(),
            params: vec![],
            body: Type::Refinement {
                var: "x".into(),
                base: Box::new(Type::Float),
                predicate: crate::types::Predicate::And(
                    Box::new(crate::types::Predicate::Comparison {
                        left: Box::new(crate::types::PredicateExpr::FloatLit(0.0)),
                        op: crate::types::ComparisonOp::Le,
                        right: Box::new(crate::types::PredicateExpr::Var("x".into())),
                    }),
                    Box::new(crate::types::Predicate::Comparison {
                        left: Box::new(crate::types::PredicateExpr::Var("x".into())),
                        op: crate::types::ComparisonOp::Le,
                        right: Box::new(crate::types::PredicateExpr::FloatLit(1.0)),
                    }),
                ),
            },
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scope_push_pop() {
        let mut env = TypeEnv::empty();
        assert_eq!(env.depth(), 1);

        env.bind("x".into(), Type::Int);
        assert_eq!(env.lookup("x"), Some(&Type::Int));

        env.push_scope();
        assert_eq!(env.depth(), 2);

        // Inner scope sees outer bindings
        assert_eq!(env.lookup("x"), Some(&Type::Int));

        // Shadow in inner scope
        env.bind("x".into(), Type::Float);
        assert_eq!(env.lookup("x"), Some(&Type::Float));

        env.pop_scope();
        // Back to outer — original binding restored
        assert_eq!(env.lookup("x"), Some(&Type::Int));
    }

    #[test]
    fn inner_scope_does_not_leak() {
        let mut env = TypeEnv::empty();
        env.push_scope();
        env.bind("local".into(), Type::Bool);
        env.pop_scope();

        assert!(env.lookup("local").is_none());
    }

    #[test]
    fn builtin_types_registered() {
        let env = TypeEnv::new();
        assert!(env.lookup_type("Option").is_some());
        assert!(env.lookup_type("Result").is_some());
        assert!(env.lookup_type("List").is_some());
        assert!(env.lookup_type("Nat").is_some());
        assert!(env.lookup_type("Positive").is_some());
        assert!(env.lookup_type("Percentage").is_some());
    }

    #[test]
    fn type_def_shadowing() {
        let mut env = TypeEnv::empty();
        env.register_type(TypeDef {
            name: "Foo".into(),
            params: vec![],
            body: Type::Int,
        });

        env.push_scope();
        env.register_type(TypeDef {
            name: "Foo".into(),
            params: vec![],
            body: Type::Float,
        });

        let inner = env.lookup_type("Foo").unwrap();
        assert_eq!(inner.body, Type::Float);

        env.pop_scope();
        let outer = env.lookup_type("Foo").unwrap();
        assert_eq!(outer.body, Type::Int);
    }

    #[test]
    #[should_panic(expected = "cannot pop the global scope")]
    fn pop_global_scope_panics() {
        let mut env = TypeEnv::empty();
        env.pop_scope();
    }
}
