use std::time::Duration;

// ---------------------------------------------------------------------------
// Pairing
// ---------------------------------------------------------------------------

/// Length of the numeric pairing code (6 digits).
pub const PAIRING_CODE_LENGTH: usize = 6;

/// How long a pairing code remains valid.
pub const PAIRING_CODE_TTL: Duration = Duration::from_secs(10 * 60);

/// Interval between expired-pairing cleanup sweeps.
pub const PAIRING_CLEANUP_INTERVAL: Duration = Duration::from_secs(60);

// ---------------------------------------------------------------------------
// Streaming & Throttle
// ---------------------------------------------------------------------------

/// Minimum interval between consecutive `editMessage` calls for
/// streaming responses (prevents API rate-limit errors).
pub const STREAM_THROTTLE_INTERVAL: Duration = Duration::from_millis(500);

/// Timeout for tool confirmation from the IM user.
pub const TOOL_CONFIRM_TIMEOUT: Duration = Duration::from_secs(15);

// ---------------------------------------------------------------------------
// Platform message limits
// ---------------------------------------------------------------------------

/// Maximum characters per Telegram message.
pub const TELEGRAM_MESSAGE_LIMIT: usize = 4096;

/// Maximum characters per Lark (Feishu) message.
pub const LARK_MESSAGE_LIMIT: usize = 4000;

/// Maximum characters per DingTalk message.
pub const DINGTALK_MESSAGE_LIMIT: usize = 4000;

// ---------------------------------------------------------------------------
// Reconnection (Telegram long-polling)
// ---------------------------------------------------------------------------

/// Maximum reconnection attempts for Telegram long-polling.
pub const TELEGRAM_MAX_RECONNECT_ATTEMPTS: u32 = 10;

/// Maximum delay between reconnection attempts (exponential backoff cap).
pub const TELEGRAM_MAX_RECONNECT_DELAY: Duration = Duration::from_secs(30);

// ---------------------------------------------------------------------------
// Lark
// ---------------------------------------------------------------------------

/// TTL for Lark event deduplication cache.
pub const LARK_EVENT_DEDUP_TTL: Duration = Duration::from_secs(5 * 60);

// ---------------------------------------------------------------------------
// WeChat (iLink Bot)
// ---------------------------------------------------------------------------

/// Response timeout for WeChat message processing.
pub const WEIXIN_RESPONSE_TIMEOUT: Duration = Duration::from_secs(5 * 60);

/// Maximum file size for WeChat file handling (200 MB).
pub const WEIXIN_MAX_FILE_SIZE: u64 = 200 * 1024 * 1024;

/// Maximum consecutive failures before WeChat stops retrying.
pub const WEIXIN_MAX_RETRIES: u32 = 3;

/// Backoff delay between WeChat retry attempts.
pub const WEIXIN_RETRY_DELAY: Duration = Duration::from_secs(30);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pairing_code_length_is_six() {
        assert_eq!(PAIRING_CODE_LENGTH, 6);
    }

    #[test]
    fn pairing_code_ttl_is_ten_minutes() {
        assert_eq!(PAIRING_CODE_TTL, Duration::from_secs(600));
    }

    #[test]
    fn cleanup_interval_is_sixty_seconds() {
        assert_eq!(PAIRING_CLEANUP_INTERVAL, Duration::from_secs(60));
    }

    #[test]
    fn stream_throttle_is_500ms() {
        assert_eq!(STREAM_THROTTLE_INTERVAL, Duration::from_millis(500));
    }

    #[test]
    fn tool_confirm_timeout_is_15s() {
        assert_eq!(TOOL_CONFIRM_TIMEOUT, Duration::from_secs(15));
    }

    #[test]
    fn telegram_message_limit() {
        assert_eq!(TELEGRAM_MESSAGE_LIMIT, 4096);
    }

    #[test]
    fn lark_message_limit() {
        assert_eq!(LARK_MESSAGE_LIMIT, 4000);
    }

    #[test]
    fn dingtalk_message_limit() {
        assert_eq!(DINGTALK_MESSAGE_LIMIT, 4000);
    }

    #[test]
    fn telegram_reconnect_limits() {
        assert_eq!(TELEGRAM_MAX_RECONNECT_ATTEMPTS, 10);
        assert_eq!(TELEGRAM_MAX_RECONNECT_DELAY, Duration::from_secs(30));
    }

    #[test]
    fn lark_event_dedup_ttl_is_five_minutes() {
        assert_eq!(LARK_EVENT_DEDUP_TTL, Duration::from_secs(300));
    }

    #[test]
    fn weixin_constants() {
        assert_eq!(WEIXIN_RESPONSE_TIMEOUT, Duration::from_secs(300));
        assert_eq!(WEIXIN_MAX_FILE_SIZE, 200 * 1024 * 1024);
        assert_eq!(WEIXIN_MAX_RETRIES, 3);
        assert_eq!(WEIXIN_RETRY_DELAY, Duration::from_secs(30));
    }
}
