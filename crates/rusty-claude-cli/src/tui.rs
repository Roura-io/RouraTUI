use std::io;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use runtime::TurnCancelSignal;

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
        /// One plain-English sentence describing what the tool call would
        /// do — already humanized by the caller (main.rs), not raw JSON.
        detail: String,
        response: mpsc::SyncSender<ApprovalDecision>,
    },
}

/// A y/n/a answer to an approval card. `AllowAlways` means "and don't ask
/// about this exact action again this session" (#RTUI-REMEMBER-APPROVAL).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApprovalDecision {
    Allow,
    AllowAlways,
    Deny,
}

struct ApprovalCard {
    tool_name: String,
    detail: String,
}

/// A turn ended without producing a final answer — either it genuinely
/// failed, or the user interrupted it (Esc/Ctrl+C mid-turn). The two are
/// kept distinct so `App::finish_turn` can skip showing an "Error" message
/// over an interrupt: whatever content already streamed to the transcript
/// via `TurnEvent::TextDelta` just stays as it was.
pub struct TurnFailure {
    pub message: String,
    pub interrupted: bool,
}

impl From<Box<dyn std::error::Error>> for TurnFailure {
    fn from(error: Box<dyn std::error::Error>) -> Self {
        Self {
            message: error.to_string(),
            interrupted: false,
        }
    }
}

pub struct TuiConfig {
    pub version: String,
    pub agent: String,
    pub permission_mode: String,
    pub branch: String,
    /// `(/name, one-line summary)` for every slash command, used to build
    /// the "/" autocomplete dropdown. Computed once by the caller (main.rs
    /// already owns `commands::slash_command_specs()`) rather than adding a
    /// `commands` crate dependency here.
    pub slash_commands: Vec<(String, String)>,
    /// Names of skills discoverable from the current workspace, used to
    /// build the bare-word autocomplete dropdown (skills are invoked by
    /// typing their name directly, no leading `/`).
    pub skill_names: Vec<String>,
}

/// One entry in the composer's autocomplete dropdown.
struct Suggestion {
    /// Text that replaces the composer's current (single-line) contents
    /// when this suggestion is accepted.
    insert: String,
    /// What's actually drawn in the dropdown — may include a description
    /// the raw `insert` text doesn't have room for.
    label: String,
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
    /// #RTUI-STICKY-SCROLL: true once the reader has paged up. While set,
    /// streamed tokens no longer yank the view back to the bottom on every
    /// delta — auto-follow only resumes once they page back down to the
    /// bottom, or send a new message.
    user_scrolled: bool,
    composer_focused: bool,
    /// Recomputed on every composer edit (see `refresh_suggestions`); empty
    /// means no dropdown is shown.
    suggestions: Vec<Suggestion>,
    suggestion_index: usize,
}

/// A freshly styled, empty composer — shared by `App::new`, `submit`, and
/// `accept_suggestion` so the placeholder/cursor/style setup lives in one
/// place.
fn composer_textarea<'a>() -> TextArea<'a> {
    let mut composer = TextArea::default();
    composer.set_placeholder_text("Ask RouraTUI anything…");
    composer.set_cursor_line_style(Style::default());
    composer.set_cursor_style(Style::default().fg(Color::Black).bg(CORAL));
    composer.set_style(Style::default().fg(TEXT));
    composer.set_placeholder_style(Style::default().fg(FAINT));
    composer.set_block(composer_block(false));
    composer
}

