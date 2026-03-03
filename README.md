# Lattice

**AI-native programming language** where you declare intent and constraints, and the compiler synthesizes optimal implementations.

```lattice
graph UserService {
  version: "1.0.0"

  solve {
    goal: minimize(p99_latency)
    constraint: availability > 0.999
    constraint: ∀ endpoint → response_time < 500.ms
  }

  node GetUser {
    input: Request { params: { id: UUID } }
    output: Response<User>

    proof_obligations: {
      returns_404_if_not_found:
        id ∉ Users.ids ⟹ response.status = 404
    }
  }
}
```

## Key Features

- **Intent-First** -- Declare goals and constraints, not procedures
- **Graph-Native** -- Programs are directed hypergraphs, not sequential text
- **Proof-Carrying** -- Pre/post conditions and invariants verified at compile time
- **Dependent Types** -- Types that depend on values: `Vector(n: Nat)`, `Matrix(m, n)`
- **Refinement Types** -- `type Nat = { n ∈ ℤ | n ≥ 0 }`
- **Probabilistic** -- `Distribution<T>` as a first-class citizen
- **Unicode Math** -- `σ`, `π`, `⋈`, `∀`, `∃`, `λ` with ASCII fallbacks
- **Physical Units** -- `200.ms`, `4.GiB`, `19.99.USD` with compile-time dimension checking

## Architecture

```
lattice/
├── crates/
│   ├── lattice-parser/     # Lexer, recursive descent parser, AST, pretty printer
│   ├── lattice-types/      # Type system, dependent types, normalization, type checker
│   ├── lattice-proof/      # Proof obligation extraction, arithmetic solver backend
│   ├── lattice-bsg/        # Binary Semantic Graph (protobuf serialization)
│   └── lattice-cli/        # CLI: parse, check, prove, fmt, compile, bsg-dump
├── tree-sitter-lattice/    # Tree-sitter grammar (WIP)
└── examples/               # Example .lattice programs
```

## Quick Start

```bash
# Build
cargo build

# Parse a .lattice file
cargo run -p lattice-cli -- parse examples/hello.lattice

# Format
cargo run -p lattice-cli -- fmt examples/user_service.lattice

# Verify proof obligations
cargo run -p lattice-cli -- prove examples/transfer.lattice

# Type check (includes proof verification)
cargo run -p lattice-cli -- check examples/transfer.lattice

# JSON output
cargo run -p lattice-cli -- parse examples/hello.lattice --format json
cargo run -p lattice-cli -- prove examples/transfer.lattice --format json
```

## CLI Commands

| Command | Description |
|---------|-------------|
| `lattice parse <file>` | Parse and display AST (`--format ast\|json`) |
| `lattice check <file>` | Type check + proof verification |
| `lattice prove <file>` | Verify all proof obligations (`--format text\|json`) |
| `lattice fmt <file>` | Pretty-print (`--write` to overwrite) |
| `lattice compile <file>` | Compile to BSG (`-o output.bsg`) |
| `lattice bsg-dump <file>` | Dump BSG in human-readable format |

Global flags: `-v` for verbose timing output.

## Type System

```lattice
-- Basic types
type Nat = { n ∈ ℤ | n ≥ 0 }
type Email = { s ∈ String | matches(s, RFC5322) }

-- Dependent types
type Vector(n: Nat) = Array<Float64, length: n>
type Matrix(m: Nat, n: Nat) = Array<Array<Float64, n>, m>

-- Sum types
type Result<T, E> = Ok(T) | Err(E)

-- Probabilistic types
type Prediction = Uncertain<Label> where entropy < 2.0
```

## Proof System

Functions and nodes can declare proof obligations:

```lattice
function transfer(from: Account, to: Account, amount: Money) -> Result {
  pre: {
    from.balance >= amount
    amount > 0
  }

  post: {
    from.balance' = from.balance - amount
    to.balance' = to.balance + amount
    -- Conservation law
    from.balance' + to.balance' = from.balance + to.balance
  }

  synthesize(strategy: pessimistic_locking)
}
```

The proof engine extracts obligations and verifies them using an arithmetic solver backend with symbolic simplification.

## Implementation Status

| Phase | Status | Description |
|-------|--------|-------------|
| Phase 1: Foundation | Done | Parser, type system, BSG, CLI |
| Phase 2: Verification | Done | Proof engine, dependent types |
| Phase 3: Graph Runtime | Planned | Dataflow execution, code generation |
| Phase 4: AI Integration | Planned | LLM synthesis, self-optimization |
| Phase 5: Ecosystem | Planned | LSP, package manager, playground |

## Test Suite

```bash
cargo test --workspace
```

163 tests across all crates:

| Crate | Tests | Coverage |
|-------|-------|----------|
| lattice-parser | 46 | Lexer, parser, all language constructs |
| lattice-types | 88 | Types, inference, dependent types, normalization |
| lattice-proof | 23 | Obligation extraction, arithmetic prover, caching |
| lattice-bsg | 6 | Protobuf schema, AST↔BSG conversion, roundtrip |

## Language Spec

See [lattice-lang-spec.md](lattice-lang-spec.md) for the full language specification.

## License

MIT
