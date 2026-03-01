//! Core type definitions for the Lattice type system.
//!
//! Based on dependent type theory (Calculus of Constructions)
//! with refinement types, probabilistic types, and unit types.

use serde::{Deserialize, Serialize};
use std::fmt;

/// Unique identifier for type inference variables.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TypeVarId(pub u32);

impl fmt::Display for TypeVarId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "?T{}", self.0)
    }
}

/// The core Type enum representing all Lattice types.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Type {
    /// Integer type (ℤ)
    Int,
    /// Floating-point type (ℝ)
    Float,
    /// String type
    String,
    /// Boolean type
    Bool,
    /// Unit type (void/nothing)
    Unit,

    /// Named/reference type (user-defined or alias)
    Named(std::string::String),

    /// Refinement type: `{ var ∈ base | predicate }`
    Refinement {
        var: std::string::String,
        base: Box<Type>,
        predicate: Predicate,
    },

    /// Dependent function type (Π-type): `(param: param_type) -> return_type`
    DependentFunction {
        param: std::string::String,
        param_type: Box<Type>,
        return_type: Box<Type>,
    },

    /// Simple function type: `(params) -> return_type`
    Function {
        params: Vec<Type>,
        return_type: Box<Type>,
    },

    /// Product type (record/struct): `{ field1: T1, field2: T2, ... }`
    Product { fields: Vec<(std::string::String, Type)> },

    /// Sum type (tagged union): `Variant1(T1) | Variant2(T2) | ...`
    Sum { variants: Vec<Variant> },

    /// Generic/parameterized type: `Name<T1, T2, ...>`
    Applied {
        constructor: std::string::String,
        args: Vec<Type>,
    },

    /// Stream type: `Stream<T>`
    Stream(Box<Type>),

    /// Distribution/probabilistic type: `Distribution<T>`
    Distribution(Box<Type>),

    /// Physical unit type: base type annotated with a unit
    WithUnit {
        base: Box<Type>,
        unit: PhysicalUnit,
    },

    /// Type variable (for inference)
    Var(TypeVarId),
}

impl Type {
    /// Construct `Option<T>` as `Some(T) | None`.
    pub fn option(t: Type) -> Type {
        Type::Applied {
            constructor: "Option".into(),
            args: vec![t],
        }
    }

    /// Construct `Result<T, E>` as `Ok(T) | Err(E)`.
    pub fn result(t: Type, e: Type) -> Type {
        Type::Applied {
            constructor: "Result".into(),
            args: vec![t, e],
        }
    }

    /// Construct `List<T>`.
    pub fn list(t: Type) -> Type {
        Type::Applied {
            constructor: "List".into(),
            args: vec![t],
        }
    }

    /// Construct `Vector(n)` — a dependent-length array of Float.
    pub fn vector(n: u64) -> Type {
        Type::Applied {
            constructor: "Vector".into(),
            args: vec![Type::Named(n.to_string())],
        }
    }

    /// Construct `Matrix(m, n)` — a dependent-shape 2D array of Float.
    pub fn matrix(m: u64, n: u64) -> Type {
        Type::Applied {
            constructor: "Matrix".into(),
            args: vec![Type::Named(m.to_string()), Type::Named(n.to_string())],
        }
    }

    /// Returns true if this is a numeric type (Int or Float).
    pub fn is_numeric(&self) -> bool {
        matches!(self, Type::Int | Type::Float)
    }
}

impl fmt::Display for Type {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Type::Int => write!(f, "Int"),
            Type::Float => write!(f, "Float"),
            Type::String => write!(f, "String"),
            Type::Bool => write!(f, "Bool"),
            Type::Unit => write!(f, "Unit"),
            Type::Named(name) => write!(f, "{name}"),
            Type::Refinement { var, base, predicate } => {
                write!(f, "{{ {var} ∈ {base} | {predicate} }}")
            }
            Type::DependentFunction { param, param_type, return_type } => {
                write!(f, "({param}: {param_type}) -> {return_type}")
            }
            Type::Function { params, return_type } => {
                let ps: Vec<_> = params.iter().map(|p| p.to_string()).collect();
                write!(f, "({}) -> {return_type}", ps.join(", "))
            }
            Type::Product { fields } => {
                let fs: Vec<_> = fields
                    .iter()
                    .map(|(n, t)| format!("{n}: {t}"))
                    .collect();
                write!(f, "{{ {} }}", fs.join(", "))
            }
            Type::Sum { variants } => {
                let vs: Vec<_> = variants.iter().map(|v| v.to_string()).collect();
                write!(f, "{}", vs.join(" | "))
            }
            Type::Applied { constructor, args } => {
                let as_: Vec<_> = args.iter().map(|a| a.to_string()).collect();
                write!(f, "{constructor}<{}>", as_.join(", "))
            }
            Type::Stream(inner) => write!(f, "Stream<{inner}>"),
            Type::Distribution(inner) => write!(f, "Distribution<{inner}>"),
            Type::WithUnit { base, unit } => write!(f, "{base}.{unit}"),
            Type::Var(id) => write!(f, "{id}"),
        }
    }
}

