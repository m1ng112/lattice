use lattice_parser::parser;
use lattice_synthesizer::extractor::extract_requests;
use lattice_synthesizer::prompt::build_prompt;
use lattice_synthesizer::types::*;

// ── Extractor tests ─────────────────────────────

#[test]
fn extract_basic_synthesize_function() {
    let src = r#"
function transfer(from: Account, to: Account, amount: Money) -> Result {
  pre: {
    from.balance >= amount
    amount > 0
  }
  post: {
    from.balance_new = from.balance - amount
    to.balance_new = to.balance + amount
  }
  synthesize(strategy: pessimistic_locking)
}
"#;
    let program = parser::parse(src).unwrap();
    let requests = extract_requests(&program);

    assert_eq!(requests.len(), 1);
    let req = &requests[0];
    assert_eq!(req.function_name, "transfer");
    assert_eq!(req.parameters.len(), 3);
    assert_eq!(req.parameters[0], ("from".to_string(), "Account".to_string()));
    assert_eq!(req.parameters[1], ("to".to_string(), "Account".to_string()));
    assert_eq!(req.parameters[2], ("amount".to_string(), "Money".to_string()));
    assert_eq!(req.return_type, "Result");
    assert_eq!(req.preconditions.len(), 2);
    assert_eq!(req.postconditions.len(), 2);
    assert_eq!(req.strategy, Some(SynthesisStrategy::PessimisticLocking));
    assert_eq!(req.optimize, None);
}

#[test]
fn extract_function_with_optimize() {
    let src = r#"
function sort(items: List) -> List {
  pre: { items != null }
  post: { result.length = items.length }
  synthesize(strategy: lock_free, optimize: latency)
}
"#;
    let program = parser::parse(src).unwrap();
    let requests = extract_requests(&program);

    assert_eq!(requests.len(), 1);
    let req = &requests[0];
    assert_eq!(req.function_name, "sort");
    assert_eq!(req.strategy, Some(SynthesisStrategy::LockFree));
    assert_eq!(req.optimize, Some(OptimizationTarget::Latency));
}

#[test]
fn skip_non_synthesize_functions() {
    let src = r#"
function add(a: Int, b: Int) -> Int {
  a + b
}
"#;
    let program = parser::parse(src).unwrap();
    let requests = extract_requests(&program);
    assert!(requests.is_empty());
}

#[test]
fn extract_multiple_functions() {
    let src = r#"
function foo(x: Int) -> Int {
  pre: { x > 0 }
  synthesize(strategy: optimistic_locking)
}

function bar(y: String) -> Bool {
  post: { result = true }
  synthesize(optimize: throughput)
}

function baz(z: Float) -> Float {
  z + 1.0
}
"#;
    let program = parser::parse(src).unwrap();
    let requests = extract_requests(&program);

    assert_eq!(requests.len(), 2);
    assert_eq!(requests[0].function_name, "foo");
    assert_eq!(requests[0].strategy, Some(SynthesisStrategy::OptimisticLocking));
    assert_eq!(requests[1].function_name, "bar");
    assert_eq!(requests[1].optimize, Some(OptimizationTarget::Throughput));
}

#[test]
fn extract_with_invariants() {
    let src = r#"
function check(x: Int) -> Bool {
  pre: { x > 0 }
  invariant: { x >= 0 }
  synthesize(strategy: auto)
}
"#;
    let program = parser::parse(src).unwrap();
    let requests = extract_requests(&program);

    assert_eq!(requests.len(), 1);
    let req = &requests[0];
    assert_eq!(req.invariants.len(), 1);
    assert_eq!(req.preconditions.len(), 1);
    assert_eq!(req.strategy, Some(SynthesisStrategy::Custom("auto".to_string())));
}

#[test]
fn extract_no_return_type() {
    let src = r#"
function fire(event: Event) {
  synthesize()
}
"#;
    let program = parser::parse(src).unwrap();
    let requests = extract_requests(&program);

    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].return_type, "()");
    assert_eq!(requests[0].strategy, None);
    assert_eq!(requests[0].optimize, None);
}

#[test]
fn empty_program_yields_no_requests() {
    let requests = extract_requests(&vec![]);
    assert!(requests.is_empty());
}

#[test]
fn custom_strategy_and_optimize() {
    let src = r#"
function process(data: Bytes) -> Bytes {
  synthesize(strategy: my_custom_strat, optimize: my_custom_opt)
}
"#;
    let program = parser::parse(src).unwrap();
    let requests = extract_requests(&program);

    assert_eq!(requests.len(), 1);
    assert_eq!(
        requests[0].strategy,
        Some(SynthesisStrategy::Custom("my_custom_strat".to_string()))
    );
    assert_eq!(
        requests[0].optimize,
        Some(OptimizationTarget::Custom("my_custom_opt".to_string()))
    );
}

// ── Prompt builder tests ────────────────────────

