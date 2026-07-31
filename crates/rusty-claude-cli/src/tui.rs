use std::io;
use std::time::Duration;

use ratatui_core::layout::{Constraint, Direction, Layout, Rect};
use ratatui_core::style::{Color, Modifier, Style};
use ratatui_core::terminal::Terminal;
use ratatui_core::text::{Line, Span, Text};
use ratatui_crossterm::crossterm;
use ratatui_crossterm::crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers};
use ratatui_crossterm::crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui_crossterm::CrosstermBackend;
use ratatui_textarea::{Input, Key, TextArea};
use ratatui_widgets::block::{Block, Padding};
use ratatui_widgets::borders::Borders;
use ratatui_widgets::paragraph::{Paragraph, Wrap};
use ratatui_widgets::scrollbar::{Scrollbar, ScrollbarOrientation, ScrollbarState};

const CORAL: Color = Color::Rgb(217, 119, 87);
const FAINT: Color = Color::Rgb(100, 107, 121);
const TEXT: Color = Color::Rgb(236, 236, 236);
const USER: Color = Color::Rgb(138, 180, 248);

#[derive(Debug, Clone, PartialEq, Eq)]
struct ChatMessage {
    label: String,
    text: String,
    role: MessageRole,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MessageRole {
    User,
    Agent,
    System,
}

pub struct TuiConfig {
    pub version: String,
    pub agent: String,
    pub permission_mode: String,
    pub branch: String,
}

struct App<'a> {
    config: TuiConfig,
    composer: TextArea<'a>,
    messages: Vec<ChatMessage>,
    scroll: u16,
    status: String,
    should_quit: bool,
}

impl App<'_> {
    fn new(config: TuiConfig) -> Self {
        let mut composer = TextArea::default();
        composer.set_placeholder_text("Ask RouraTUI anything…");
        composer.set_cursor_line_style(Style::default());
        composer.set_cursor_style(Style::default().fg(CORAL));
        composer.set_style(Style::default().fg(TEXT));
        composer.set_placeholder_style(Style::default().fg(FAINT));
        composer.set_block(composer_block(false));
        Self {
            config,
            composer,
            messages: vec![ChatMessage {
                label: "RouraTUI".to_string(),
                text: "Ready. Your composer stays anchored here while the conversation scrolls above it.".to_string(),
                role: MessageRole::System,
            }],
            scroll: 0,
            status: "ready".to_string(),
            should_quit: false,
        }
    }

    fn submit(&mut self) -> Option<String> {
        let input = self.composer.lines().join("\n");
        if input.trim().is_empty() {
            return None;
        }
        if matches!(input.trim(), "/exit" | "/quit") {
            self.should_quit = true;
            return None;
        }
        self.messages.push(ChatMessage {
            label: "You".to_string(),
            text: input.trim().to_string(),
            role: MessageRole::User,
        });
        self.composer = TextArea::default();
        self.composer.set_placeholder_text("Ask RouraTUI anything…");
        self.composer.set_cursor_line_style(Style::default());
        self.composer.set_cursor_style(Style::default().fg(CORAL));
        self.composer.set_style(Style::default().fg(TEXT));
        self.composer
            .set_placeholder_style(Style::default().fg(FAINT));
        self.composer.set_block(composer_block(true));
        self.status = format!("{} is thinking", self.config.agent);
        Some(input)
    }

    fn finish_turn(&mut self, result: Result<String, String>) {
        match result {
            Ok(text) => self.messages.push(ChatMessage {
                label: self.config.agent.clone(),
                text,
                role: MessageRole::Agent,
            }),
            Err(error) => self.messages.push(ChatMessage {
                label: "Error".to_string(),
                text: error,
                role: MessageRole::System,
            }),
        }
        self.status = "ready".to_string();
        self.composer.set_block(composer_block(false));
        self.scroll = u16::MAX;
    }

    fn transcript(&self) -> Text<'static> {
        let mut lines = Vec::new();
        for message in &self.messages {
            let color = match message.role {
                MessageRole::User => USER,
                MessageRole::Agent => CORAL,
                MessageRole::System => FAINT,
            };
            lines.push(Line::from(Span::styled(
                message.label.clone(),
                Style::default().fg(color).add_modifier(Modifier::BOLD),
            )));
            lines.extend(
                message
                    .text
                    .lines()
                    .map(|line| Line::from(line.to_string())),
            );
            lines.push(Line::default());
        }
        Text::from(lines)
    }
}

fn composer_block(busy: bool) -> Block<'static> {
    Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(if busy { FAINT } else { CORAL }))
        .title(Span::styled(
            " ❯ ",
            Style::default().fg(CORAL).add_modifier(Modifier::BOLD),
        ))
        .padding(Padding::horizontal(1))
}