/// A variant in a sum type.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Variant {
    pub name: std::string::String,
    /// None for unit variants (e.g., `None`), Some for data variants.
    pub fields: Option<Vec<(std::string::String, Type)>>,
}

impl fmt::Display for Variant {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.name)?;
        if let Some(fields) = &self.fields {
            let fs: Vec<_> = fields.iter().map(|(n, t)| format!("{n}: {t}")).collect();
            write!(f, "({})", fs.join(", "))?;
        }
        Ok(())
    }
}

/// Predicates for refinement types.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Predicate {
    /// Comparison: `var op value`
    Comparison {
        left: Box<PredicateExpr>,
        op: ComparisonOp,
        right: Box<PredicateExpr>,
    },
    /// Logical AND: `p1 ∧ p2`
    And(Box<Predicate>, Box<Predicate>),
    /// Logical OR: `p1 ∨ p2`
    Or(Box<Predicate>, Box<Predicate>),
    /// Logical NOT: `¬p`
    Not(Box<Predicate>),
    /// Universal quantifier: `∀ x ∈ domain. body`
    ForAll {
        var: std::string::String,
        domain: Box<PredicateExpr>,
        body: Box<Predicate>,
    },
    /// Existential quantifier: `∃ x ∈ domain. body`
    Exists {
        var: std::string::String,
        domain: Box<PredicateExpr>,
        body: Box<Predicate>,
    },
    /// Function call predicate: `f(args...)`
    Call {
        function: std::string::String,
        args: Vec<PredicateExpr>,
    },
    /// Boolean literal
    Bool(bool),
}

impl fmt::Display for Predicate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Predicate::Comparison { left, op, right } => write!(f, "{left} {op} {right}"),
            Predicate::And(l, r) => write!(f, "{l} ∧ {r}"),
            Predicate::Or(l, r) => write!(f, "{l} ∨ {r}"),
            Predicate::Not(p) => write!(f, "¬{p}"),
            Predicate::ForAll { var, domain, body } => {
                write!(f, "∀ {var} ∈ {domain}. {body}")
            }
            Predicate::Exists { var, domain, body } => {
                write!(f, "∃ {var} ∈ {domain}. {body}")
            }
            Predicate::Call { function, args } => {
                let as_: Vec<_> = args.iter().map(|a| a.to_string()).collect();
                write!(f, "{function}({})", as_.join(", "))
            }
            Predicate::Bool(b) => write!(f, "{b}"),
        }
    }
}

/// Expression within a predicate.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum PredicateExpr {
    /// Variable reference
    Var(std::string::String),
    /// Integer literal
    IntLit(i64),
    /// Float literal
    FloatLit(f64),
    /// String literal
    StringLit(std::string::String),
}

impl fmt::Display for PredicateExpr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PredicateExpr::Var(name) => write!(f, "{name}"),
            PredicateExpr::IntLit(n) => write!(f, "{n}"),
            PredicateExpr::FloatLit(x) => write!(f, "{x}"),
            PredicateExpr::StringLit(s) => write!(f, "\"{s}\""),
        }
    }
}

/// Comparison operators.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ComparisonOp {
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
}

impl fmt::Display for ComparisonOp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ComparisonOp::Eq => write!(f, "="),
            ComparisonOp::Ne => write!(f, "≠"),
            ComparisonOp::Lt => write!(f, "<"),
            ComparisonOp::Le => write!(f, "≤"),
            ComparisonOp::Gt => write!(f, ">"),
            ComparisonOp::Ge => write!(f, "≥"),
        }
    }
}

/// Physical units for unit-typed values.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PhysicalUnit {
    Duration(DurationUnit),
    Size(SizeUnit),
    Bandwidth(BandwidthUnit),
    Currency(CurrencyUnit),
    Temperature(TemperatureUnit),
}

impl PhysicalUnit {
    /// Returns the dimension category of this unit. Units with the
    /// same dimension can be added/subtracted after conversion.
    pub fn dimension(&self) -> &'static str {
        match self {
            PhysicalUnit::Duration(_) => "Duration",
            PhysicalUnit::Size(_) => "Size",
            PhysicalUnit::Bandwidth(_) => "Bandwidth",
            PhysicalUnit::Currency(_) => "Currency",
            PhysicalUnit::Temperature(_) => "Temperature",
        }
    }

    /// Returns true if two units share the same dimension.
    pub fn same_dimension(&self, other: &PhysicalUnit) -> bool {
        self.dimension() == other.dimension()
    }
}

