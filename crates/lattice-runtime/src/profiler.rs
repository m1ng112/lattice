//! Profiling support for graph execution.
//!
//! Collects per-node timing data from graph execution traces
//! and provides hotspot analysis.

use std::fmt;
use std::time::Duration;

/// A single profiling entry for one node execution.
#[derive(Debug, Clone)]
pub struct ProfileEntry {
    /// Name of the executed node.
    pub node_name: String,
    /// Wall-clock duration of the node execution.
    pub duration: Duration,
    /// Size of the input value in bytes (if measurable).
    pub input_size: Option<usize>,
    /// Size of the output value in bytes (if measurable).
    pub output_size: Option<usize>,
}

/// Aggregated profiling report for a full graph execution.
#[derive(Debug, Clone)]
pub struct ProfileReport {
    /// Per-node profiling entries.
    pub entries: Vec<ProfileEntry>,
    /// Total wall-clock duration of the entire graph execution.
    pub total_duration: Duration,
}

impl ProfileReport {
    /// Return entries whose duration exceeds `threshold_pct`% of total.
    pub fn hotspots(&self, threshold_pct: f64) -> Vec<&ProfileEntry> {
        let total_ns = self.total_duration.as_nanos() as f64;
        if total_ns == 0.0 {
            return vec![];
        }
        self.entries
            .iter()
            .filter(|e| {
                let pct = (e.duration.as_nanos() as f64 / total_ns) * 100.0;
                pct > threshold_pct
            })
            .collect()
    }

    /// Produce a human-readable summary of the profile.
    pub fn summary(&self) -> String {
        use fmt::Write;
        let mut out = String::new();
        let total_ns = self.total_duration.as_nanos() as f64;

        writeln!(
            out,
            "Profile Report  (total: {:.2?})",
            self.total_duration
        )
        .unwrap();
        writeln!(out, "{:-<50}", "").unwrap();

        for entry in &self.entries {
            let pct = if total_ns > 0.0 {
                (entry.duration.as_nanos() as f64 / total_ns) * 100.0
            } else {
                0.0
            };
            write!(
                out,
                "  {:<20} {:>10.2?}  ({:>5.1}%)",
                entry.node_name, entry.duration, pct,
            )
            .unwrap();
            if let Some(in_sz) = entry.input_size {
                write!(out, "  in={in_sz}B").unwrap();
            }
            if let Some(out_sz) = entry.output_size {
                write!(out, "  out={out_sz}B").unwrap();
            }
            writeln!(out).unwrap();
        }

        writeln!(out, "{:-<50}", "").unwrap();
        let hotspots = self.hotspots(30.0);
        if hotspots.is_empty() {
            writeln!(out, "  No hotspots (>30% threshold)").unwrap();
        } else {
            writeln!(out, "  Hotspots (>30%):").unwrap();
            for h in &hotspots {
                let pct = (h.duration.as_nanos() as f64 / total_ns) * 100.0;
                writeln!(out, "    {} ({:.1}%)", h.node_name, pct).unwrap();
            }
        }

        out
    }
}

/// Build a [`ProfileReport`] from scheduler trace entries.
pub fn build_report(
    trace: &[crate::scheduler::TraceEntry],
    total_duration: Duration,
) -> ProfileReport {
    use crate::scheduler::TracePhase;

    let entries = trace
        .iter()
        .filter_map(|entry| match &entry.phase {
            TracePhase::Complete => Some(ProfileEntry {
                node_name: entry.node.clone(),
                duration: Duration::from_millis(entry.duration_ms.unwrap_or(0)),
                input_size: None,
                output_size: entry.value.as_ref().map(|v| estimate_value_size(v)),
            }),
            _ => None,
        })
        .collect();

    ProfileReport {
        entries,
        total_duration,
    }
}

/// Rough byte-size estimate for a [`Value`].
fn estimate_value_size(value: &crate::node::Value) -> usize {
    use crate::node::Value;
    match value {
        Value::Null => 0,
        Value::Bool(_) => 1,
        Value::Int(_) => 8,
        Value::Float(_) => 8,
        Value::String(s) => s.len(),
        Value::Array(arr) => arr.iter().map(estimate_value_size).sum(),
        Value::Constructor { name, fields } => {
            name.len() + fields.iter().map(estimate_value_size).sum::<usize>()
        }
        Value::Object(map) => map
            .iter()
            .map(|(k, v)| k.len() + estimate_value_size(v))
            .sum(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_report(durations_ms: &[(&str, u64)], total_ms: u64) -> ProfileReport {
        ProfileReport {
            entries: durations_ms
                .iter()
                .map(|(name, ms)| ProfileEntry {
                    node_name: name.to_string(),
                    duration: Duration::from_millis(*ms),
                    input_size: None,
                    output_size: None,
                })
                .collect(),
            total_duration: Duration::from_millis(total_ms),
        }
    }

    #[test]
    fn hotspots_above_threshold() {
        let report = make_report(&[("fast", 10), ("slow", 80), ("medium", 10)], 100);
        let hot = report.hotspots(50.0);
        assert_eq!(hot.len(), 1);
        assert_eq!(hot[0].node_name, "slow");
    }

    #[test]
    fn hotspots_none_when_all_below_threshold() {
        let report = make_report(&[("a", 30), ("b", 30), ("c", 40)], 100);
        let hot = report.hotspots(50.0);
        assert!(hot.is_empty());
    }

    #[test]
    fn hotspots_empty_report() {
        let report = make_report(&[], 0);
        let hot = report.hotspots(10.0);
        assert!(hot.is_empty());
    }

    #[test]
    fn summary_contains_node_names() {
        let report = make_report(&[("A", 60), ("B", 40)], 100);
        let s = report.summary();
        assert!(s.contains("A"), "summary should contain node A: {s}");
        assert!(s.contains("B"), "summary should contain node B: {s}");
        assert!(s.contains("Profile Report"), "should have header: {s}");
    }

    #[test]
    fn summary_shows_hotspots() {
        let report = make_report(&[("hot_node", 80), ("cool_node", 20)], 100);
        let s = report.summary();
        assert!(s.contains("Hotspots"), "should show hotspots section: {s}");
        assert!(s.contains("hot_node"), "should list hot_node: {s}");
    }

    #[test]
    fn build_report_from_trace() {
        use crate::node::Value;
        use crate::scheduler::{TraceEntry, TracePhase};

        let trace = vec![
            TraceEntry {
                node: "A".into(),
                phase: TracePhase::Start,
                timestamp_ms: 0,
                duration_ms: None,
                value: None,
            },
            TraceEntry {
                node: "A".into(),
                phase: TracePhase::Complete,
                timestamp_ms: 50,
                duration_ms: Some(50),
                value: Some(Value::Int(42)),
            },
            TraceEntry {
                node: "B".into(),
                phase: TracePhase::Start,
                timestamp_ms: 50,
                duration_ms: None,
                value: None,
            },
            TraceEntry {
                node: "B".into(),
                phase: TracePhase::Complete,
                timestamp_ms: 80,
                duration_ms: Some(30),
                value: Some(Value::String("hello".into())),
            },
        ];

        let report = build_report(&trace, Duration::from_millis(80));
        assert_eq!(report.entries.len(), 2);
        assert_eq!(report.entries[0].node_name, "A");
        assert_eq!(report.entries[0].duration, Duration::from_millis(50));
        assert_eq!(report.entries[0].output_size, Some(8)); // Int = 8 bytes
        assert_eq!(report.entries[1].node_name, "B");
        assert_eq!(report.entries[1].output_size, Some(5)); // "hello" = 5 bytes
    }
}