pub fn run<F>(config: TuiConfig, mut perform_turn: F) -> io::Result<()>
where
    F: FnMut(&str) -> Result<String, String>,
{
    let mut terminal = TerminalSession::enter()?;
    let mut app = App::new(config);

    while !app.should_quit {
        terminal.draw(|frame| draw(frame, &mut app))?;
        if !event::poll(Duration::from_millis(250))? {
            continue;
        }
        let Event::Key(key) = event::read()? else {
            continue;
        };
        if key.kind != event::KeyEventKind::Press {
            continue;
        }
        if is_submit(key) {
            if let Some(input) = app.submit() {
                terminal.draw(|frame| draw(frame, &mut app))?;
                let result = perform_turn(&input);
                app.finish_turn(result);
            }
        } else if is_quit(key) {
            app.should_quit = true;
        } else if key.code == KeyCode::PageUp {
            app.scroll = app.scroll.saturating_sub(5);
        } else if key.code == KeyCode::PageDown {
            app.scroll = app.scroll.saturating_add(5);
        } else {
            app.composer.input(Input::from(key));
        }
    }
    Ok(())
}

fn draw(frame: &mut ratatui_core::terminal::Frame<'_>, app: &mut App<'_>) {
    let areas = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(5),
            Constraint::Length(5),
            Constraint::Length(1),
        ])
        .split(frame.area());

    draw_header(frame, areas[0], app);
    let transcript = app.transcript();
    let transcript_height = transcript.height() as u16;
    let viewport_height = areas[1].height.saturating_sub(2);
    if app.scroll == u16::MAX {
        app.scroll = transcript_height.saturating_sub(viewport_height);
    }
    let paragraph = Paragraph::new(transcript)
        .block(
            Block::default()
                .borders(Borders::BOTTOM)
                .border_style(Style::default().fg(FAINT)),
        )
        .wrap(Wrap { trim: false })
        .scroll((app.scroll, 0));
    frame.render_widget(paragraph, areas[1]);
    let mut scrollbar_state =
        ScrollbarState::new(transcript_height as usize).position(app.scroll as usize);
    frame.render_stateful_widget(
        Scrollbar::new(ScrollbarOrientation::VerticalRight),
        areas[1],
        &mut scrollbar_state,
    );
    frame.render_widget(&app.composer, areas[2]);
    let footer = Line::from(vec![
        Span::styled(
            format!(" {} ", app.config.agent),
            Style::default().fg(FAINT),
        ),
        Span::styled(" · ", Style::default().fg(FAINT)),
        Span::styled(
            app.config.permission_mode.clone(),
            Style::default().fg(FAINT),
        ),
        Span::styled(" · ", Style::default().fg(FAINT)),
        Span::styled(app.config.branch.clone(), Style::default().fg(FAINT)),
        Span::styled(" · ", Style::default().fg(FAINT)),
        Span::styled(app.status.clone(), Style::default().fg(CORAL)),
    ]);
    frame.render_widget(Paragraph::new(footer), areas[3]);
}

fn draw_header(frame: &mut ratatui_core::terminal::Frame<'_>, area: Rect, app: &App<'_>) {
    let header = vec![
        Line::from(vec![
            Span::styled(" ✻ rouraTUI Code ", Style::default().fg(CORAL).add_modifier(Modifier::BOLD)),
            Span::styled(format!("v{}", app.config.version), Style::default().fg(FAINT)),
        ]),
        Line::from(vec![
            Span::styled(" Agent  ", Style::default().fg(FAINT)),
            Span::styled(app.config.agent.clone(), Style::default().fg(TEXT).add_modifier(Modifier::BOLD)),
            Span::styled("    Enter sends · Shift-Enter/Ctrl-J adds a line · PageUp/PageDown scroll · Ctrl-C exits", Style::default().fg(FAINT)),
        ]),
    ];
    frame.render_widget(Paragraph::new(header), area);
}

fn is_submit(key: KeyEvent) -> bool {
    key.code == KeyCode::Enter
        && !key
            .modifiers
            .intersects(KeyModifiers::SHIFT | KeyModifiers::CONTROL)
}

fn is_quit(key: KeyEvent) -> bool {
    key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL)
}

struct TerminalSession {
    terminal: Terminal<CrosstermBackend<io::Stdout>>,
}

impl TerminalSession {
    fn enter() -> io::Result<Self> {
        enable_raw_mode()?;
        let mut stdout = io::stdout();
        crossterm::execute!(stdout, EnterAlternateScreen, event::EnableMouseCapture)?;
        let terminal = Terminal::new(CrosstermBackend::new(stdout))?;
        Ok(Self { terminal })
    }
}

impl std::ops::Deref for TerminalSession {
    type Target = Terminal<CrosstermBackend<io::Stdout>>;
    fn deref(&self) -> &Self::Target {
        &self.terminal
    }
}

impl std::ops::DerefMut for TerminalSession {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.terminal
    }
}

impl Drop for TerminalSession {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = crossterm::execute!(
            self.terminal.backend_mut(),
            LeaveAlternateScreen,
            event::DisableMouseCapture
        );
        let _ = self.terminal.show_cursor();
    }
}

#[cfg(test)]
mod tests {
    use super::{is_quit, is_submit};
    use ratatui_crossterm::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    #[test]
    fn enter_submits_but_modified_enter_does_not() {
        assert!(is_submit(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)));
        assert!(!is_submit(KeyEvent::new(
            KeyCode::Enter,
            KeyModifiers::SHIFT
        )));
        assert!(!is_submit(KeyEvent::new(
            KeyCode::Enter,
            KeyModifiers::CONTROL
        )));
    }

    #[test]
    fn control_c_quits() {
        assert!(is_quit(KeyEvent::new(
            KeyCode::Char('c'),
            KeyModifiers::CONTROL
        )));
    }
}
