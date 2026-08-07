//! Slack Socket Mode bridge for Roura-tui.
//!
//! One orchestrating bot, reachable from any channel it's invited to (reply
//! in-thread after an @-mention) or by DM, restricted to the family
//! allowlist. Built on the same `BuiltRuntime` the interactive CLI and
//! `chat-server` use — see `chat-server`'s module docs for why a
//! non-`Send` runtime lives on its own dedicated worker thread per
//! conversation, and why gated tool calls pause for an explicit,
//! chat-relayed approval rather than running unattended.
//!
//! Socket Mode (rather than an inbound HTTPS Events API endpoint) means no
//! public hostname or firewall hole is needed: this process opens an
//! outbound WebSocket to Slack and receives events over it. On any
//! connection error it reconnects with backoff and never exits — a
//! long-running family bot dying on a blip is worse than a few seconds of
//! missed messages.

mod approval;
mod events;
mod slack_api;

use std::collections::HashMap;
use std::sync::mpsc as std_mpsc;
use std::time::Duration;

use clap::Parser;
use futures_util::{SinkExt, StreamExt};
use runtime::{PermissionMode, Session, TurnSummary};
use tokio_tungstenite::tungstenite::Message as WsMessage;

use approval::{ChatApprovalPrompter, ConversationKey, PendingApprovals};
use events::{Route, SocketFrame};
use slack_api::SlackClient;

type WsSink = futures_util::stream::SplitSink<
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>,
    WsMessage,
>;

#[derive(Parser)]
#[command(
    name = "rouratui-slack-bridge",
    about = "Slack Socket Mode bridge for Roura-tui"
)]
struct Cli {
    /// Bot token (`xoxb-...`), scopes: chat:write, users:read.email,
    /// im:history, channels:history, groups:history, mpim:history.
    #[arg(long, env = "SLACK_BOT_TOKEN")]
    bot_token: String,
    /// App-level token (`xapp-...`) with the `connections:write` scope.
    #[arg(long, env = "SLACK_APP_TOKEN")]
    app_token: String,
    /// Comma-separated emails allowed to use the bot.
    #[arg(
        long,
        env = "SLACK_ALLOWED_EMAILS",
        value_delimiter = ',',
        default_value = "hqqncggw4t@privaterelay.appleid.com,wroura@gmail.com,carito.rodas92@gmail.com,susanroura@gmail.com"
    )]
    allowed_emails: Vec<String>,
    /// Ollama model tag to drive the orchestrator with.
    #[arg(
        long,
        env = "ROURATUI_SLACK_MODEL",
        default_value = "qwen3.6:27b-coding-bf16"
    )]
    model: String,
}

struct TurnRequest {
    key: ConversationKey,
    reply_intent: approval::ReplyIntent,
    user_input: String,
    respond_to: tokio::sync::oneshot::Sender<Result<String, String>>,
}

#[derive(Clone)]
struct KnownThreads {
    inner: std::sync::Arc<std::sync::Mutex<std::collections::HashSet<ConversationKey>>>,
}

impl KnownThreads {
    fn new() -> Self {
        Self {
            inner: std::sync::Arc::new(std::sync::Mutex::new(std::collections::HashSet::new())),
        }
    }

    fn contains(&self, channel: &str, thread_root: &str) -> bool {
        let key = format!("ch:{channel}:{thread_root}");
        self.inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .contains(&key)
    }

    fn record(&self, key: &ConversationKey) {
        self.inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(key.clone());
    }
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    if std::env::var_os("OLLAMA_HOST").is_none() {
        eprintln!(
            "warning: OLLAMA_HOST is not set; the api crate will fall back to its own default \
             (http://127.0.0.1:11434). Set it explicitly to be certain which Ollama instance is used."
        );
    }

    let slack = SlackClient::new(cli.bot_token.clone(), cli.app_token.clone());
    let bot_user_id = slack
        .bot_user_id()
        .await
        .unwrap_or_else(|error| panic!("auth.test failed — check SLACK_BOT_TOKEN: {error}"));

    let mut allowed_user_ids = Vec::new();
    for email in &cli.allowed_emails {
        match slack.lookup_user_id_by_email(email).await {
            Ok(Some(user_id)) => {
                println!("resolved {email} -> {user_id}");
                allowed_user_ids.push(user_id);
            }
            Ok(None) => {
                eprintln!("warning: {email} has no Slack account in this workspace; skipping")
            }
            Err(error) => eprintln!("warning: failed to resolve {email}: {error}"),
        }
    }
    if allowed_user_ids.is_empty() {
        panic!("no allowed emails resolved to a Slack user — refusing to start with an open bot");
    }

    let (tx, rx) = std_mpsc::channel::<TurnRequest>();
    let model = cli.model.clone();
    std::thread::spawn(move || worker_loop(rx, model));

    let known_threads = KnownThreads::new();

