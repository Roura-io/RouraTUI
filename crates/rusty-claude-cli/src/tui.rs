use std::io;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use ratatui_core::layout::{Constraint, Direction, Layout, Rect};
use ratatui_core::style::{Color, Modifier, Style};
use ratatui_core::terminal::Terminal;
use ratatui_core::text::{Line, Span, Text};
use ratatui_crossterm::crossterm;
use ratatui_crossterm::crossterm::event::{
    self, Event, KeyCode, KeyEvent, KeyModifiers, MouseEventKind,
};
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
const SPINNER: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

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
    Tool,
}

pub enum TurnEvent {
    TextDelta(String),
    ToolCall {
        name: String,
        detail: String,
    },
    ToolsFinished,
    ApprovalRequested {
        tool_name: String,
        detail: String,
        required_mode: String,
        reason: Option<String>,
        response: mpsc::SyncSender<bool>,
    },
}

struct ApprovalCard {
    tool_name: String,
    detail: String,
    required_mode: String,
    reason: Option<String>,
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
    spinner_phase: usize,
    should_quit: bool,
    approval: Option<ApprovalCard>,
    composer_focused: bool,
}

impl App<'_> {
    fn new(config: TuiConfig) -> Self {
        let mut composer = TextArea::default();
        composer.set_placeholder_text("Ask RouraTUI anything…");
        composer.set_cursor_line_style(Style::default().add_modifier(Modifier::UNDERLINED));
        composer.set_cursor_style(Style::default().fg(Color::Black).bg(CORAL));
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
            spinner_phase: 0,
            should_quit: false,
            approval: None,
            composer_focused: true,
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
        self.messages.push(ChatMessage {
            label: self.config.agent.clone(),
            text: String::new(),
            role: MessageRole::Agent,
        });
        self.composer = TextArea::default();
        self.composer.set_placeholder_text("Ask RouraTUI anything…");
        self.composer
            .set_cursor_line_style(Style::default().add_modifier(Modifier::UNDERLINED));
        self.composer
            .set_cursor_style(Style::default().fg(Color::Black).bg(CORAL));
        self.composer.set_style(Style::default().fg(TEXT));
        self.composer
            .set_placeholder_style(Style::default().fg(FAINT));
        self.composer.set_block(composer_block(true));
        self.status = format!("{} is thinking", self.config.agent);
        Some(input)
    }

    fn finish_turn(&mut self, result: Result<String, String>) {
        match result {
            Ok(text) => {
                if let Some(message) = self.messages.last_mut() {
                    if message.role == MessageRole::Agent && message.text.is_empty() {
                        message.text = text;
                    }
                }
            }
            Err(error) => {
                if self.messages.last().is_some_and(|message| {
                    message.role == MessageRole::Agent && message.text.is_empty()
                }) {
                    self.messages.pop();
                }
                self.messages.push(ChatMessage {
                    label: "Error".to_string(),
                    text: error,
                    role: MessageRole::System,
                });
            }
        }
        self.status = "ready".to_string();
        self.composer.set_block(composer_block(false));
        self.scroll = u16::MAX;
    }

    fn append_stream_delta(&mut self, delta: &str) {
        if self.messages.last().map(|message| message.role) != Some(MessageRole::Agent) {
            self.messages.push(ChatMessage {
                label: self.config.agent.clone(),
                text: String::new(),
                role: MessageRole::Agent,
            });
        }
        if let Some(message) = self.messages.last_mut() {
            message.text.push_str(delta);
        }
        self.scroll = u16::MAX;
    }

    fn handle_turn_event(&mut self, event: TurnEvent) -> Option<mpsc::SyncSender<bool>> {
        match event {
            TurnEvent::TextDelta(delta) => self.append_stream_delta(&delta),
            TurnEvent::ToolCall { name, detail } => {
                self.messages.push(ChatMessage {
                    label: format!("Tool · {name}"),
                    text: detail,
                    role: MessageRole::Tool,
                });
                self.status = format!("running {name}");
                self.scroll = u16::MAX;
            }
            TurnEvent::ToolsFinished => {
                for message in &mut self.messages {
                    if message.role == MessageRole::Tool && !message.label.ends_with(" ✓") {
                        message.label.push_str(" ✓");
                    }
                }
                self.status = format!("{} is thinking", self.config.agent);
            }
            TurnEvent::ApprovalRequested {
                tool_name,
                detail,
                required_mode,
                reason,
                response,
            } => {
                self.status = format!("approval required · {tool_name}");
                self.approval = Some(ApprovalCard {
                    tool_name,
                    detail,
                    required_mode,
                    reason,
                });
                return Some(response);
            }
        }
        None
    }

    fn transcript(&self) -> Text<'static> {
        let mut lines = Vec::new();
        for message in &self.messages {
            let color = match message.role {
                MessageRole::User => USER,
                MessageRole::Agent => CORAL,
                MessageRole::System => FAINT,
                MessageRole::Tool => Color::Rgb(217, 179, 87),
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
        .padding(Padding::horizontal(2))
}

pub fn run<F>(config: TuiConfig, mut perform_turn: F) -> io::Result<()>
where
    F: FnMut(&str, mpsc::Sender<TurnEvent>) -> Result<String, String>,
{
    let mut terminal = TerminalSession::enter()?;
    let mut app = App::new(config);

    while !app.should_quit {
        terminal.draw(|frame| draw(frame, &mut app))?;
        if !event::poll(Duration::from_millis(250))? {
            continue;
        }
        let event = event::read()?;
        if let Event::Mouse(mouse) = event {
            if matches!(mouse.kind, MouseEventKind::Down(_)) {
                app.composer_focused = mouse.row >= terminal.size()?.height.saturating_sub(6);
            }
            continue;
        }
        let Event::Key(key) = event else { continue };
        if key.kind != event::KeyEventKind::Press {
            continue;
        }
        if is_submit(key) {
            if let Some(input) = app.submit() {
                terminal.draw(|frame| draw(frame, &mut app))?;
                let (next_terminal, next_app) = thread::scope(|scope| {
                    let (turn_tx, turn_rx) = mpsc::channel::<TurnEvent>();
                    let (result_tx, result_rx) = mpsc::channel::<Result<String, String>>();
                    let renderer = scope.spawn(move || -> io::Result<_> {
                        loop {
                            let mut changed = false;
                            if let Ok(event) = turn_rx.recv_timeout(Duration::from_millis(50)) {
                                if let Some(response) = app.handle_turn_event(event) {
                                    terminal.draw(|frame| draw(frame, &mut app))?;
                                    let allowed = read_approval_decision()?;
                                    let _ = response.send(allowed);
                                    let tool_name = app.approval.as_ref().map_or_else(
                                        || "tool".to_string(),
                                        |approval| approval.tool_name.clone(),
                                    );
                                    app.messages.push(ChatMessage {
                                        label: "Approval".to_string(),
                                        text: format!(
                                            "{tool_name} · {}",
                                            if allowed { "allowed" } else { "denied" }
                                        ),
                                        role: MessageRole::System,
                                    });
                                    app.approval = None;
                                    app.status = if allowed {
                                        format!("running {tool_name}")
                                    } else {
                                        format!("{tool_name} denied")
                                    };
                                }
                                changed = true;
                            }
                            if let Ok(result) = result_rx.try_recv() {
                                app.finish_turn(result);
                                terminal.draw(|frame| draw(frame, &mut app))?;
                                return Ok((terminal, app));
                            }
                            // Keep the TUI alive at 20 FPS while the model is thinking so
                            // the loading indicator visibly animates even without events.
                            let _ = changed;
                            terminal.draw(|frame| draw(frame, &mut app))?;
                        }
                    });
                    let result = perform_turn(&input, turn_tx);
                    let _ = result_tx.send(result);
                    renderer
                        .join()
                        .map_err(|_| io::Error::other("stream renderer thread panicked"))?
                })?;
                terminal = next_terminal;
                app = next_app;
            }
        } else if is_quit(key) {
            app.should_quit = true;
        } else if key.code == KeyCode::PageUp {
            app.scroll = app.scroll.saturating_sub(5);
        } else if key.code == KeyCode::PageDown {
            app.scroll = app.scroll.saturating_add(5);
        } else {
            app.composer_focused = true;
            app.composer.input(Input::from(key));
        }
    }
    Ok(())
}

fn read_approval_decision() -> io::Result<bool> {
    loop {
        if !event::poll(Duration::from_millis(250))? {
            continue;
        }
        let Event::Key(key) = event::read()? else {
            continue;
        };
        if key.kind != event::KeyEventKind::Press {
            continue;
        }
        match key.code {
            KeyCode::Char('y') | KeyCode::Char('Y') | KeyCode::Enter => return Ok(true),
            KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => return Ok(false),
            _ => {}
        }
    }
}

fn draw(frame: &mut ratatui_core::terminal::Frame<'_>, app: &mut App<'_>) {
    app.spinner_phase = app.spinner_phase.wrapping_add(1);
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
    if let Some(approval) = &app.approval {
        let reason = approval
            .reason
            .as_deref()
            .unwrap_or("No additional reason provided");
        let card = Paragraph::new(vec![
            Line::from(vec![
                Span::styled("Tool  ", Style::default().fg(FAINT)),
                Span::styled(approval.tool_name.clone(), Style::default().fg(TEXT)),
                Span::styled(
                    format!("  · requires {}", approval.required_mode),
                    Style::default().fg(FAINT),
                ),
            ]),
            Line::from(approval.detail.clone()),
            Line::from(vec![
                Span::styled(reason.to_string(), Style::default().fg(FAINT)),
                Span::styled("    Y/Enter allow · N/Esc deny", Style::default().fg(CORAL)),
            ]),
        ])
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(CORAL))
                .title(" Approval required ")
                .padding(Padding::horizontal(1)),
        )
        .wrap(Wrap { trim: true });
        frame.render_widget(card, areas[2]);
    } else {
        app.composer.set_cursor_style(if app.composer_focused {
            Style::default().fg(Color::Black).bg(CORAL)
        } else {
            Style::default().fg(Color::Black).bg(Color::Black)
        });
        frame.render_widget(&app.composer, areas[2]);
        let empty = app.composer.lines().iter().all(|line| line.is_empty());
        let prompt = if empty && app.composer_focused {
            "❯ ▌"
        } else {
            "❯"
        };
        let caret = Paragraph::new(Line::from(Span::styled(
            prompt,
            Style::default().fg(CORAL).add_modifier(Modifier::BOLD),
        )));
        frame.render_widget(
            caret,
            Rect {
                x: areas[2].x + 2,
                y: areas[2].y + 1,
                width: 3,
                height: 1,
            },
        );
    }
    let spinner = SPINNER[app.spinner_phase % SPINNER.len()];
    let footer = Line::from(vec![
        Span::styled(
            if app.status == "ready" {
                " Ready  "
            } else {
                " Thinking  "
            },
            Style::default()
                .fg(if app.status == "ready" { FAINT } else { CORAL })
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            if app.status == "ready" { "" } else { spinner },
            Style::default().fg(CORAL),
        ),
        Span::styled(" · ", Style::default().fg(FAINT)),
        Span::styled(" · ", Style::default().fg(FAINT)),
        Span::styled(
            app.config.permission_mode.clone(),
            Style::default().fg(FAINT),
        ),
        Span::styled(" · ", Style::default().fg(FAINT)),
        Span::styled(app.config.branch.clone(), Style::default().fg(FAINT)),
        Span::styled(" · ", Style::default().fg(FAINT)),
        Span::styled(
            if app.status == "ready" {
                "ready".to_string()
            } else {
                app.status.clone()
            },
            Style::default().fg(if app.status == "ready" { FAINT } else { CORAL }),
        ),
    ]);
    frame.render_widget(Paragraph::new(footer), areas[3]);
}

