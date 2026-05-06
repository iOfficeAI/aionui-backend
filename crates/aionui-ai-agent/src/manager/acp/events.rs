use std::collections::HashMap;

/// Domain events emitted by the `AcpSession` aggregate.
///
/// These capture *intent* changes (user wants mode X) and *observation*
/// arrivals (CLI reported mode Y) separately — persistence consumers can
/// decide which to write to DB without re-interpreting UI stream events.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AcpSessionEvent {
    SessionAssigned { session_id: String },
    SessionOpened,
    DesiredModeChanged { mode_id: String },
    DesiredConfigChanged { selections: HashMap<String, String> },
    ObservedModeSynced { mode_id: String },
    ObservedModelSynced { model_id: String },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_equality() {
        let a = AcpSessionEvent::SessionAssigned {
            session_id: "s1".into(),
        };
        let b = AcpSessionEvent::SessionAssigned {
            session_id: "s1".into(),
        };
        assert_eq!(a, b);
    }

    #[test]
    fn event_debug_format() {
        let e = AcpSessionEvent::DesiredModeChanged { mode_id: "plan".into() };
        let dbg = format!("{e:?}");
        assert!(dbg.contains("plan"));
    }
}
