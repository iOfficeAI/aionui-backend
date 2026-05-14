use std::time::Duration;

use crossterm::event::{self, Event};
use tokio::sync::mpsc;

/// Spawn a dedicated thread that polls terminal events and forwards them.
pub fn spawn_terminal_event_reader() -> mpsc::UnboundedReceiver<Event> {
    let (tx, rx) = mpsc::unbounded_channel();

    std::thread::spawn(move || loop {
        if event::poll(Duration::from_millis(50)).unwrap_or(false) {
            if let Ok(ev) = event::read() {
                if tx.send(ev).is_err() {
                    break;
                }
            }
        }
    });

    rx
}
