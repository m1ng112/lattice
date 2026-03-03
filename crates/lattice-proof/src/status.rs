//! Proof status tracking and result caching.
//!
//! Provides [`ProofStatus`] for tracking verification state and
//! [`ProofCache`] for persisting proof results across runs.

use std::collections::HashMap;
use std::path::Path;

/// The verification status of a proof obligation.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ProofStatus {
    Unverified,
    Verified,
    Failed { reason: String },
    Timeout,
    Skipped,
}

/// A single cached proof result, keyed by obligation ID + content hash.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CacheEntry {
    pub obligation_id: String,
    pub content_hash: u64,
    pub status: ProofStatus,
    pub timestamp: u64,
    /// Which solver produced this result: `"z3"`, `"lean4"`, `"manual"`.
    pub solver: String,
    pub duration_ms: u64,
}

/// Persistent cache for proof results.
///
/// Entries are keyed by obligation ID. A cache hit requires both the
/// ID and the content hash to match (so that stale results are
/// automatically invalidated when source changes).
#[derive(Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct ProofCache {
    entries: HashMap<String, CacheEntry>,
}

impl ProofCache {
    pub fn new() -> Self {
        Self::default()
    }

    /// Look up a cached result. Returns `Some` only when the content
    /// hash matches (i.e. the obligation has not changed since it was
    /// last checked).
    pub fn get(&self, id: &str, hash: u64) -> Option<&CacheEntry> {
        self.entries
            .get(id)
            .filter(|entry| entry.content_hash == hash)
    }

    /// Insert or update a cache entry.
    pub fn insert(&mut self, entry: CacheEntry) {
        self.entries.insert(entry.obligation_id.clone(), entry);
    }

    /// Serialize the cache to a JSON file.
    pub fn save(&self, path: &Path) -> Result<(), std::io::Error> {
        let json = serde_json::to_string_pretty(self)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
        std::fs::write(path, json)
    }

    /// Deserialize the cache from a JSON file.
    pub fn load(path: &Path) -> Result<Self, std::io::Error> {
        let json = std::fs::read_to_string(path)?;
        serde_json::from_str(&json)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
    }

    /// Number of cached entries.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the cache is empty.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_insert_and_get() {
        let mut cache = ProofCache::new();
        assert!(cache.is_empty());

        let entry = CacheEntry {
            obligation_id: "po_0".to_string(),
            content_hash: 12345,
            status: ProofStatus::Verified,
            timestamp: 1000,
            solver: "z3".to_string(),
            duration_ms: 42,
        };

        cache.insert(entry);
        assert_eq!(cache.len(), 1);

        // Matching hash → hit
        let hit = cache.get("po_0", 12345);
        assert!(hit.is_some());
        assert_eq!(hit.unwrap().status, ProofStatus::Verified);

        // Mismatched hash → miss (source changed)
        assert!(cache.get("po_0", 99999).is_none());

        // Unknown id → miss
        assert!(cache.get("po_999", 12345).is_none());
    }

    #[test]
    fn cache_save_load_roundtrip() {
        let mut cache = ProofCache::new();
        cache.insert(CacheEntry {
            obligation_id: "po_0".to_string(),
            content_hash: 111,
            status: ProofStatus::Verified,
            timestamp: 1000,
            solver: "z3".to_string(),
            duration_ms: 10,
        });
        cache.insert(CacheEntry {
            obligation_id: "po_1".to_string(),
            content_hash: 222,
            status: ProofStatus::Failed {
                reason: "counterexample found".to_string(),
            },
            timestamp: 2000,
            solver: "lean4".to_string(),
            duration_ms: 500,
        });

        let dir = std::env::temp_dir().join("lattice_proof_test");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("proof_cache.json");

        cache.save(&path).unwrap();
        let loaded = ProofCache::load(&path).unwrap();

        assert_eq!(loaded.len(), 2);
        assert_eq!(
            loaded.get("po_0", 111).unwrap().status,
            ProofStatus::Verified
        );
        assert_eq!(
            loaded.get("po_1", 222).unwrap().status,
            ProofStatus::Failed {
                reason: "counterexample found".to_string(),
            }
        );

        // Cleanup
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_dir(&dir);
    }

    #[test]
    fn cache_overwrite_entry() {
        let mut cache = ProofCache::new();
        cache.insert(CacheEntry {
            obligation_id: "po_0".to_string(),
            content_hash: 100,
            status: ProofStatus::Unverified,
            timestamp: 1,
            solver: "trivial".to_string(),
            duration_ms: 0,
        });

        // Update with new result
        cache.insert(CacheEntry {
            obligation_id: "po_0".to_string(),
            content_hash: 200,
            status: ProofStatus::Verified,
            timestamp: 2,
            solver: "z3".to_string(),
            duration_ms: 50,
        });

        assert_eq!(cache.len(), 1);
        assert!(cache.get("po_0", 100).is_none()); // old hash misses
        assert_eq!(
            cache.get("po_0", 200).unwrap().status,
            ProofStatus::Verified
        );
    }
}
