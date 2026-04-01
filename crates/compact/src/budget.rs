use serde::{Deserialize, Serialize};

/// Token budget configuration for a session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenBudget {
    /// Maximum context window tokens the model supports.
    pub context_window: usize,
    /// Maximum tokens to reserve for the model's output.
    pub max_output_tokens: usize,
    /// Threshold at which auto-compaction is triggered
    /// (as a fraction of context_window, e.g. 0.8).
    pub compact_threshold: f64,
}

impl TokenBudget {
    pub fn new(context_window: usize, max_output_tokens: usize) -> Self {
        Self {
            context_window,
            max_output_tokens,
            compact_threshold: 0.8,
        }
    }

    /// Available tokens for input messages.
    pub fn input_budget(&self) -> usize {
        self.context_window.saturating_sub(self.max_output_tokens)
    }

    /// Whether compaction should fire given the current token usage.
    pub fn should_compact(&self, current_tokens: usize) -> bool {
        current_tokens as f64 > self.input_budget() as f64 * self.compact_threshold
    }
}

impl Default for TokenBudget {
    fn default() -> Self {
        Self::new(200_000, 16_000)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_values() {
        let budget = TokenBudget::default();
        assert_eq!(budget.context_window, 200_000);
        assert_eq!(budget.max_output_tokens, 16_000);
        assert!((budget.compact_threshold - 0.8).abs() < f64::EPSILON);
    }

    #[test]
    fn input_budget_calculation() {
        let budget = TokenBudget::new(100_000, 10_000);
        assert_eq!(budget.input_budget(), 90_000);
    }

    #[test]
    fn input_budget_saturating_sub() {
        let budget = TokenBudget::new(100, 200);
        assert_eq!(budget.input_budget(), 0);
    }

    #[test]
    fn should_compact_below_threshold() {
        let budget = TokenBudget::new(100_000, 10_000);
        // input_budget = 90_000, threshold = 72_000
        assert!(!budget.should_compact(70_000));
    }

    #[test]
    fn should_compact_above_threshold() {
        let budget = TokenBudget::new(100_000, 10_000);
        // input_budget = 90_000, threshold = 72_000
        assert!(budget.should_compact(75_000));
    }

    #[test]
    fn should_compact_at_boundary() {
        let budget = TokenBudget::new(100_000, 10_000);
        // input_budget = 90_000, threshold at 0.8 = 72_000
        assert!(!budget.should_compact(72_000));
        assert!(budget.should_compact(72_001));
    }

    #[test]
    fn serde_roundtrip() {
        let budget = TokenBudget::new(50_000, 4_000);
        let json = serde_json::to_string(&budget).unwrap();
        let deserialized: TokenBudget = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.context_window, 50_000);
        assert_eq!(deserialized.max_output_tokens, 4_000);
    }
}
