//! Exposes Roura-tui's own orchestrator (the same one the interactive CLI
//! drives) as an OpenAI-compatible HTTP API, so it can be registered as a
//! Connection inside Open WebUI. See the repo plan for the full design
//! rationale — most notably why gated tool calls pause for an explicit,
//! chat-relayed human approval rather than running unattended.
//!
//! `BuiltRuntime` (the CLI's own runtime wrapper) is not `Send` — it holds
//! a `Box<dyn HookProgressReporter>`, and that trait has no `Send` bound.
//! Rather than widen that trait's contract, every runtime instance lives on
//! one dedicated worker thread for the life of the process and is never
//! moved elsewhere; the async Axum handlers talk to that thread over plain
//! channels. This also means turns are processed one at a time — a fine
//! trade-off for a personal-use bridge, not a high-throughput service.

mod approval;
mod types;

use std::collections::HashMap;
use std::convert::Infallible;
use std::sync::mpsc as std_mpsc;

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::sse::{Event, Sse};
use axum::response::{IntoResponse, Json, Response};
use axum::routing::{get, post};
use axum::Router;
use clap::Parser;
use rouratui_router::{select, RouteDecision, RouteKind};
use runtime::{PermissionMode, Session, TurnSummary};
use tokio::sync::oneshot;

use approval::{ChatApprovalPrompter, ConversationKey, PendingApprovals};
use types::{
    ChatCompletionChunk, ChatCompletionRequest, ChatCompletionResponse, Choice, ChunkChoice, Delta,
    ModelEntry, ModelsResponse, OutgoingMessage,
};

#[derive(Parser)]
#[command(
    name = "rouratui-chat-server",
    about = "OpenAI-compatible bridge for Roura-tui"
)]
struct Cli {
    /// Port to listen on.
    #[arg(long, env = "ROURATUI_CHAT_PORT", default_value_t = 18080)]
    port: u16,
    /// Ollama model tag to drive the orchestrator with, or `auto` to route
    /// each new conversation through the shared RouraTUI policy.
    #[arg(long, env = "ROURATUI_CHAT_MODEL", default_value = "auto")]
    model: String,
    /// Reasoning effort forwarded to the model. `none` disables thinking
    /// tokens, which dominates latency for chat traffic on thinking-capable
    /// models. Use `low`/`medium`/`high` to re-enable reasoning, or `default`
    /// to leave the provider's own behaviour untouched.
    #[arg(long, env = "ROURATUI_CHAT_REASONING_EFFORT", default_value = "none")]
    reasoning_effort: String,
}

struct TurnRequest {
    key: ConversationKey,
    reply_intent: approval::ReplyIntent,
    user_input: String,
    respond_to: oneshot::Sender<Result<String, String>>,
}

#[derive(Clone)]
struct AppState {
    worker: std_mpsc::Sender<TurnRequest>,
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    if std::env::var_os("OLLAMA_HOST").is_none() {
        eprintln!(
            "warning: OLLAMA_HOST is not set in this process's environment; the api crate will \
             fall back to its own default (http://127.0.0.1:11434). Set OLLAMA_HOST explicitly \
             (e.g. in the launchd plist) to be certain which Ollama instance is used."
        );
    }

    let (tx, rx) = std_mpsc::channel::<TurnRequest>();
    let model = cli.model.clone();
    let reasoning_effort = normalize_reasoning_effort(&cli.reasoning_effort);
    std::thread::spawn(move || worker_loop(rx, model, reasoning_effort));

    let state = AppState { worker: tx };

    let app = Router::new()
        .route("/health", get(|| async { "ok" }))
        .route("/v1/models", get(list_models))
        .route("/v1/chat/completions", post(chat_completions))
        .with_state(state);

    let addr = format!("0.0.0.0:{}", cli.port);
    println!(
        "rouratui-chat-server listening on {addr}, model={}, reasoning_effort={}",
        cli.model, cli.reasoning_effort
    );
    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .unwrap_or_else(|error| panic!("failed to bind {addr}: {error}"));
    axum::serve(listener, app)
        .await
        .unwrap_or_else(|error| panic!("server error: {error}"));
}

/// Owns every `BuiltRuntime` for the life of the process. Runs on a plain
/// OS thread (not part of the Tokio runtime) so the non-`Send` runtime
/// never needs to cross an async task boundary.
fn worker_loop(
    rx: std_mpsc::Receiver<TurnRequest>,
    model: String,
    reasoning_effort: Option<String>,
) {
    let approvals = PendingApprovals::new();
    let mut runtimes: HashMap<ConversationKey, (String, rouratui_cli::BuiltRuntime)> =
        HashMap::new();

    while let Ok(request) = rx.recv() {
        let result = run_one_turn(
            &mut runtimes,
            &approvals,
            &model,
            reasoning_effort.as_deref(),
            request.key,
            request.reply_intent,
            request.user_input,
        );
        // Ignore send errors: the HTTP request that wanted this result may
        // have already timed out or been dropped by the client.
        let _ = request.respond_to.send(result);
    }
}

