//! Prompt builder for LLM-driven code synthesis.
//!
//! Converts a [`SynthesisRequest`] into a structured prompt string
//! suitable for feeding to a language model.

use crate::types::{OptimizationTarget, SynthesisRequest, SynthesisStrategy};

/// Build a structured LLM prompt from a synthesis request.
pub fn build_prompt(request: &SynthesisRequest) -> String {
    let mut sections = Vec::new();

    // Header
    sections.push(format!(
        "Synthesize an implementation for the function `{}`.",
        request.function_name
    ));

    // Signature
    sections.push(build_signature(request));

    // Preconditions
    if !request.preconditions.is_empty() {
        sections.push(build_constraint_section("Preconditions", &request.preconditions));
    }

    // Postconditions
    if !request.postconditions.is_empty() {
        sections.push(build_constraint_section("Postconditions", &request.postconditions));
    }

    // Invariants
    if !request.invariants.is_empty() {
        sections.push(build_constraint_section("Invariants", &request.invariants));
    }

    // Strategy hint
    if let Some(strategy) = &request.strategy {
        sections.push(format!("Strategy: {}", format_strategy(strategy)));
    }

    // Optimization target
    if let Some(target) = &request.optimize {
        sections.push(format!("Optimize for: {}", format_optimization(target)));
    }

    // Closing instruction
    sections.push(
        "Generate a correct implementation that satisfies all constraints. \
         Return only the function body."
            .to_string(),
    );

    sections.join("\n\n")
}

fn build_signature(request: &SynthesisRequest) -> String {
    let params: Vec<String> = request
        .parameters
        .iter()
        .map(|(name, ty)| format!("{}: {}", name, ty))
        .collect();

    format!(
        "Signature:\n  function {}({}) -> {}",
        request.function_name,
        params.join(", "),
        request.return_type
    )
}

fn build_constraint_section(title: &str, constraints: &[String]) -> String {
    let items: Vec<String> = constraints.iter().map(|c| format!("  - {}", c)).collect();
    format!("{}:\n{}", title, items.join("\n"))
}

fn format_strategy(strategy: &SynthesisStrategy) -> &str {
    match strategy {
        SynthesisStrategy::PessimisticLocking => "Use pessimistic locking for safe concurrent access",
        SynthesisStrategy::OptimisticLocking => "Use optimistic locking with retry on conflict",
        SynthesisStrategy::LockFree => "Use lock-free data structures and atomic operations",
        SynthesisStrategy::Custom(s) => s.as_str(),
    }
}

fn format_optimization(target: &OptimizationTarget) -> &str {
    match target {
        OptimizationTarget::Latency => "minimal latency",
        OptimizationTarget::Throughput => "maximum throughput",
        OptimizationTarget::Memory => "minimal memory usage",
        OptimizationTarget::TimeComplexity => "optimal time complexity",
        OptimizationTarget::Custom(s) => s.as_str(),
    }
}
