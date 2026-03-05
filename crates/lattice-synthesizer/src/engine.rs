//! Synthesis engine: generate-and-verify loop.
//!
//! The [`Synthesizer`] orchestrates LLM code generation with
//! parse validation and proof verification, retrying on failure.

use crate::client::LlmProvider;
use crate::prompt::build_prompt;
use crate::types::{SynthesisRequest, SynthesisResult};

/// Main orchestrator for LLM-driven code synthesis.
pub struct Synthesizer<P: LlmProvider> {
    provider: P,
    max_attempts: u32,
}

impl<P: LlmProvider> Synthesizer<P> {
    pub fn new(provider: P) -> Self {
        Self {
            provider,
            max_attempts: 3,
        }
    }

    /// Set the maximum number of generation attempts before giving up.
    pub fn with_max_attempts(mut self, n: u32) -> Self {
        self.max_attempts = n;
        self
    }

    /// Run the generate-and-verify loop for a single synthesis request.
    pub async fn synthesize(&self, request: &SynthesisRequest) -> SynthesisResult {
        let base_prompt = build_prompt(request);
        let mut feedback = Vec::new();

        for attempt in 1..=self.max_attempts {
            // Build prompt, appending previous failure feedback if any.
            let prompt = if feedback.is_empty() {
                base_prompt.clone()
            } else {
                format!(
                    "{base_prompt}\n\nPrevious attempts failed:\n{}",
                    feedback.join("\n")
                )
            };

            // Step 1: Call LLM to generate candidate code.
            let code = match self.provider.generate(&prompt).await {
                Ok(code) => code,
                Err(e) => {
                    feedback.push(format!("- Attempt {attempt}: API error: {e}"));
                    continue;
                }
            };

            // Step 2: Validate syntax by parsing.
            let program = match lattice_parser::parser::parse(&code) {
                Ok(program) => program,
                Err(errors) => {
                    let msgs: Vec<String> = errors.iter().map(|e| format!("{e}")).collect();
                    feedback.push(format!(
                        "- Attempt {attempt}: Parse error: {}",
                        msgs.join("; ")
                    ));
                    continue;
                }
            };

            // Step 3: Extract proof obligations and verify.
            let obligations = lattice_proof::obligation::extract_obligations(&program);

            if obligations.is_empty() {
                return SynthesisResult::Synthesized {
                    code,
                    verified: true,
                    attempts: attempt,
                };
            }

            let checker = lattice_proof::checker::ProofChecker::default();
            let results = checker.check_all(&obligations);

            let has_failures = results.iter().any(|(_, r)| {
                matches!(r.status, lattice_proof::status::ProofStatus::Failed { .. })
            });

            if !has_failures {
                let all_verified = results.iter().all(|(_, r)| {
                    matches!(
                        r.status,
                        lattice_proof::status::ProofStatus::Verified
                            | lattice_proof::status::ProofStatus::Skipped
                    )
                });
                return SynthesisResult::Synthesized {
                    code,
                    verified: all_verified,
                    attempts: attempt,
                };
            }

            // Collect failure reasons for feedback.
            let failures: Vec<String> = results
                .iter()
                .filter(|(_, r)| {
                    matches!(
                        r.status,
                        lattice_proof::status::ProofStatus::Failed { .. }
                            | lattice_proof::status::ProofStatus::Unverified
                    )
                })
                .map(|(ob, r)| format!("  {} ({:?}): {:?}", ob.name, ob.kind, r.status))
                .collect();

            feedback.push(format!(
                "- Attempt {attempt}: Verification failed:\n{}",
                failures.join("\n")
            ));
        }

        SynthesisResult::ManualRequired {
            reason: format!(
                "Failed after {} attempts:\n{}",
                self.max_attempts,
                feedback.join("\n")
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::LlmProvider;
    use crate::error::SynthesisError;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    /// Mock provider that returns a fixed response.
    struct MockProvider {
        response: Result<String, SynthesisError>,
    }

    #[async_trait::async_trait]
    impl LlmProvider for MockProvider {
        async fn generate(&self, _prompt: &str) -> Result<String, SynthesisError> {
            match &self.response {
                Ok(s) => Ok(s.clone()),
                Err(e) => Err(SynthesisError::ApiError(format!("{e}"))),
            }
        }
    }

    /// Mock provider that tracks calls and returns different responses per attempt.
    struct SequentialMockProvider {
        responses: Vec<Result<String, SynthesisError>>,
        call_count: AtomicUsize,
        prompts: std::sync::Mutex<Vec<String>>,
    }

    impl SequentialMockProvider {
        fn new(responses: Vec<Result<String, SynthesisError>>) -> Self {
            Self {
                responses,
                call_count: AtomicUsize::new(0),
                prompts: std::sync::Mutex::new(Vec::new()),
            }
        }
    }

    #[async_trait::async_trait]
    impl LlmProvider for SequentialMockProvider {
        async fn generate(&self, prompt: &str) -> Result<String, SynthesisError> {
            self.prompts.lock().unwrap().push(prompt.to_string());
            let idx = self.call_count.fetch_add(1, Ordering::SeqCst);
            match self.responses.get(idx) {
                Some(Ok(s)) => Ok(s.clone()),
                Some(Err(e)) => Err(SynthesisError::ApiError(format!("{e}"))),
                None => Err(SynthesisError::ApiError("no more responses".into())),
            }
        }
    }

    fn make_request() -> SynthesisRequest {
        SynthesisRequest {
            function_name: "test_fn".into(),
            parameters: vec![("x".into(), "Int".into())],
            return_type: "Int".to_string(),
            preconditions: vec![],
            postconditions: vec![],
            invariants: vec![],
            strategy: None,
            optimize: None,
        }
    }

    /// Valid Lattice code with no proof obligations.
    const VALID_CODE: &str = "let x = 42";

    /// Invalid code that won't parse (avoids `}` which triggers parser sync loop).
    const INVALID_CODE: &str = "invalid_token another_invalid more_invalid";

    #[tokio::test]
    async fn synthesize_with_valid_code() {
        let provider = MockProvider {
            response: Ok(VALID_CODE.into()),
        };
        let synth = Synthesizer::new(provider);
        let result = synth.synthesize(&make_request()).await;

        match result {
            SynthesisResult::Synthesized {
                code,
                verified,
                attempts,
            } => {
                assert_eq!(code, VALID_CODE);
                assert!(verified);
                assert_eq!(attempts, 1);
            }
            other => panic!("expected Synthesized, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn synthesize_invalid_code_returns_manual_required() {
        let provider = MockProvider {
            response: Ok(INVALID_CODE.into()),
        };
        let synth = Synthesizer::new(provider).with_max_attempts(2);
        let result = synth.synthesize(&make_request()).await;

        match result {
            SynthesisResult::ManualRequired { reason } => {
                assert!(reason.contains("Parse error"), "reason: {reason}");
                assert!(reason.contains("Failed after 2 attempts"), "reason: {reason}");
            }
            other => panic!("expected ManualRequired, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn synthesize_max_attempts_exceeded() {
        let provider = MockProvider {
            response: Ok(INVALID_CODE.into()),
        };
        let synth = Synthesizer::new(provider).with_max_attempts(3);
        let result = synth.synthesize(&make_request()).await;

        assert!(matches!(result, SynthesisResult::ManualRequired { .. }));
    }

    #[tokio::test]
    async fn synthesize_succeeds_on_second_attempt() {
        let provider = Arc::new(SequentialMockProvider::new(vec![
            Ok(INVALID_CODE.into()),
            Ok(VALID_CODE.into()),
        ]));
        let synth = Synthesizer::new(provider.clone());
        let result = synth.synthesize(&make_request()).await;

        match result {
            SynthesisResult::Synthesized { attempts, .. } => {
                assert_eq!(attempts, 2);
            }
            other => panic!("expected Synthesized, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn prompt_includes_feedback_on_retry() {
        let provider = Arc::new(SequentialMockProvider::new(vec![
            Ok(INVALID_CODE.into()),
            Ok(VALID_CODE.into()),
        ]));
        let synth = Synthesizer::new(provider.clone());
        synth.synthesize(&make_request()).await;

        let prompts = provider.prompts.lock().unwrap();
        assert_eq!(prompts.len(), 2);
        // First prompt is just the base prompt
        assert!(!prompts[0].contains("Previous attempts failed"));
        // Second prompt includes feedback from first failure
        assert!(
            prompts[1].contains("Previous attempts failed"),
            "retry prompt should include feedback: {}",
            prompts[1]
        );
        assert!(
            prompts[1].contains("Parse error"),
            "retry prompt should mention parse error: {}",
            prompts[1]
        );
    }

    #[tokio::test]
    async fn synthesize_api_error_retries() {
        let provider = Arc::new(SequentialMockProvider::new(vec![
            Err(SynthesisError::ApiError("rate limited".into())),
            Ok(VALID_CODE.into()),
        ]));
        let synth = Synthesizer::new(provider.clone());
        let result = synth.synthesize(&make_request()).await;

        match result {
            SynthesisResult::Synthesized { attempts, .. } => {
                assert_eq!(attempts, 2);
            }
            other => panic!("expected Synthesized, got {other:?}"),
        }
    }
}