    let mut backoff = Duration::from_secs(2);
    loop {
        match run_connection(&slack, &bot_user_id, &allowed_user_ids, &tx, &known_threads).await {
            Ok(()) => backoff = Duration::from_secs(2),
            Err(error) => eprintln!("socket mode connection lost: {error}"),
        }
        eprintln!("reconnecting in {}s", backoff.as_secs());
        tokio::time::sleep(backoff).await;
        backoff = std::cmp::min(backoff * 2, Duration::from_secs(60));
    }
}

async fn run_connection(
    slack: &SlackClient,
    bot_user_id: &str,
    allowed_user_ids: &[String],
    tx: &std_mpsc::Sender<TurnRequest>,
    known_threads: &KnownThreads,
) -> Result<(), String> {
    let url = slack.open_socket_url().await?;
    let (ws_stream, _) = tokio_tungstenite::connect_async(url)
        .await
        .map_err(|error| format!("websocket connect failed: {error}"))?;
    println!("connected to Slack Socket Mode");
    let (mut write, mut read) = ws_stream.split();

    while let Some(frame) = read.next().await {
        let frame = frame.map_err(|error| format!("websocket read error: {error}"))?;
        let WsMessage::Text(text) = frame else {
            continue;
        };
        let Some(parsed) = events::parse_frame(&text) else {
            continue;
        };
        match parsed {
            SocketFrame::Hello => {}
            SocketFrame::Disconnect => {
                return Err("Slack requested a reconnect".to_string());
            }
            SocketFrame::Unhandled { envelope_id } => {
                if let Some(envelope_id) = envelope_id {
                    ack(&mut write, &envelope_id).await?;
                }
            }
            SocketFrame::Message { envelope_id, event } => {
                ack(&mut write, &envelope_id).await?;
                let route = events::route(
                    &event,
                    bot_user_id,
                    allowed_user_ids,
                    |channel, thread_root| known_threads.contains(channel, thread_root),
                );
                if let Route::Handle {
                    key,
                    channel,
                    thread_ts,
                    text,
                } = route
                {
                    known_threads.record(&key);
                    spawn_turn(slack.clone(), tx.clone(), key, channel, thread_ts, text);
                }
            }
        }
    }

    Err("websocket stream ended".to_string())
}

async fn ack(write: &mut WsSink, envelope_id: &str) -> Result<(), String> {
    let ack = serde_json::json!({ "envelope_id": envelope_id }).to_string();
    write
        .send(WsMessage::Text(ack))
        .await
        .map_err(|error| format!("failed to ack envelope: {error}"))
}

/// Runs one turn on the worker thread and posts the reply back to Slack.
/// Spawned so the socket read loop keeps acking new events (within Slack's
/// 3-second budget) instead of blocking on however long the agent takes.
fn spawn_turn(
    slack: SlackClient,
    tx: std_mpsc::Sender<TurnRequest>,
    key: ConversationKey,
    channel: String,
    thread_ts: Option<String>,
    text: String,
) {
    tokio::spawn(async move {
        let loading_ts = match slack
            .post_message(&channel, "⏳ Thinking…", thread_ts.as_deref())
            .await
        {
            Ok(ts) => Some(ts),
            Err(error) => {
                eprintln!("failed to post loading message to Slack: {error}");
                None
            }
        };

        let reply_intent = approval::classify_reply(&text);
        let (respond_to, receiver) = tokio::sync::oneshot::channel();
        if tx
            .send(TurnRequest {
                key,
                reply_intent,
                user_input: text,
                respond_to,
            })
            .is_err()
        {
            eprintln!("worker thread is not running; dropping turn");
            return;
        }
        let reply = match receiver.await {
            Ok(Ok(text)) => text,
            Ok(Err(error)) => format!("Internal error running this turn: {error}"),
            Err(_) => "Internal error: worker thread dropped the response channel".to_string(),
        };
        let post_result = match &loading_ts {
            Some(ts) => slack.update_message(&channel, ts, &reply).await,
            None => slack
                .post_message(&channel, &reply, thread_ts.as_deref())
                .await
                .map(|_| ()),
        };
        if let Err(error) = post_result {
            eprintln!("failed to post reply to Slack: {error}");
        }
    });
}

/// Owns every `BuiltRuntime` for the life of the process. Runs on a plain
/// OS thread (not part of the Tokio runtime) so the non-`Send` runtime
/// never needs to cross an async task boundary. One turn at a time by
/// design — the "single orchestrating bot" — a fine trade-off for a
/// personal/family-use bridge.
fn worker_loop(rx: std_mpsc::Receiver<TurnRequest>, model: String) {
    let approvals = PendingApprovals::new();
    let mut runtimes: HashMap<ConversationKey, rouratui_cli::BuiltRuntime> = HashMap::new();

    while let Ok(request) = rx.recv() {
        let result = run_one_turn(
            &mut runtimes,
            &approvals,
            &model,
            request.key,
            request.reply_intent,
            request.user_input,
        );
        let _ = request.respond_to.send(result);
    }
}

