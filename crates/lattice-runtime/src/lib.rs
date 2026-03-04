//! Runtime execution engine for the Lattice programming language.
//!
//! Converts AST graphs into executable dataflow graphs using [`petgraph`],
//! then schedules parallel node execution via [`tokio`].
//!
//! # Architecture
//!
//! ```text
//! AST Graph → ExecutableGraph (petgraph) → Scheduler → ExecutionResult
//! ```
//!
//! The [`graph`] module converts parsed AST into an executable form with
//! cycle detection and topological ordering. The [`scheduler`] runs nodes
//! in parallel groups, propagating values through [`stream`] channels.

pub mod error;
pub mod graph;
pub mod node;
pub mod scheduler;
pub mod stream;
