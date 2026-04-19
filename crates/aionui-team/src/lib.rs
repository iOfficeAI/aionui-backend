pub mod error;
pub mod mailbox;
pub mod task_board;
#[cfg(test)]
pub(crate) mod test_utils;
pub mod types;

pub use error::TeamError;
pub use mailbox::Mailbox;
pub use task_board::{TaskBoard, TaskUpdate};
pub use types::{
    MailboxMessage, MailboxMessageType, TaskStatus, Team, TeamAgent, TeamTask, TeammateRole,
    TeammateStatus,
};