impl App<'_> {
    fn new(config: TuiConfig) -> Self {
        let composer = composer_textarea();
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
            user_scrolled: false,
            composer_focused: true,
            suggestions: Vec::new(),
            suggestion_index: 0,
        }
    }

    /// Recomputes the autocomplete dropdown from the composer's current
    /// (single-line, not-yet-submitted) contents. Only the leading word is
    /// ever completed — once a space appears the user has moved on to
    /// arguments/a real sentence, so the dropdown gets out of the way.
    fn refresh_suggestions(&mut self) {
        self.suggestions.clear();
        self.suggestion_index = 0;
        if self.composer.lines().len() != 1 {
            return;
        }
        let line = self.composer.lines()[0].as_str();
        if line.is_empty() || line.contains(' ') {
            return;
        }
        if let Some(prefix) = line.strip_prefix('/') {
            for (name, summary) in &self.config.slash_commands {
                let Some(rest) = name.strip_prefix('/') else {
                    continue;
                };
                if !prefix.is_empty() && !rest.starts_with(prefix) {
                    continue;
                }
                self.suggestions.push(Suggestion {
                    insert: format!("{name} "),
                    label: format!("{name} — {summary}"),
                });
            }
        } else {
            for name in &self.config.skill_names {
                if name.starts_with(line) {
                    self.suggestions.push(Suggestion {
                        insert: name.clone(),
                        label: name.clone(),
                    });
                }
            }
        }
        self.suggestions.truncate(8);
    }

    fn select_next_suggestion(&mut self) {
        if !self.suggestions.is_empty() {
            self.suggestion_index = (self.suggestion_index + 1) % self.suggestions.len();
        }
    }

    fn select_previous_suggestion(&mut self) {
        if !self.suggestions.is_empty() {
            self.suggestion_index = self
                .suggestion_index
                .checked_sub(1)
                .unwrap_or(self.suggestions.len() - 1);
        }
    }

    /// Replace the composer's contents with the highlighted suggestion.
    fn accept_suggestion(&mut self) {
        let Some(suggestion) = self.suggestions.get(self.suggestion_index) else {
            return;
        };
        let insert = suggestion.insert.clone();
        self.composer = composer_textarea();
        self.composer.insert_str(&insert);
        self.suggestions.clear();
        self.suggestion_index = 0;
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
        self.suggestions.clear();
        self.suggestion_index = 0;
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
        self.composer = composer_textarea();
        self.composer.set_block(composer_block(true));
        self.status = format!("{} is thinking", self.config.agent);
        // Sending always follows the new turn, even if you'd paged up to
        // reread earlier history.
        self.user_scrolled = false;
        self.scroll = u16::MAX;
        Some(input)
    }

    fn finish_turn(&mut self, result: Result<String, TurnFailure>) {
        match result {
            Ok(text) => {
                if let Some(message) = self.messages.last_mut() {
                    if message.role == MessageRole::Agent && message.text.is_empty() {
                        message.text = text;
                    }
                }
            }
            Err(failure) => {
                let had_partial_content = self.messages.last().is_some_and(|message| {
                    message.role == MessageRole::Agent && !message.text.is_empty()
                });
                if self.messages.last().is_some_and(|message| {
                    message.role == MessageRole::Agent && message.text.is_empty()
                }) {
                    self.messages.pop();
                }
                // An interrupt with partial content already visible needs no
                // extra message — the streamed text stands on its own. An
                // interrupt with nothing streamed yet still gets a quiet
                // marker so the composer doesn't just go silently idle.
                if !failure.interrupted {
                    self.messages.push(ChatMessage {
                        label: "Error".to_string(),
                        text: failure.message,
                        role: MessageRole::System,
                    });
                } else if !had_partial_content {
                    self.messages.push(ChatMessage {
                        label: "Interrupted".to_string(),
                        text: String::new(),
                        role: MessageRole::System,
                    });
                }
            }
        }
        self.status = "ready".to_string();
        self.composer.set_block(composer_block(false));
        if !self.user_scrolled {
            self.scroll = u16::MAX;
        }
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
        // #RTUI-STICKY-SCROLL: only follow the stream if the reader hasn't
        // paged up — otherwise every token during a long answer yanked a
        // deliberate scroll-back straight to the bottom.
        if !self.user_scrolled {
            self.scroll = u16::MAX;
        }
    }

    fn handle_turn_event(
        &mut self,
        event: TurnEvent,
    ) -> Option<mpsc::SyncSender<ApprovalDecision>> {
        match event {
            TurnEvent::TextDelta(delta) => self.append_stream_delta(&delta),
            TurnEvent::ToolCall { name, detail } => {
                self.messages.push(ChatMessage {
                    label: format!("Tool · {name}"),
                    text: detail,
                    role: MessageRole::Tool,
                });
                self.status = format!("running {name}");
                if !self.user_scrolled {
                    self.scroll = u16::MAX;
                }
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
                response,
            } => {
                self.status = format!("approval required · {tool_name}");
                self.approval = Some(ApprovalCard { tool_name, detail });
                return Some(response);
            }
        }
        None
    }

    fn transcript(&self) -> Text<'static> {
        let mut lines = Vec::new();
        let last_index = self.messages.len().saturating_sub(1);
        for (index, message) in self.messages.iter().enumerate() {
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
            // #RTUI-THINKING-BANNER: the empty placeholder pushed by submit()
            // sits right below the message you just sent, waiting for the
            // first streamed token. Show the live spinner right there
            // instead of leaving a blank gap.
            if index == last_index && message.role == MessageRole::Agent && message.text.is_empty()
            {
                let spinner = SPINNER[self.spinner_phase % SPINNER.len()];
                lines.push(Line::from(Span::styled(
                    format!("{spinner} thinking…"),
                    Style::default().fg(FAINT),
                )));
            } else {
                lines.extend(
                    message
                        .text
                        .lines()
                        .map(|line| Line::from(line.to_string())),
                );
            }
            lines.push(Line::default());
        }
        Text::from(lines)
    }
}

