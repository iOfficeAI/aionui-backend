use crate::shared_kernel::{ConfigKey, ConfigValue, ModeId};

/// Actions the session driver must execute to align CLI state with user intent.
///
/// Produced by `AcpSession::plan_reconcile` — a pure function that compares
/// desired vs observed and returns a list of idempotent, order-independent ops.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReconcileAction {
    SetMode { mode: ModeId },
    SetConfigOption { key: ConfigKey, value: ConfigValue },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reconcile_action_equality() {
        let a = ReconcileAction::SetMode {
            mode: ModeId::new("plan"),
        };
        let b = ReconcileAction::SetMode {
            mode: ModeId::new("plan"),
        };
        assert_eq!(a, b);
    }
}
