//! Classify fatal agent stream events into recoverable (rate-limited) vs crash.
//!
//! Paired with W4-D20a `detect_crash`. Rate-limit errors are surfaced as
//! `TeammateStatus::Failed` without going through crash recovery (no kill,
//! no testament) — see interface-contracts §23.

use aionui_ai_agent::stream_event::AgentStreamEvent;
use regex::Regex;
use std::sync::OnceLock;

fn rate_limit_regex() -> &'static Regex {
    static REGEX: OnceLock<Regex> = OnceLock::new();
    REGEX.get_or_init(|| {
        Regex::new(r"(?i)429|rate.?limit|quota|too many requests")
            .expect("rate-limit regex must compile")
    })
}

/// Returns true when an [`AgentStreamEvent::Error`] message looks like an
/// upstream rate-limit / quota response.
pub fn is_rate_limited(event: &AgentStreamEvent) -> bool {
    match event {
        AgentStreamEvent::Error(data) => rate_limit_regex().is_match(&data.message),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aionui_ai_agent::stream_event::{ErrorEventData, StartEventData};

    fn error_event(message: &str) -> AgentStreamEvent {
        AgentStreamEvent::Error(ErrorEventData {
            message: message.to_string(),
            code: None,
        })
    }

    #[test]
    fn http_429_is_rate_limited() {
        assert!(is_rate_limited(&error_event("HTTP 429 Too Many Requests")));
    }

    #[test]
    fn rate_limit_phrase_is_rate_limited() {
        assert!(is_rate_limited(&error_event(
            "Anthropic API: rate limit exceeded, retry later"
        )));
    }

    #[test]
    fn plain_error_is_not_rate_limited() {
        assert!(!is_rate_limited(&error_event("syntax error at line 42")));
    }

    #[test]
    fn non_error_event_is_not_rate_limited() {
        assert!(!is_rate_limited(&AgentStreamEvent::Start(
            StartEventData::default()
        )));
    }
}
