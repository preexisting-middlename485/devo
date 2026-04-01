use serde::{Deserialize, Serialize};

/// The mode controlling how the agent handles permission checks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PermissionMode {
    /// Approve every request without asking.
    AutoApprove,
    /// Ask the user for confirmation on each request.
    Interactive,
    /// Deny all requests that require permission.
    Deny,
}

/// What kind of resource a tool wants to access.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ResourceKind {
    FileRead,
    FileWrite,
    ShellExec,
    Network,
    Custom(String),
}

/// A permission check request emitted by the tool system.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermissionRequest {
    pub tool_name: String,
    pub resource: ResourceKind,
    /// Free-form description of what is being accessed.
    pub description: String,
    /// Optional path or command being accessed.
    pub target: Option<String>,
}

/// The result of a permission check.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PermissionDecision {
    Allow,
    Deny { reason: String },
    Ask { message: String },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn permission_mode_serde_roundtrip() {
        for mode in [
            PermissionMode::AutoApprove,
            PermissionMode::Interactive,
            PermissionMode::Deny,
        ] {
            let json = serde_json::to_string(&mode).unwrap();
            let deserialized: PermissionMode = serde_json::from_str(&json).unwrap();
            assert_eq!(deserialized, mode);
        }
    }

    #[test]
    fn resource_kind_equality() {
        assert_eq!(ResourceKind::FileRead, ResourceKind::FileRead);
        assert_ne!(ResourceKind::FileRead, ResourceKind::FileWrite);
        assert_eq!(
            ResourceKind::Custom("x".into()),
            ResourceKind::Custom("x".into())
        );
        assert_ne!(
            ResourceKind::Custom("x".into()),
            ResourceKind::Custom("y".into())
        );
    }

    #[test]
    fn permission_request_serde() {
        let req = PermissionRequest {
            tool_name: "bash".into(),
            resource: ResourceKind::ShellExec,
            description: "run ls".into(),
            target: Some("ls -la".into()),
        };
        let json = serde_json::to_string(&req).unwrap();
        let deserialized: PermissionRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.tool_name, "bash");
        assert_eq!(deserialized.resource, ResourceKind::ShellExec);
        assert_eq!(deserialized.target, Some("ls -la".into()));
    }

    #[test]
    fn permission_decision_serde() {
        let decision = PermissionDecision::Deny {
            reason: "no way".into(),
        };
        let json = serde_json::to_string(&decision).unwrap();
        let deserialized: PermissionDecision = serde_json::from_str(&json).unwrap();
        match deserialized {
            PermissionDecision::Deny { reason } => assert_eq!(reason, "no way"),
            _ => panic!("expected Deny"),
        }
    }
}
