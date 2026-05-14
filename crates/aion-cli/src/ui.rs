use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};

use crate::app::{App, AppState, MessageRole};

pub fn render(frame: &mut Frame, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // Status bar
            Constraint::Min(3),    // Messages
            Constraint::Length(3), // Input
        ])
        .split(frame.area());

    render_status_bar(frame, app, chunks[0]);
    render_messages(frame, app, chunks[1]);
    render_input(frame, app, chunks[2]);
}

fn render_status_bar(frame: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    let state_str = match app.state {
        AppState::Connecting => "connecting...",
        AppState::Idle => "idle",
        AppState::Sending => "sending...",
        AppState::Streaming => "streaming...",
    };

    let model_str = app.model.as_deref().unwrap_or("default");

    let left = format!(" [{}·{}]", app.agent_type, model_str);
    let right = format!("[{state_str}] ");
    let total = left.len() + right.len();
    let width = area.width as usize;
    let padding = if width > total {
        " ".repeat(width - total)
    } else {
        String::new()
    };

    let line = Line::from(vec![
        Span::styled(left, Style::default().fg(Color::Cyan)),
        Span::raw(padding),
        Span::styled(right, Style::default().fg(Color::DarkGray)),
    ]);

    let bar = Paragraph::new(line)
        .style(Style::default().bg(Color::DarkGray).fg(Color::White));
    frame.render_widget(bar, area);
}

fn render_messages(frame: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    let mut lines: Vec<Line> = Vec::new();

    for msg in &app.messages {
        let (prefix, style) = match msg.role {
            MessageRole::User => ("You: ", Style::default().fg(Color::Green)),
            MessageRole::Assistant => ("Assistant: ", Style::default().fg(Color::White)),
            MessageRole::System => ("", Style::default().fg(Color::Red)),
        };

        let content = if msg.role == MessageRole::Assistant && app.state == AppState::Streaming {
            format!("{}▌", msg.content)
        } else {
            msg.content.clone()
        };

        lines.push(Line::from(vec![
            Span::styled(prefix, style.add_modifier(Modifier::BOLD)),
            Span::styled(content, style),
        ]));
        lines.push(Line::raw(""));
    }

    if lines.is_empty() {
        lines.push(Line::styled(
            "  Type a message and press Enter to start...",
            Style::default().fg(Color::DarkGray),
        ));
    }

    let visible_height = area.height as usize;
    let total_lines = lines.len();
    let scroll = if total_lines > visible_height {
        (total_lines - visible_height) as u16
    } else {
        0
    };

    let messages = Paragraph::new(lines)
        .block(Block::default().borders(Borders::NONE))
        .wrap(Wrap { trim: false })
        .scroll((scroll, 0));

    frame.render_widget(messages, area);
}

fn render_input(frame: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    let input_text = format!("> {}", app.input);
    let input = Paragraph::new(input_text.as_str())
        .block(Block::default().borders(Borders::TOP))
        .style(Style::default().fg(Color::White));
    frame.render_widget(input, area);

    let cursor_x = area.x + 2 + app.cursor_pos as u16;
    let cursor_y = area.y + 1;
    frame.set_cursor_position((cursor_x, cursor_y));
}