fn draw_header(frame: &mut ratatui_core::terminal::Frame<'_>, area: Rect, app: &App<'_>) {
    let model_state = if app.status == "ready" {
        "ready".to_string()
    } else {
        "thinking".to_string()
    };
    let header = vec![
        Line::from(vec![
            Span::styled(
                " ✻ rouraTUI Code ",
                Style::default().fg(CORAL).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("v{}", app.config.version),
                Style::default().fg(FAINT),
            ),
            Span::styled("  ·  local workspace agent", Style::default().fg(FAINT)),
        ]),
        Line::from(vec![
            Span::styled(" Model  ", Style::default().fg(FAINT)),
            Span::styled(
                app.config.agent.clone(),
                Style::default().fg(TEXT).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("  ·  {}", model_state),
                Style::default().fg(if app.status == "ready" { FAINT } else { CORAL }),
            ),
            Span::styled(
                format!(
                    "    ·  {}  ·  {}  ·  {}",
                    app.config.permission_mode, app.config.branch, "session active"
                ),
                Style::default().fg(FAINT),
            ),
        ]),
        Line::from(Span::styled(
            " Enter sends · Shift-Enter/Ctrl-J adds a line · PageUp/PageDown scroll · Ctrl-C exits",
            Style::default().fg(FAINT),
        )),
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
    use super::{is_quit, is_submit, App, MessageRole, TuiConfig, TurnEvent};
    use ratatui_crossterm::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use std::sync::mpsc;

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

    #[test]
    fn streaming_deltas_append_to_agent_label() {
        let mut app = App::new(TuiConfig {
            version: "test".to_string(),
            agent: "RIO Agent".to_string(),
            permission_mode: "workspace-write".to_string(),
            branch: "dev".to_string(),
        });
        app.composer.insert_str("hello");
        assert!(app.submit().is_some());
        app.append_stream_delta("Hello");
        app.append_stream_delta(" there");
        let message = app.messages.last().expect("agent message");
        assert_eq!(message.role, MessageRole::Agent);
        assert_eq!(message.label, "RIO Agent");
        assert_eq!(message.text, "Hello there");
    }

    #[test]
    fn tool_activity_becomes_a_completed_card() {
        let mut app = App::new(TuiConfig {
            version: "test".to_string(),
            agent: "RIO Agent".to_string(),
            permission_mode: "workspace-write".to_string(),
            branch: "dev".to_string(),
        });
        app.handle_turn_event(TurnEvent::ToolCall {
            name: "read_file".to_string(),
            detail: "README.md".to_string(),
        });
        assert_eq!(app.status, "running read_file");
        app.handle_turn_event(TurnEvent::ToolsFinished);
        let card = app.messages.last().expect("tool card");
        assert_eq!(card.role, MessageRole::Tool);
        assert_eq!(card.label, "Tool · read_file ✓");
        assert_eq!(card.text, "README.md");
    }

    #[test]
    fn approval_event_returns_a_decision_channel() {
        let mut app = App::new(TuiConfig {
            version: "test".to_string(),
            agent: "RIO Agent".to_string(),
            permission_mode: "workspace-write".to_string(),
            branch: "dev".to_string(),
        });
        let (response, decision) = mpsc::sync_channel(1);
        let returned = app.handle_turn_event(TurnEvent::ApprovalRequested {
            tool_name: "shell".to_string(),
            detail: "git push".to_string(),
            required_mode: "danger-full-access".to_string(),
            reason: Some("network access".to_string()),
            response,
        });
        returned.expect("approval response").send(false).unwrap();
        assert!(!decision.recv().unwrap());
        assert!(app.approval.is_some());
        assert_eq!(app.status, "approval required · shell");
    }
}
