//! Runtime error types for the Lattice execution engine.

/// Errors that can occur during graph execution.
#[derive(Debug, thiserror::Error)]
pub enum RuntimeError {
    #[error("Node '{name}' failed: {cause}")]
    NodeFailed { name: String, cause: String },

    #[error("Deadlock detected: cycle involving nodes {nodes:?}")]
    Deadlock { nodes: Vec<String> },

    #[error("Timeout: node '{name}' exceeded {timeout_ms}ms")]
    Timeout { name: String, timeout_ms: u64 },

    #[error("Channel closed: edge from '{from}' to '{to}'")]
    ChannelClosed { from: String, to: String },

    #[error("Resource limit exceeded: {resource} ({usage} > {limit})")]
    ResourceExceeded {
        resource: String,
        usage: String,
        limit: String,
    },

    #[error("No implementation for node '{name}'")]
    NoImplementation { name: String },
}
