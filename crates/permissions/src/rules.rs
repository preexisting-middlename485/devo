use crate::{
    PermissionDecision, PermissionMode, PermissionPolicy, PermissionRequest, ResourceKind,
};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

/// A single path/command allow-rule persisted in configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermissionRule {
    pub resource: ResourceKind,
    /// Glob or prefix that the target must match.
    pub pattern: String,
    pub allow: bool,
}

/// A rule-based permission policy.
///
/// 1. If an explicit rule matches, use it.
/// 2. Otherwise fall back to the configured [`PermissionMode`].
pub struct RuleBasedPolicy {
    pub mode: PermissionMode,
    pub rules: Vec<PermissionRule>,
}

impl RuleBasedPolicy {
    pub fn new(mode: PermissionMode) -> Self {
        Self {
            mode,
            rules: Vec::new(),
        }
    }

    pub fn with_rules(mode: PermissionMode, rules: Vec<PermissionRule>) -> Self {
        Self { mode, rules }
    }

    fn match_rule(&self, request: &PermissionRequest) -> Option<&PermissionRule> {
        let target = request.target.as_deref().unwrap_or("");
        self.rules.iter().find(|rule| {
            rule.resource == request.resource && Self::pattern_matches(&rule.pattern, target)
        })
    }

    fn pattern_matches(pattern: &str, target: &str) -> bool {
        if pattern == "*" {
            return true;
        }
        if pattern.ends_with('*') {
            return target.starts_with(pattern.trim_end_matches('*'));
        }
        target == pattern
    }
}

#[async_trait]
impl PermissionPolicy for RuleBasedPolicy {
    async fn check(&self, request: &PermissionRequest) -> PermissionDecision {
        if let Some(rule) = self.match_rule(request) {
            return if rule.allow {
                PermissionDecision::Allow
            } else {
                PermissionDecision::Deny {
                    reason: format!("blocked by rule: {}", rule.pattern),
                }
            };
        }

        match self.mode {
            PermissionMode::AutoApprove => PermissionDecision::Allow,
            PermissionMode::Deny => PermissionDecision::Deny {
                reason: "permission mode is Deny".into(),
            },
            PermissionMode::Interactive => PermissionDecision::Ask {
                message: format!(
                    "{} wants to access {:?}: {}",
                    request.tool_name, request.resource, request.description
                ),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file_write_request(target: Option<&str>) -> PermissionRequest {
        PermissionRequest {
            tool_name: "file_write".into(),
            resource: ResourceKind::FileWrite,
            description: "write a file".into(),
            target: target.map(|s| s.into()),
        }
    }

    #[test]
    fn pattern_matches_wildcard() {
        assert!(RuleBasedPolicy::pattern_matches("*", "anything"));
        assert!(RuleBasedPolicy::pattern_matches("*", ""));
    }

    #[test]
    fn pattern_matches_prefix() {
        assert!(RuleBasedPolicy::pattern_matches(
            "/home/*",
            "/home/user/file.txt"
        ));
        assert!(!RuleBasedPolicy::pattern_matches("/home/*", "/etc/passwd"));
    }

    #[test]
    fn pattern_matches_exact() {
        assert!(RuleBasedPolicy::pattern_matches(
            "/etc/passwd",
            "/etc/passwd"
        ));
        assert!(!RuleBasedPolicy::pattern_matches(
            "/etc/passwd",
            "/etc/shadow"
        ));
    }

    #[tokio::test]
    async fn auto_approve_allows_everything() {
        let policy = RuleBasedPolicy::new(PermissionMode::AutoApprove);
        let req = file_write_request(Some("/tmp/test"));
        assert!(matches!(
            policy.check(&req).await,
            PermissionDecision::Allow
        ));
    }

    #[tokio::test]
    async fn deny_mode_denies_everything() {
        let policy = RuleBasedPolicy::new(PermissionMode::Deny);
        let req = file_write_request(Some("/tmp/test"));
        assert!(matches!(
            policy.check(&req).await,
            PermissionDecision::Deny { .. }
        ));
    }

    #[tokio::test]
    async fn interactive_mode_asks() {
        let policy = RuleBasedPolicy::new(PermissionMode::Interactive);
        let req = file_write_request(Some("/tmp/test"));
        assert!(matches!(
            policy.check(&req).await,
            PermissionDecision::Ask { .. }
        ));
    }

    #[tokio::test]
    async fn explicit_allow_rule_overrides_deny_mode() {
        let rules = vec![PermissionRule {
            resource: ResourceKind::FileWrite,
            pattern: "/tmp/*".into(),
            allow: true,
        }];
        let policy = RuleBasedPolicy::with_rules(PermissionMode::Deny, rules);
        let req = file_write_request(Some("/tmp/test"));
        assert!(matches!(
            policy.check(&req).await,
            PermissionDecision::Allow
        ));
    }

    #[tokio::test]
    async fn explicit_deny_rule_overrides_auto_approve() {
        let rules = vec![PermissionRule {
            resource: ResourceKind::FileWrite,
            pattern: "/etc/*".into(),
            allow: false,
        }];
        let policy = RuleBasedPolicy::with_rules(PermissionMode::AutoApprove, rules);
        let req = file_write_request(Some("/etc/passwd"));
        assert!(matches!(
            policy.check(&req).await,
            PermissionDecision::Deny { .. }
        ));
    }

    #[tokio::test]
    async fn no_matching_rule_falls_back_to_mode() {
        let rules = vec![PermissionRule {
            resource: ResourceKind::ShellExec,
            pattern: "*".into(),
            allow: true,
        }];
        let policy = RuleBasedPolicy::with_rules(PermissionMode::Deny, rules);
        // FileWrite request won't match a ShellExec rule
        let req = file_write_request(Some("/tmp/test"));
        assert!(matches!(
            policy.check(&req).await,
            PermissionDecision::Deny { .. }
        ));
    }

    #[tokio::test]
    async fn request_without_target_matches_empty_string() {
        let rules = vec![PermissionRule {
            resource: ResourceKind::FileWrite,
            pattern: "".into(),
            allow: true,
        }];
        let policy = RuleBasedPolicy::with_rules(PermissionMode::Deny, rules);
        let req = file_write_request(None);
        assert!(matches!(
            policy.check(&req).await,
            PermissionDecision::Allow
        ));
    }
}
