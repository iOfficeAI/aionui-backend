use crate::client::ServerEvent;

/// Lifecycle state of the application.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppState {
    Connecting,
    Idle,
    Sending,
    Streaming,
}

/// Role of a chat message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageRole {
    User,
    Assistant,
    System,
}

/// A single chat message.
#[derive(Debug, Clone)]
pub struct ChatMessage {
    pub role: MessageRole,
    pub content: String,
}

/// Main application state.
pub struct App {
    pub state: AppState,
    pub messages: Vec<ChatMessage>,
    pub input: String,
    pub cursor_pos: usize,
    pub conversation_id: Option<String>,
    pub should_quit: bool,
    pub agent_type: String,
    pub model: Option<String>,
}

impl App {
    pub fn new(agent_type: String, model: Option<String>) -> Self {
        Self {
            state: AppState::Connecting,
            messages: Vec::new(),
            input: String::new(),
            cursor_pos: 0,
            conversation_id: None,
            should_quit: false,
            agent_type,
            model,
        }
    }

    /// Handle a server-side event from the WebSocket.
    pub fn handle_server_event(&mut self, event: ServerEvent) {
        match event {
            ServerEvent::Connected => {
                self.state = AppState::Idle;
            }
            ServerEvent::StreamText { content } => {
                self.state = AppState::Streaming;
                if let Some(last) = self.messages.last_mut() {
                    if last.role == MessageRole::Assistant {
                        last.content.push_str(&content);
                        return;
                    }
                }
                self.messages.push(ChatMessage {
                    role: MessageRole::Assistant,
                    content,
                });
            }
            ServerEvent::StreamThinking { .. } => {
                self.state = AppState::Streaming;
            }
            ServerEvent::StreamFinish => {
                self.state = AppState::Idle;
            }
            ServerEvent::StreamError { message } => {
                self.messages.push(ChatMessage {
                    role: MessageRole::System,
                    content: format!("[Error] {message}"),
                });
                self.state = AppState::Idle;
            }
            ServerEvent::Disconnected => {
                self.messages.push(ChatMessage {
                    role: MessageRole::System,
                    content: "[Disconnected]".to_string(),
                });
                self.should_quit = true;
            }
        }
    }

    /// Take the current input, push it as a user message, and return the text to send.
    pub fn submit_input(&mut self) -> Option<String> {
        let text = self.input.trim().to_string();
        if text.is_empty() {
            return None;
        }
        self.input.clear();
        self.cursor_pos = 0;
        self.messages.push(ChatMessage {
            role: MessageRole::User,
            content: text.clone(),
        });
        self.state = AppState::Sending;
        Some(text)
    }

    /// Insert a character at the cursor position.
    pub fn insert_char(&mut self, c: char) {
        self.input.insert(self.cursor_pos, c);
        self.cursor_pos += c.len_utf8();
    }

    /// Delete the character before the cursor.
    pub fn delete_char(&mut self) {
        if self.cursor_pos > 0 {
            let prev = self.input[..self.cursor_pos]
                .char_indices()
                .next_back()
                .map(|(i, _)| i)
                .unwrap_or(0);
            self.input.drain(prev..self.cursor_pos);
            self.cursor_pos = prev;
        }
    }

    /// Move cursor left.
    pub fn move_cursor_left(&mut self) {
        if self.cursor_pos > 0 {
            self.cursor_pos = self.input[..self.cursor_pos]
                .char_indices()
                .next_back()
                .map(|(i, _)| i)
                .unwrap_or(0);
        }
    }

    /// Move cursor right.
    pub fn move_cursor_right(&mut self) {
        if self.cursor_pos < self.input.len() {
            self.cursor_pos = self.input[self.cursor_pos..]
                .char_indices()
                .nth(1)
                .map(|(i, _)| self.cursor_pos + i)
                .unwrap_or(self.input.len());
        }
    }

    /// Clear the entire input line.
    pub fn clear_input(&mut self) {
        self.input.clear();
        self.cursor_pos = 0;
    }
}