fn run_one_turn(
    runtimes: &mut HashMap<ConversationKey, (String, rouratui_cli::BuiltRuntime)>,
    approvals: &PendingApprovals,
    model: &str,
    reasoning_effort: Option<&str>,
    key: ConversationKey,
    reply_intent: approval::ReplyIntent,
    user_input: String,
) -> Result<String, String> {
    let (selected_model, mut built) = match runtimes.remove(&key) {
        Some(existing) => existing,
        None => {
            let decision = if model == "auto" {
                select(&user_input)
            } else {
                RouteDecision {
                    model: model.to_string(),
                    kind: RouteKind::Fallback,
                    reason: "explicit model override",
                }
            };
            eprintln!(
                "rouratui route: model={} kind={:?} reason={}",
                decision.model, decision.kind, decision.reason
            );
            let selected_model = decision.model.clone();
            (
                selected_model,
                new_runtime(&decision.model, reasoning_effort)?,
            )
        }
    };

    let mut prompter = ChatApprovalPrompter::new(key, approvals, reply_intent);
    let outcome = built.run_turn(user_input, Some(&mut prompter));

    let text = match &outcome {
        Ok(summary) => extract_assistant_text(summary),
        Err(error) => format!("Internal error running this turn: {error}"),
    };

    runtimes.insert(key, (selected_model, built));
    Ok(text)
}

fn new_runtime(
    model: &str,
    reasoning_effort: Option<&str>,
) -> Result<rouratui_cli::BuiltRuntime, String> {
    let mut built = rouratui_cli::build_runtime(
        Session::new(),
        "rouratui-chat-server",
        model.to_string(),
        Vec::new(),
        true,
        false,
        None,
        PermissionMode::WorkspaceWrite,
        None,
    )
    .map_err(|error| format!("failed to build runtime: {error}"))?;
    built.set_reasoning_effort(reasoning_effort.map(str::to_string));
    Ok(built)
}

/// `default` (or empty) leaves the provider's own reasoning behaviour alone.
/// Anything else is forwarded verbatim, including `none` to disable thinking.
fn normalize_reasoning_effort(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("default") {
        None
    } else {
        Some(trimmed.to_string())
    }
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

async fn list_models() -> Json<ModelsResponse> {
    Json(ModelsResponse {
        object: "list",
        data: vec![ModelEntry {
            id: "rouratui".to_string(),
            object: "model",
            created: types::unix_now(),
            owned_by: "roura-io",
        }],
    })
}

async fn chat_completions(
    State(state): State<AppState>,
    Json(request): Json<ChatCompletionRequest>,
) -> Response {
    let Some(last) = request.messages.last() else {
        return (StatusCode::BAD_REQUEST, "messages must not be empty").into_response();
    };
    let user_input = last.content.as_text();

    let first_user_message = request
        .messages
        .iter()
        .find(|message| message.role == "user")
        .map(|message| message.content.as_text())
        .unwrap_or_else(|| user_input.clone());
    let key = approval::conversation_key_from_first_message(&first_user_message);
    let reply_intent = approval::classify_reply(&user_input);
    let stream = request.stream;

    let (respond_to, receiver) = oneshot::channel();
    if state
        .worker
        .send(TurnRequest {
            key,
            reply_intent,
            user_input,
            respond_to,
        })
        .is_err()
    {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            "worker thread is not running",
        )
            .into_response();
    }

    let assistant_text = match receiver.await {
        Ok(Ok(text)) => text,
        Ok(Err(error)) => return (StatusCode::INTERNAL_SERVER_ERROR, error).into_response(),
        Err(_) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                "worker thread dropped the response channel",
            )
                .into_response()
        }
    };

    if stream {
        stream_response("rouratui".to_string(), assistant_text).into_response()
    } else {
        Json(build_completion_response(
            "rouratui".to_string(),
            assistant_text,
        ))
        .into_response()
    }
}

fn build_completion_response(model: String, assistant_text: String) -> ChatCompletionResponse {
    ChatCompletionResponse {
        id: format!("chatcmpl-{}", types::unix_now()),
        object: "chat.completion",
        created: types::unix_now(),
        model,
        choices: vec![Choice {
            index: 0,
            message: OutgoingMessage {
                role: "assistant",
                content: assistant_text,
            },
            finish_reason: "stop",
        }],
    }
}

/// v1 streaming is not token-incremental: it buffers the full turn, then
/// emits it as a single content delta followed by a terminal chunk and
/// `[DONE]`. This is still a valid OpenAI-compatible SSE stream and is
/// enough for Open WebUI to render correctly; true incremental streaming
/// would require threading a sink through `run_turn` itself.
fn stream_response(
    model: String,
    assistant_text: String,
) -> Sse<impl futures_core::Stream<Item = Result<Event, Infallible>>> {
    let id = format!("chatcmpl-{}", types::unix_now());
    let created = types::unix_now();

    let content_chunk = ChatCompletionChunk {
        id: id.clone(),
        object: "chat.completion.chunk",
        created,
        model: model.clone(),
        choices: vec![ChunkChoice {
            index: 0,
            delta: Delta {
                role: Some("assistant"),
                content: Some(assistant_text),
            },
            finish_reason: None,
        }],
    };
    let final_chunk = ChatCompletionChunk {
        id,
        object: "chat.completion.chunk",
        created,
        model,
        choices: vec![ChunkChoice {
            index: 0,
            delta: Delta::default(),
            finish_reason: Some("stop"),
        }],
    };

    let events = vec![
        Ok(Event::default().data(serde_json::to_string(&content_chunk).unwrap_or_default())),
        Ok(Event::default().data(serde_json::to_string(&final_chunk).unwrap_or_default())),
        Ok(Event::default().data("[DONE]")),
    ];

    Sse::new(tokio_stream::iter(events))
}