/// The branch field outside a git repo (or when detection just fails) reads
/// "unknown" — not a placeholder waiting to resolve, a permanent "not
/// applicable" for this workspace. Hide the field entirely rather than show
/// a value that looks broken.
fn known_branch<'b>(app: &'b App<'_>) -> Option<&'b str> {
    let branch = app.config.branch.trim();
    (!branch.is_empty() && branch != "unknown").then_some(branch)
}

fn composer_block(busy: bool) -> Block<'static> {
    Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(if busy { FAINT } else { CORAL }))
        .padding(Padding::horizontal(3))
}

pub fn run<F>(
    config: TuiConfig,
    cancel_signal: TurnCancelSignal,
    mut perform_turn: F,
) -> io::Result<()>
where
    F: FnMut(&str, mpsc::Sender<TurnEvent>) -> Result<String, TurnFailure>,
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
            // #RTUI-MOUSE-SCROLL: the wheel/trackpad was never wired to
            // scrolling at all — every mouse event past the focus check was
            // silently swallowed, so PageUp/PageDown were the only way to
            // scroll, which most people don't reach for first.
            match mouse.kind {
                MouseEventKind::ScrollUp => {
                    app.scroll = app.scroll.saturating_sub(3);
                    app.user_scrolled = true;
                }
                MouseEventKind::ScrollDown => {
                    app.scroll = app.scroll.saturating_add(3);
                }
                _ => {}
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
                let renderer_cancel_signal = cancel_signal.clone();
                let (next_terminal, next_app) = thread::scope(|scope| {
                    let (turn_tx, turn_rx) = mpsc::channel::<TurnEvent>();
                    let (result_tx, result_rx) = mpsc::channel::<Result<String, TurnFailure>>();
                    let renderer = scope.spawn(move || -> io::Result<_> {
                        loop {
                            let mut changed = false;
                            // #RTUI-SCROLL-DURING-TURN: this loop owns the
                            // terminal for the whole turn (the outer event
                            // loop in `run` is blocked on
                            // `renderer.join()`), so without this,
                            // PageUp/PageDown — and every other key — were
                            // silently dropped from the moment you hit enter
                            // until the turn finished.
                            if event::poll(Duration::from_millis(0))? {
                                match event::read()? {
                                    Event::Key(key) if key.kind == event::KeyEventKind::Press => {
                                        // #RTUI-TURN-INTERRUPT: checked before
                                        // PageUp/PageDown so Esc/Ctrl+C always
                                        // stops the turn even if it happens to
                                        // collide with some future key binding.
                                        if is_interrupt(key) {
                                            renderer_cancel_signal.cancel();
                                            app.status = "stopping…".to_string();
                                            changed = true;
                                        } else {
                                            match key.code {
                                                KeyCode::PageUp => {
                                                    app.scroll = app.scroll.saturating_sub(5);
                                                    app.user_scrolled = true;
                                                    changed = true;
                                                }
                                                KeyCode::PageDown => {
                                                    app.scroll = app.scroll.saturating_add(5);
                                                    changed = true;
                                                }
                                                _ => {}
                                            }
                                        }
                                    }
                                    Event::Mouse(mouse) => match mouse.kind {
                                        MouseEventKind::ScrollUp => {
                                            app.scroll = app.scroll.saturating_sub(3);
                                            app.user_scrolled = true;
                                            changed = true;
                                        }
                                        MouseEventKind::ScrollDown => {
                                            app.scroll = app.scroll.saturating_add(3);
                                            changed = true;
                                        }
                                        _ => {}
                                    },
                                    _ => {}
                                }
                            }
                            if let Ok(event) = turn_rx.recv_timeout(Duration::from_millis(50)) {
                                if let Some(response) = app.handle_turn_event(event) {
                                    terminal.draw(|frame| draw(frame, &mut app))?;
                                    let decision = read_approval_decision()?;
                                    let _ = response.send(decision);
                                    let tool_name = app.approval.as_ref().map_or_else(
                                        || "tool".to_string(),
                                        |approval| approval.tool_name.clone(),
                                    );
                                    let outcome_label = match decision {
                                        ApprovalDecision::Allow => "allowed",
                                        ApprovalDecision::AllowAlways => {
                                            "allowed — won't ask again"
                                        }
                                        ApprovalDecision::Deny => "denied",
                                    };
                                    app.messages.push(ChatMessage {
                                        label: "Approval".to_string(),
                                        text: format!("{tool_name} · {outcome_label}"),
                                        role: MessageRole::System,
                                    });
                                    app.approval = None;
                                    app.status = if decision == ApprovalDecision::Deny {
                                        format!("{tool_name} denied")
                                    } else {
                                        format!("running {tool_name}")
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
        } else if !app.suggestions.is_empty() && key.code == KeyCode::Down {
            app.select_next_suggestion();
        } else if !app.suggestions.is_empty() && key.code == KeyCode::Up {
            app.select_previous_suggestion();
        } else if !app.suggestions.is_empty() && key.code == KeyCode::Tab {
            app.accept_suggestion();
        } else if !app.suggestions.is_empty() && key.code == KeyCode::Esc {
            // Dismiss the dropdown without touching the composer text —
            // Esc here is "never mind," not "clear what I typed."
            app.suggestions.clear();
            app.suggestion_index = 0;
        } else if key.code == KeyCode::PageUp {
            app.scroll = app.scroll.saturating_sub(5);
            app.user_scrolled = true;
        } else if key.code == KeyCode::PageDown {
            app.scroll = app.scroll.saturating_add(5);
        } else {
            app.composer_focused = true;
            app.composer.input(Input::from(key));
            app.refresh_suggestions();
        }
    }
    Ok(())
}

fn read_approval_decision() -> io::Result<ApprovalDecision> {
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
            KeyCode::Char('y') | KeyCode::Char('Y') | KeyCode::Enter => {
                return Ok(ApprovalDecision::Allow)
            }
            KeyCode::Char('a') | KeyCode::Char('A') => return Ok(ApprovalDecision::AllowAlways),
            KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
                return Ok(ApprovalDecision::Deny)
            }
            _ => {}
        }
    }
}

fn draw(frame: &mut ratatui_core::terminal::Frame<'_>, app: &mut App<'_>) {
    app.spinner_phase = app.spinner_phase.wrapping_add(1);
    let areas = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(6),
            Constraint::Min(5),
            Constraint::Length(5),
            Constraint::Length(1),
        ])
        .split(frame.area());

    draw_header(frame, areas[0], app);
    let transcript = app.transcript();
    let transcript_height = transcript.height() as u16;
    let viewport_height = areas[1].height.saturating_sub(2);
    let bottom = transcript_height.saturating_sub(viewport_height);
    if app.scroll == u16::MAX {
        app.scroll = bottom;
    } else if app.user_scrolled && app.scroll >= bottom {
        // Paged all the way back down — resume auto-follow.
        app.user_scrolled = false;
        app.scroll = bottom;
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
    // #RTUI-SKILL-AUTOCOMPLETE: overlay the dropdown on the transcript's
    // bottom rows, right above the composer, rather than reserving a
    // permanent layout row for it — the vast majority of turns never show
    // it, so a dedicated always-there row would waste vertical space.
    if app.approval.is_none() && !app.suggestions.is_empty() {
        let popup_height = (app.suggestions.len() as u16 + 2).min(areas[1].height);
        let popup_area = Rect {
            x: areas[1].x + 2,
            y: areas[1].y + areas[1].height.saturating_sub(popup_height),
            width: areas[1].width.saturating_sub(4),
            height: popup_height,
        };
        let lines: Vec<Line> = app
            .suggestions
            .iter()
            .enumerate()
            .map(|(index, suggestion)| {
                let selected = index == app.suggestion_index;
                Line::from(Span::styled(
                    format!(" {} ", suggestion.label),
                    if selected {
                        Style::default().fg(Color::Black).bg(CORAL)
                    } else {
                        Style::default().fg(TEXT)
                    },
                ))
            })
            .collect();
        let popup = Paragraph::new(lines).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(CORAL))
                .title(" tab to complete · ↑↓ to choose · esc to dismiss "),
        );
        frame.render_widget(popup, popup_area);
    }
    if let Some(approval) = &app.approval {
        // #RTUI-HUMANE-APPROVAL: one plain sentence (built in main.rs from
        // the tool name + its actual input) and three keys — not a tool
        // name, a permission-mode label, a reason string, and a raw JSON
        // blob stacked on top of each other.
        let card = Paragraph::new(vec![
            Line::from(Span::styled(
                approval.detail.clone(),
                Style::default().fg(TEXT),
            )),
            Line::from(vec![
                Span::styled("y", Style::default().fg(CORAL).add_modifier(Modifier::BOLD)),
                Span::styled(" allow    ", Style::default().fg(FAINT)),
                Span::styled("n", Style::default().fg(CORAL).add_modifier(Modifier::BOLD)),
                Span::styled(" skip    ", Style::default().fg(FAINT)),
                Span::styled("a", Style::default().fg(CORAL).add_modifier(Modifier::BOLD)),
                Span::styled(" always allow this", Style::default().fg(FAINT)),
            ]),
        ])
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(CORAL))
                .title(" Approve? ")
                .padding(Padding::horizontal(1)),
        )
        .wrap(Wrap { trim: true });
        frame.render_widget(card, areas[2]);
    } else {
        let empty = app.composer.lines().iter().all(|line| line.is_empty());
        app.composer
            .set_cursor_style(if app.composer_focused && !empty {
                Style::default().fg(Color::Black).bg(CORAL)
            } else {
                // The custom prompt/caret owns the empty-field cursor.
                Style::default().fg(Color::Black).bg(Color::Black)
            });
        frame.render_widget(&app.composer, areas[2]);
        let caret = Paragraph::new(Line::from(Span::styled(
            "❯",
            Style::default().fg(CORAL).add_modifier(Modifier::BOLD),
        )));
        frame.render_widget(
            caret,
            Rect {
                x: areas[2].x + 2,
                y: areas[2].y + 1,
                width: 1,
                height: 1,
            },
        );
        if empty && app.composer_focused {
            frame.render_widget(
                Paragraph::new(Span::styled(" ", Style::default().bg(CORAL))),
                Rect {
                    x: areas[2].x + 4,
                    y: areas[2].y + 1,
                    width: 1,
                    height: 1,
                },
            );
        }
    }
    // #RTUI-THINKING-BANNER: the spinner now lives in the transcript, right
    // below the turn it's animating for (see `App::transcript`). This bar
    // names the model doing the work, not a second, disconnected busy
    // indicator.
    let footer = Line::from({
        let mut spans = vec![
            Span::styled(
                format!(" {} ", app.config.agent),
                Style::default().fg(TEXT).add_modifier(Modifier::BOLD),
            ),
            Span::styled(" · ", Style::default().fg(FAINT)),
            Span::styled(
                app.config.permission_mode.clone(),
                Style::default().fg(FAINT),
            ),
        ];
        if let Some(branch) = known_branch(app) {
            spans.push(Span::styled(" · ", Style::default().fg(FAINT)));
            spans.push(Span::styled(branch.to_string(), Style::default().fg(FAINT)));
        }
        spans.push(Span::styled(" · ", Style::default().fg(FAINT)));
        spans.push(Span::styled(app.status.clone(), Style::default().fg(CORAL)));
        spans
    });
    frame.render_widget(Paragraph::new(footer), areas[3]);
}

