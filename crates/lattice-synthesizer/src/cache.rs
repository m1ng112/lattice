//! Synthesis result cache.
//!
//! Caches verified implementations keyed by a hash of the
//! [`SynthesisRequest`], persisted as a JSON file (similar to
//! `lattice-proof`'s [`ProofCache`]).

use std::collections::hash_map::DefaultHasher;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::path::Path;

use crate::types::{SynthesisRequest, SynthesisResult};

/// Persistent cache for synthesis results.
#[derive(Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct SynthesisCache {
    entries: HashMap<String, SynthesisResult>,
}

impl SynthesisCache {
    pub fn new() -> Self {
        Self::default()
    }

    /// Look up a cached result for the given request.
    pub fn get(&self, request: &SynthesisRequest) -> Option<&SynthesisResult> {
        let key = cache_key(request);
        self.entries.get(&key)
    }

    /// Store a synthesis result.
    pub fn put(&mut self, request: &SynthesisRequest, result: &SynthesisResult) {
        let key = cache_key(request);
        self.entries.insert(key, result.clone());
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

/// Compute a deterministic cache key from a [`SynthesisRequest`].
pub fn cache_key(request: &SynthesisRequest) -> String {
    let mut hasher = DefaultHasher::new();
    request.function_name.hash(&mut hasher);
    for (name, ty) in &request.parameters {
        name.hash(&mut hasher);
        ty.hash(&mut hasher);
    }
    request.return_type.hash(&mut hasher);
    for c in &request.preconditions {
        c.hash(&mut hasher);
    }
    for c in &request.postconditions {
        c.hash(&mut hasher);
    }
    for c in &request.invariants {
        c.hash(&mut hasher);
    }
    if let Some(s) = &request.strategy {
        format!("{s:?}").hash(&mut hasher);
    }
    if let Some(o) = &request.optimize {
        format!("{o:?}").hash(&mut hasher);
    }
    format!("{:016x}", hasher.finish())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::SynthesisStrategy;

    fn make_request(name: &str) -> SynthesisRequest {
        SynthesisRequest {
            function_name: name.to_string(),
            parameters: vec![("x".into(), "Int".into())],
            return_type: "Int".to_string(),
            preconditions: vec!["x > 0".into()],
            postconditions: vec![],
            invariants: vec![],
            strategy: None,
            optimize: None,
        }
    }

    #[test]
    fn cache_new_is_empty() {
        let cache = SynthesisCache::new();
        assert!(cache.is_empty());
        assert_eq!(cache.len(), 0);
    }

    #[test]
    fn cache_put_and_get() {
        let mut cache = SynthesisCache::new();
        let req = make_request("foo");
        let result = SynthesisResult::Synthesized {
            code: "let x = 1".into(),
            verified: true,
            attempts: 1,
        };

        cache.put(&req, &result);
        assert_eq!(cache.len(), 1);

        let hit = cache.get(&req);
        assert!(hit.is_some());
        match hit.unwrap() {
            SynthesisResult::Synthesized { verified, .. } => assert!(verified),
            _ => panic!("expected Synthesized"),
        }
    }

    #[test]
    fn cache_miss_for_different_request() {
        let mut cache = SynthesisCache::new();
        let req_a = make_request("foo");
        let req_b = make_request("bar");
        let result = SynthesisResult::Synthesized {
            code: "let x = 1".into(),
            verified: true,
            attempts: 1,
        };

        cache.put(&req_a, &result);
        assert!(cache.get(&req_b).is_none());
    }

    #[test]
    fn cache_save_load_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("synth_cache.json");

        let mut cache = SynthesisCache::new();
        let req = make_request("roundtrip");
        let result = SynthesisResult::Synthesized {
            code: "let y = 2".into(),
            verified: false,
            attempts: 2,
        };
        cache.put(&req, &result);
        cache.save(&path).unwrap();

        let loaded = SynthesisCache::load(&path).unwrap();
        assert_eq!(loaded.len(), 1);
        assert!(loaded.get(&req).is_some());
    }

    #[test]
    fn cache_key_uniqueness() {
        let req_a = make_request("alpha");
        let req_b = make_request("beta");
        assert_ne!(cache_key(&req_a), cache_key(&req_b));

        // Same name but different strategy → different key
        let mut req_c = make_request("alpha");
        req_c.strategy = Some(SynthesisStrategy::LockFree);
        assert_ne!(cache_key(&req_a), cache_key(&req_c));
    }

    #[test]
    fn cache_overwrite_entry() {
        let mut cache = SynthesisCache::new();
        let req = make_request("overwrite");

        let v1 = SynthesisResult::Synthesized {
            code: "v1".into(),
            verified: false,
            attempts: 1,
        };
        let v2 = SynthesisResult::Synthesized {
            code: "v2".into(),
            verified: true,
            attempts: 2,
        };

        cache.put(&req, &v1);
        cache.put(&req, &v2);
        assert_eq!(cache.len(), 1);

        match cache.get(&req).unwrap() {
            SynthesisResult::Synthesized { code, .. } => assert_eq!(code, "v2"),
            _ => panic!("expected Synthesized"),
        }
    }
}
