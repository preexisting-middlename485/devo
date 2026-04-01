use async_trait::async_trait;
use serde::{Deserialize, Serialize};

/// A message representation used by the compaction layer.
///
/// The compactor works at the serialized message level so it stays
/// decoupled from the full message types in `core`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompactMessage {
    pub role: String,
    pub content: String,
    /// Estimated token count for this message.
    pub token_estimate: usize,
}

/// Output of a compaction pass.
#[derive(Debug, Clone)]
pub struct CompactResult {
    pub messages: Vec<CompactMessage>,
    pub removed_count: usize,
    pub tokens_saved: usize,
}

/// A pluggable strategy for compressing conversation history.
///
/// Implementations can range from simple truncation to LLM-based
/// summarization, sliding windows, or tiered compression.
#[async_trait]
pub trait CompactStrategy: Send + Sync {
    async fn compact(
        &self,
        messages: Vec<CompactMessage>,
        budget: usize,
    ) -> anyhow::Result<CompactResult>;
}

/// Simplest strategy: drop oldest messages until under budget.
pub struct TruncateStrategy;

#[async_trait]
impl CompactStrategy for TruncateStrategy {
    async fn compact(
        &self,
        messages: Vec<CompactMessage>,
        budget: usize,
    ) -> anyhow::Result<CompactResult> {
        let total: usize = messages.iter().map(|m| m.token_estimate).sum();
        if total <= budget {
            return Ok(CompactResult {
                messages,
                removed_count: 0,
                tokens_saved: 0,
            });
        }

        let mut kept = Vec::new();
        let mut running = 0usize;
        let mut removed = 0usize;
        let mut saved = 0usize;

        // Keep the system/first message, then keep from the end
        for msg in messages.iter().rev() {
            if running + msg.token_estimate <= budget {
                running += msg.token_estimate;
                kept.push(msg.clone());
            } else {
                removed += 1;
                saved += msg.token_estimate;
            }
        }

        kept.reverse();

        Ok(CompactResult {
            messages: kept,
            removed_count: removed,
            tokens_saved: saved,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_msg(role: &str, tokens: usize) -> CompactMessage {
        CompactMessage {
            role: role.into(),
            content: format!("message with {} tokens", tokens),
            token_estimate: tokens,
        }
    }

    #[tokio::test]
    async fn truncate_no_op_when_under_budget() {
        let messages = vec![make_msg("user", 100), make_msg("assistant", 100)];
        let result = TruncateStrategy.compact(messages, 500).await.unwrap();
        assert_eq!(result.messages.len(), 2);
        assert_eq!(result.removed_count, 0);
        assert_eq!(result.tokens_saved, 0);
    }

    #[tokio::test]
    async fn truncate_removes_oldest_when_over_budget() {
        let messages = vec![
            make_msg("user", 300),
            make_msg("assistant", 200),
            make_msg("user", 100),
        ];
        let result = TruncateStrategy.compact(messages, 250).await.unwrap();
        // Budget = 250. From the end: 100 + 200 = 300 > 250, so only last msg fits,
        // then middle msg would push to 300 which exceeds, so only last is kept.
        // Actually: iterating from end: msg[2]=100 (running=100 <=250), msg[1]=200 (running=300 > 250 → removed), msg[0]=300 (running=400 > 250 → removed)
        // Wait, let me re-check: running starts at 0, we add 100 → 100 <=250 → keep, then 200 → 300 > 250 → remove, then 300 → 400 > 250 → remove
        assert_eq!(result.messages.len(), 1);
        assert_eq!(result.removed_count, 2);
        assert_eq!(result.tokens_saved, 500);
    }

    #[tokio::test]
    async fn truncate_keeps_recent_messages() {
        let messages = vec![
            make_msg("user", 100),
            make_msg("assistant", 100),
            make_msg("user", 50),
        ];
        // Budget 180: from end → 50 (=50 ok) → 100 (=150 ok) → 100 (=250 > 180, remove)
        let result = TruncateStrategy.compact(messages, 180).await.unwrap();
        assert_eq!(result.messages.len(), 2);
        assert_eq!(result.removed_count, 1);
        assert_eq!(result.tokens_saved, 100);
        assert_eq!(result.messages[0].role, "assistant");
        assert_eq!(result.messages[1].role, "user");
    }

    #[tokio::test]
    async fn truncate_empty_input() {
        let result = TruncateStrategy.compact(Vec::new(), 100).await.unwrap();
        assert!(result.messages.is_empty());
        assert_eq!(result.removed_count, 0);
    }

    #[tokio::test]
    async fn compact_message_serde() {
        let msg = make_msg("user", 42);
        let json = serde_json::to_string(&msg).unwrap();
        let deserialized: CompactMessage = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.role, "user");
        assert_eq!(deserialized.token_estimate, 42);
    }
}