fn draw_header(frame: &mut ratatui_core::terminal::Frame<'_>, area: Rect, app: &App<'_>) {
    let model_state = if app.status == "ready" {
        "ready".to_string()
    } else {
        "thinking".to_string()
    };
    let directory = std::env::current_dir()
        .map(|path| path.display().to_string())
        .unwrap_or_else(|_| "unknown directory".to_string());
    let header = vec![
        Line::from(vec![
            Span::styled("● ", Style::default().fg(CORAL)),
            Span::styled(
                "rouraTUI Code",
                Style::default().fg(TEXT).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("  {}", app.config.version),
                Style::default().fg(FAINT),
            ),
            Span::styled("   local workspace agent", Style::default().fg(FAINT)),
        ]),
        Line::from(vec![
            Span::styled(
                "MODEL  ",
                Style::default().fg(FAINT).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                app.config.agent.clone(),
                Style::default().fg(TEXT).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                "   STATUS  ",
                Style::default().fg(FAINT).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                model_state,
                Style::default().fg(if app.status == "ready" { FAINT } else { CORAL }),
            ),
        ]),
        Line::from(vec![
            Span::styled(
                "WORKSPACE  ",
                Style::default().fg(FAINT).add_modifier(Modifier::BOLD),
            ),
            Span::styled(directory, Style::default().fg(TEXT)),
        ]),
        Line::from({
            let mut spans = vec![
                Span::styled(
                    "MODE  ",
                    Style::default().fg(FAINT).add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    app.config.permission_mode.clone(),
                    Style::default().fg(TEXT),
                ),
            ];
            // #RTUI-HIDE-UNKNOWN-BRANCH: outside a git repo (or when branch
            // detection just fails), showing the literal word "unknown"
            // reads like something broken or still loading. There's nothing
            // to show, so don't show the field at all.
            if let Some(branch) = known_branch(app) {
                spans.push(Span::styled(
                    "   BRANCH  ",
                    Style::default().fg(FAINT).add_modifier(Modifier::BOLD),
                ));
                spans.push(Span::styled(branch.to_string(), Style::default().fg(TEXT)));
            }
            spans.push(Span::styled(
                "   SESSION ACTIVE",
                Style::default().fg(CORAL),
            ));
            spans
        }),
        Line::from(Span::styled(
            "Enter send  ·  Shift-Enter/Ctrl-J newline  ·  PageUp/PageDown scroll  ·  Ctrl-C exit",
            Style::default().fg(FAINT),
        )),
    ];
    frame.render_widget(
        Paragraph::new(header).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(FAINT))
                .title(" Agent session ")
                .padding(Padding::horizontal(1)),
        ),
        area,
    );
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