fn run_one_turn(
    runtimes: &mut HashMap<ConversationKey, rouratui_cli::BuiltRuntime>,
    approvals: &PendingApprovals,
    model: &str,
    key: ConversationKey,
    reply_intent: approval::ReplyIntent,
    user_input: String,
) -> Result<String, String> {
    let mut built = match runtimes.remove(&key) {
        Some(existing) => existing,
        None => new_runtime(model)?,
    };

    let mut prompter = ChatApprovalPrompter::new(key.clone(), approvals, reply_intent);
    let outcome = built.run_turn(user_input, Some(&mut prompter));

    let text = match &outcome {
        Ok(summary) => extract_assistant_text(summary),
        Err(error) => format!("Internal error running this turn: {error}"),
    };

    runtimes.insert(key, built);
    Ok(text)
}

/// Read-only information lookups don't need the chat-relayed approval
/// round trip family members would otherwise hit for every factual
/// question — only tools that change state or spend money stay gated at
/// `WorkspaceWrite` (bash, file writes, git, etc.).
const UNGATED_READONLY_TOOLS: &[(&str, PermissionMode)] = &[
    ("WebSearch", PermissionMode::WorkspaceWrite),
    ("WebFetch", PermissionMode::WorkspaceWrite),
];

/// `build_runtime_with_tool_overrides` sends this straight through as the
/// model's system prompt (unlike the interactive CLI and `chat-server`, this
/// bridge passes a non-empty one) — it exists to correct two things a bare
/// LLM otherwise gets wrong in this context: it defaults to CommonMark, which
/// Slack does not render, and it has no way to know which of several
/// possible streaming apps this specific family actually has installed.
const SLACK_ASSISTANT_CONTEXT: &str = "\
You are replying directly inside Slack messages, which render Slack's \"mrkdwn\" \
syntax, not standard Markdown. Follow these formatting rules for every reply:
- Bold: *text* (single asterisks), never **text**
- Italic: _text_
- Links: <https://example.com|link text>, never [text](url)
- Never use Markdown tables (Slack does not render them) — use a bulleted list instead
- Never use Markdown headers (#, ##, ###) — use a *bold* line instead
- Bulleted lists: start each line with \"- \" or \"\u{2022} \"

Family context for Yankees / sports questions: the household only has these apps \
for watching Yankees games — Netflix (rare, only for the occasional national \
broadcast), Peacock, Prime Video, DAZN (the exclusive streaming home for YES \
Network and MSG Networks' regional Yankees broadcasts), and YouTube TV (for \
national broadcasts on channels like ESPN, NBC, TNT, and Fox). When identifying \
where to watch a game, name the exact app from this list rather than the \
underlying network name (e.g. say \"DAZN\", not \"YES Network\"). When you can \
find a direct link into that app or its live/team page via WebSearch, include \
it as a Slack link; do not fabricate a URL you have not verified.

For any question about a live or upcoming event (a game today, this week's \
schedule, current scores, etc.), never answer from memory — your training data \
has no reliable sense of \"today\" and can surface an unrelated game from a \
different date. Always run a fresh WebSearch for the specific date in question, \
and check that the date on each search result actually matches before treating \
it as today's answer; if results are ambiguous or don't clearly state a date, \
say so instead of guessing.\
";

/// Runs `date` to ground the model in the actual wall-clock date at the
/// moment each conversation starts (this process has no other source of
/// truth for \"today\" — `DEFAULT_DATE` in the interactive CLI is a
/// build-time constant, useless for a long-running daemon). Best-effort:
/// if the shell-out fails for any reason, the bridge still runs, just
/// without this grounding.
fn today_date_context() -> Option<String> {
    let output = std::process::Command::new("date")
        .arg("+%A, %B %-d, %Y")
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let today = String::from_utf8(output.stdout).ok()?;
    let today = today.trim();
    (!today.is_empty()).then(|| format!("Today's date is {today}."))
}

fn new_runtime(model: &str) -> Result<rouratui_cli::BuiltRuntime, String> {
    let mut system_prompt = vec![SLACK_ASSISTANT_CONTEXT.to_string()];
    if let Some(today) = today_date_context() {
        system_prompt.push(today);
    }
    rouratui_cli::build_runtime_with_tool_overrides(
        Session::new(),
        "rouratui-slack-bridge",
        model.to_string(),
        system_prompt,
        true,
        false,
        None,
        PermissionMode::WorkspaceWrite,
        None,
        UNGATED_READONLY_TOOLS,
    )
    .map_err(|error| format!("failed to build runtime: {error}"))
}

fn extract_assistant_text(summary: &TurnSummary) -> String {
    let mut text = String::new();
    for message in &summary.assistant_messages {
        for block in &message.blocks {
            if let runtime::ContentBlock::Text { text: block_text } = block {
                if !text.is_empty() {
                    text.push('\n');
                }
                text.push_str(block_text);
            }
        }
    }
    if text.is_empty() {
        text.push_str("(no response text)");
    }
    text
}