#[test]
fn prompt_contains_function_name() {
    let req = SynthesisRequest {
        function_name: "transfer".to_string(),
        parameters: vec![
            ("from".to_string(), "Account".to_string()),
            ("to".to_string(), "Account".to_string()),
        ],
        return_type: "Result".to_string(),
        preconditions: vec!["from.balance >= amount".to_string()],
        postconditions: vec!["to.balance = to.balance + amount".to_string()],
        invariants: vec![],
        strategy: Some(SynthesisStrategy::PessimisticLocking),
        optimize: None,
    };
    let prompt = build_prompt(&req);

    assert!(prompt.contains("transfer"));
    assert!(prompt.contains("from: Account"));
    assert!(prompt.contains("to: Account"));
    assert!(prompt.contains("Result"));
    assert!(prompt.contains("from.balance >= amount"));
    assert!(prompt.contains("to.balance = to.balance + amount"));
    assert!(prompt.contains("pessimistic locking"));
}

#[test]
fn prompt_includes_optimization_target() {
    let req = SynthesisRequest {
        function_name: "sort".to_string(),
        parameters: vec![("items".to_string(), "List".to_string())],
        return_type: "List".to_string(),
        preconditions: vec![],
        postconditions: vec![],
        invariants: vec!["items.length >= 0".to_string()],
        strategy: None,
        optimize: Some(OptimizationTarget::Memory),
    };
    let prompt = build_prompt(&req);

    assert!(prompt.contains("sort"));
    assert!(prompt.contains("Invariants:"));
    assert!(prompt.contains("items.length >= 0"));
    assert!(prompt.contains("minimal memory"));
    assert!(!prompt.contains("Preconditions:"));
    assert!(!prompt.contains("Strategy:"));
}

#[test]
fn prompt_minimal_request() {
    let req = SynthesisRequest {
        function_name: "noop".to_string(),
        parameters: vec![],
        return_type: "()".to_string(),
        preconditions: vec![],
        postconditions: vec![],
        invariants: vec![],
        strategy: None,
        optimize: None,
    };
    let prompt = build_prompt(&req);

    assert!(prompt.contains("noop"));
    assert!(prompt.contains("Signature:"));
    assert!(!prompt.contains("Preconditions:"));
    assert!(!prompt.contains("Postconditions:"));
    assert!(!prompt.contains("Invariants:"));
    assert!(!prompt.contains("Strategy:"));
    assert!(!prompt.contains("Optimize for:"));
}

// ── Types tests ─────────────────────────────────

#[test]
fn strategy_from_ident_known_values() {
    assert_eq!(
        SynthesisStrategy::from_ident("pessimistic_locking"),
        SynthesisStrategy::PessimisticLocking
    );
    assert_eq!(
        SynthesisStrategy::from_ident("optimistic_locking"),
        SynthesisStrategy::OptimisticLocking
    );
    assert_eq!(
        SynthesisStrategy::from_ident("lock_free"),
        SynthesisStrategy::LockFree
    );
    assert_eq!(
        SynthesisStrategy::from_ident("unknown"),
        SynthesisStrategy::Custom("unknown".to_string())
    );
}

#[test]
fn optimization_target_from_ident_known_values() {
    assert_eq!(OptimizationTarget::from_ident("latency"), OptimizationTarget::Latency);
    assert_eq!(
        OptimizationTarget::from_ident("throughput"),
        OptimizationTarget::Throughput
    );
    assert_eq!(OptimizationTarget::from_ident("memory"), OptimizationTarget::Memory);
    assert_eq!(
        OptimizationTarget::from_ident("time_complexity"),
        OptimizationTarget::TimeComplexity
    );
    assert_eq!(
        OptimizationTarget::from_ident("custom_thing"),
        OptimizationTarget::Custom("custom_thing".to_string())
    );
}

// ── Round-trip: parse → extract → prompt ────────

#[test]
fn end_to_end_parse_extract_prompt() {
    let src = r#"
function transfer(from: Account, to: Account, amount: Money) -> Result {
  pre: {
    from.balance >= amount
    amount > 0
  }
  post: {
    from.balance_new = from.balance - amount
    to.balance_new = to.balance + amount
    from.balance_new + to.balance_new = from.balance + to.balance
  }
  synthesize(strategy: pessimistic_locking)
}
"#;
    let program = parser::parse(src).unwrap();
    let requests = extract_requests(&program);
    assert_eq!(requests.len(), 1);

    let prompt = build_prompt(&requests[0]);
    assert!(prompt.contains("transfer"));
    assert!(prompt.contains("from: Account"));
    assert!(prompt.contains("Preconditions:"));
    assert!(prompt.contains("Postconditions:"));
    assert!(prompt.contains("pessimistic locking"));
    assert!(prompt.contains("function body"));
}

#[test]
fn synthesis_result_serialization() {
    let result = SynthesisResult::Synthesized {
        code: "fn transfer() { }".to_string(),
        verified: true,
        attempts: 3,
    };
    let json = serde_json::to_string(&result).unwrap();
    assert!(json.contains("Synthesized"));
    assert!(json.contains("verified"));

    let result2 = SynthesisResult::ManualRequired {
        reason: "too complex".to_string(),
    };
    let json2 = serde_json::to_string(&result2).unwrap();
    assert!(json2.contains("ManualRequired"));
    assert!(json2.contains("too complex"));
}