impl fmt::Display for PhysicalUnit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PhysicalUnit::Duration(u) => write!(f, "{u}"),
            PhysicalUnit::Size(u) => write!(f, "{u}"),
            PhysicalUnit::Bandwidth(u) => write!(f, "{u}"),
            PhysicalUnit::Currency(u) => write!(f, "{u}"),
            PhysicalUnit::Temperature(u) => write!(f, "{u}"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DurationUnit {
    Milliseconds,
    Seconds,
    Minutes,
    Hours,
}

impl fmt::Display for DurationUnit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DurationUnit::Milliseconds => write!(f, "ms"),
            DurationUnit::Seconds => write!(f, "s"),
            DurationUnit::Minutes => write!(f, "min"),
            DurationUnit::Hours => write!(f, "hour"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SizeUnit {
    Bytes,
    KiB,
    MiB,
    GiB,
}

impl fmt::Display for SizeUnit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SizeUnit::Bytes => write!(f, "B"),
            SizeUnit::KiB => write!(f, "KiB"),
            SizeUnit::MiB => write!(f, "MiB"),
            SizeUnit::GiB => write!(f, "GiB"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BandwidthUnit {
    Bps,
    Kbps,
    Mbps,
    Gbps,
}

impl fmt::Display for BandwidthUnit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BandwidthUnit::Bps => write!(f, "bps"),
            BandwidthUnit::Kbps => write!(f, "Kbps"),
            BandwidthUnit::Mbps => write!(f, "Mbps"),
            BandwidthUnit::Gbps => write!(f, "Gbps"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CurrencyUnit {
    USD,
    EUR,
    JPY,
    GBP,
}

impl fmt::Display for CurrencyUnit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CurrencyUnit::USD => write!(f, "USD"),
            CurrencyUnit::EUR => write!(f, "EUR"),
            CurrencyUnit::JPY => write!(f, "JPY"),
            CurrencyUnit::GBP => write!(f, "GBP"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TemperatureUnit {
    Celsius,
    Fahrenheit,
    Kelvin,
}

impl fmt::Display for TemperatureUnit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TemperatureUnit::Celsius => write!(f, "°C"),
            TemperatureUnit::Fahrenheit => write!(f, "°F"),
            TemperatureUnit::Kelvin => write!(f, "K"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn type_display_primitives() {
        assert_eq!(Type::Int.to_string(), "Int");
        assert_eq!(Type::Float.to_string(), "Float");
        assert_eq!(Type::String.to_string(), "String");
        assert_eq!(Type::Bool.to_string(), "Bool");
        assert_eq!(Type::Unit.to_string(), "Unit");
    }

    #[test]
    fn type_display_function() {
        let f = Type::Function {
            params: vec![Type::Int, Type::Int],
            return_type: Box::new(Type::Bool),
        };
        assert_eq!(f.to_string(), "(Int, Int) -> Bool");
    }

    #[test]
    fn type_constructors() {
        let opt = Type::option(Type::Int);
        assert_eq!(opt.to_string(), "Option<Int>");

        let res = Type::result(Type::String, Type::Named("AppError".into()));
        assert_eq!(res.to_string(), "Result<String, AppError>");

        let list = Type::list(Type::Float);
        assert_eq!(list.to_string(), "List<Float>");
    }

    #[test]
    fn unit_same_dimension() {
        let ms = PhysicalUnit::Duration(DurationUnit::Milliseconds);
        let s = PhysicalUnit::Duration(DurationUnit::Seconds);
        let gib = PhysicalUnit::Size(SizeUnit::GiB);

        assert!(ms.same_dimension(&s));
        assert!(!ms.same_dimension(&gib));
    }

    #[test]
    fn refinement_display() {
        let nat = Type::Refinement {
            var: "n".into(),
            base: Box::new(Type::Int),
            predicate: Predicate::Comparison {
                left: Box::new(PredicateExpr::Var("n".into())),
                op: ComparisonOp::Ge,
                right: Box::new(PredicateExpr::IntLit(0)),
            },
        };
        assert_eq!(nat.to_string(), "{ n ∈ Int | n ≥ 0 }");
    }

    #[test]
    fn is_numeric() {
        assert!(Type::Int.is_numeric());
        assert!(Type::Float.is_numeric());
        assert!(!Type::String.is_numeric());
        assert!(!Type::Bool.is_numeric());
    }
}