/// Esc or Ctrl+C *during a turn* means "stop this turn," not "quit the
/// app" — `is_quit` is reused here since it's the same chord, just handled
/// by a different loop (the in-turn renderer below, rather than `run`'s
/// idle loop) with a different meaning.
fn is_interrupt(key: KeyEvent) -> bool {
    key.code == KeyCode::Esc || is_quit(key)
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
    use super::{is_quit, is_submit, App, ApprovalDecision, MessageRole, TuiConfig, TurnEvent};
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
            slash_commands: Vec::new(),
            skill_names: Vec::new(),
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
            slash_commands: Vec::new(),
            skill_names: Vec::new(),
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
            slash_commands: Vec::new(),
            skill_names: Vec::new(),
        });
        let (response, decision) = mpsc::sync_channel(1);
        let returned = app.handle_turn_event(TurnEvent::ApprovalRequested {
            tool_name: "shell".to_string(),
            detail: "Run: git push".to_string(),
            response,
        });
        returned
            .expect("approval response")
            .send(ApprovalDecision::Deny)
            .unwrap();
        assert_eq!(decision.recv().unwrap(), ApprovalDecision::Deny);
        assert!(app.approval.is_some());
        assert_eq!(app.status, "approval required · shell");
    }

    #[test]
    fn transcript_shows_a_spinner_for_the_pending_agent_turn() {
        let mut app = App::new(TuiConfig {
            version: "test".to_string(),
            agent: "RIO Agent".to_string(),
            permission_mode: "workspace-write".to_string(),
            branch: "dev".to_string(),
            slash_commands: Vec::new(),
            skill_names: Vec::new(),
        });
        app.composer.insert_str("hello");
        assert!(app.submit().is_some());

        // Right after submit, the agent placeholder is empty and waiting —
        // the transcript should show a live spinner there, not a blank gap.
        let waiting = app.transcript();
        let waiting_text: String = waiting.lines.iter().map(ToString::to_string).collect();
        assert!(waiting_text.contains("thinking…"));

        // Once real content streams in, the spinner line is gone.
        app.append_stream_delta("Hello there");
        let streaming = app.transcript();
        let streaming_text: String = streaming.lines.iter().map(ToString::to_string).collect();
        assert!(!streaming_text.contains("thinking…"));
        assert!(streaming_text.contains("Hello there"));
    }

    fn autocomplete_test_app() -> App<'static> {
        App::new(TuiConfig {
            version: "test".to_string(),
            agent: "RIO Agent".to_string(),
            permission_mode: "workspace-write".to_string(),
            branch: "dev".to_string(),
            slash_commands: vec![
                ("/skills".to_string(), "List or manage skills".to_string()),
                ("/session".to_string(), "Manage sessions".to_string()),
                ("/status".to_string(), "Show status".to_string()),
            ],
            skill_names: vec!["plan".to_string(), "playbook".to_string()],
        })
    }

    #[test]
    fn slash_prefix_filters_and_accept_replaces_composer() {
        let mut app = autocomplete_test_app();
        app.composer.insert_str("/s");
        app.refresh_suggestions();
        let labels: Vec<&str> = app
            .suggestions
            .iter()
            .map(|suggestion| suggestion.label.as_str())
            .collect();
        assert_eq!(
            labels,
            vec![
                "/skills — List or manage skills",
                "/session — Manage sessions",
                "/status — Show status",
            ]
        );

        app.accept_suggestion();
        assert_eq!(app.composer.lines().join("\n"), "/skills ");
        assert!(app.suggestions.is_empty());
    }

    #[test]
    fn bare_word_completes_skill_names_not_slash_commands() {
        let mut app = autocomplete_test_app();
        app.composer.insert_str("pla");
        app.refresh_suggestions();
        let labels: Vec<&str> = app
            .suggestions
            .iter()
            .map(|suggestion| suggestion.label.as_str())
            .collect();
        assert_eq!(labels, vec!["plan", "playbook"]);
    }

    #[test]
    fn suggestions_clear_once_a_space_is_typed() {
        let mut app = autocomplete_test_app();
        app.composer.insert_str("/skills ");
        app.refresh_suggestions();
        assert!(
            app.suggestions.is_empty(),
            "dropdown should get out of the way once the user is past the leading word"
        );
    }

    #[test]
    fn suggestion_navigation_wraps_in_both_directions() {
        let mut app = autocomplete_test_app();
        app.composer.insert_str("/s");
        app.refresh_suggestions();
        assert_eq!(app.suggestion_index, 0);

        app.select_previous_suggestion();
        assert_eq!(
            app.suggestion_index,
            app.suggestions.len() - 1,
            "moving up from the first entry should wrap to the last"
        );

        app.select_next_suggestion();
        assert_eq!(
            app.suggestion_index, 0,
            "moving down should wrap back to the first"
        );
    }
}
