//! Socket Mode envelope parsing and message routing.
//!
//! Kept free of I/O so the routing rules (DM vs. channel, mention vs. reply)
//! are unit-testable without a live Slack connection.

use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct RawEnvelope {
    #[serde(rename = "type")]
    kind: String,
    envelope_id: Option<String>,
    payload: Option<RawPayload>,
}

#[derive(Debug, Deserialize)]
struct RawPayload {
    event: Option<MessageEvent>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct MessageEvent {
    #[serde(rename = "type")]
    pub kind: String,
    pub channel: Option<String>,
    pub user: Option<String>,
    pub text: Option<String>,
    pub ts: Option<String>,
    pub thread_ts: Option<String>,
    pub channel_type: Option<String>,
    pub bot_id: Option<String>,
    pub subtype: Option<String>,
}

/// One parsed Socket Mode frame.
#[derive(Debug)]
pub enum SocketFrame {
    Hello,
    Disconnect,
    Message {
        envelope_id: String,
        event: Box<MessageEvent>,
    },
    /// A frame this bridge doesn't act on (slash commands, interactive
    /// payloads, non-`message` events) but still must acknowledge.
    Unhandled {
        envelope_id: Option<String>,
    },
}

pub fn parse_frame(raw: &str) -> Option<SocketFrame> {
    let envelope: RawEnvelope = serde_json::from_str(raw).ok()?;
    match envelope.kind.as_str() {
        "hello" => Some(SocketFrame::Hello),
        "disconnect" => Some(SocketFrame::Disconnect),
        "events_api" => {
            let envelope_id = envelope.envelope_id?;
            match envelope.payload.and_then(|payload| payload.event) {
                Some(event) if event.kind == "message" => Some(SocketFrame::Message {
                    envelope_id,
                    event: Box::new(event),
                }),
                _ => Some(SocketFrame::Unhandled {
                    envelope_id: Some(envelope_id),
                }),
            }
        }
        _ => Some(SocketFrame::Unhandled {
            envelope_id: envelope.envelope_id,
        }),
    }
}

/// Why an incoming message was or wasn't routed to the agent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Route {
    /// Continue (or start) a conversation under this key, replying with the
    /// given `thread_ts` (Slack threads the reply under it; `None` posts a
    /// plain top-level DM reply).
    Handle {
        key: String,
        channel: String,
        thread_ts: Option<String>,
        text: String,
    },
    Ignore,
}

/// A message is only ever handled if it's a plain (non-bot, non-edited)
/// message and the sender is on the family allowlist.
fn is_actionable(event: &MessageEvent) -> bool {
    event.bot_id.is_none()
        && event.subtype.is_none()
        && event.channel.is_some()
        && event.user.is_some()
        && event.ts.is_some()
}

/// `known_thread` reports whether `(channel, thread_root)` already has an
/// active session — the worker's runtime map is the source of truth, so the
/// caller passes this in rather than routing carrying its own state.
pub fn route(
    event: &MessageEvent,
    bot_user_id: &str,
    allowed_user_ids: &[String],
    known_thread: impl FnOnce(&str, &str) -> bool,
) -> Route {
    if !is_actionable(event) {
        return Route::Ignore;
    }
    let user = event.user.as_deref().unwrap_or_default();
    if !allowed_user_ids.iter().any(|id| id == user) {
        return Route::Ignore;
    }
    let channel = event.channel.clone().unwrap_or_default();
    let ts = event.ts.clone().unwrap_or_default();
    let text = strip_mention(event.text.as_deref().unwrap_or_default(), bot_user_id);
    let is_dm = event.channel_type.as_deref() == Some("im");

    if is_dm {
        return Route::Handle {
            key: format!("dm:{channel}"),
            channel,
            thread_ts: None,
            text,
        };
    }

    let mentions_bot = event
        .text
        .as_deref()
        .unwrap_or_default()
        .contains(&format!("<@{bot_user_id}>"));

    match &event.thread_ts {
        Some(thread_root) => {
            let key = format!("ch:{channel}:{thread_root}");
            if mentions_bot || known_thread(&channel, thread_root) {
                Route::Handle {
                    key,
                    channel,
                    thread_ts: Some(thread_root.clone()),
                    text,
                }
            } else {
                Route::Ignore
            }
        }
        None if mentions_bot => Route::Handle {
            key: format!("ch:{channel}:{ts}"),
            channel,
            thread_ts: Some(ts),
            text,
        },
        None => Route::Ignore,
    }
}

fn strip_mention(text: &str, bot_user_id: &str) -> String {
    text.replace(&format!("<@{bot_user_id}>"), "")
        .trim()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    const BOT: &str = "U_BOT";
    fn allowed() -> Vec<String> {
        vec!["U_CHRIS".to_string()]
    }

    fn base_event() -> MessageEvent {
        MessageEvent {
            kind: "message".to_string(),
            channel: Some("C1".to_string()),
            user: Some("U_CHRIS".to_string()),
            text: Some("hi".to_string()),
            ts: Some("100.1".to_string()),
            thread_ts: None,
            channel_type: Some("channel".to_string()),
            bot_id: None,
            subtype: None,
        }
    }

    #[test]
    fn dm_is_always_handled() {
        let mut event = base_event();
        event.channel_type = Some("im".to_string());
        let route = route(&event, BOT, &allowed(), |_, _| false);
        assert_eq!(
            route,
            Route::Handle {
                key: "dm:C1".to_string(),
                channel: "C1".to_string(),
                thread_ts: None,
                text: "hi".to_string(),
            }
        );
    }

    #[test]
    fn channel_message_without_mention_is_ignored() {
        let event = base_event();
        assert_eq!(route(&event, BOT, &allowed(), |_, _| false), Route::Ignore);
    }

    #[test]
    fn channel_mention_starts_a_new_thread() {
        let mut event = base_event();
        event.text = Some("<@U_BOT> hello there".to_string());
        let route = route(&event, BOT, &allowed(), |_, _| false);
        assert_eq!(
            route,
            Route::Handle {
                key: "ch:C1:100.1".to_string(),
                channel: "C1".to_string(),
                thread_ts: Some("100.1".to_string()),
                text: "hello there".to_string(),
            }
        );
    }

    #[test]
    fn reply_in_known_thread_continues_without_remention() {
        let mut event = base_event();
        event.thread_ts = Some("50.0".to_string());
        let route = route(&event, BOT, &allowed(), |_, root| root == "50.0");
        assert_eq!(
            route,
            Route::Handle {
                key: "ch:C1:50.0".to_string(),
                channel: "C1".to_string(),
                thread_ts: Some("50.0".to_string()),
                text: "hi".to_string(),
            }
        );
    }

    #[test]
    fn reply_in_unknown_thread_without_mention_is_ignored() {
        let mut event = base_event();
        event.thread_ts = Some("50.0".to_string());
        assert_eq!(route(&event, BOT, &allowed(), |_, _| false), Route::Ignore);
    }

    #[test]
    fn unauthorized_sender_is_ignored_even_in_dm() {
        let mut event = base_event();
        event.channel_type = Some("im".to_string());
        event.user = Some("U_STRANGER".to_string());
        assert_eq!(route(&event, BOT, &allowed(), |_, _| false), Route::Ignore);
    }

    #[test]
    fn bot_messages_are_never_actionable() {
        let mut event = base_event();
        event.bot_id = Some("B1".to_string());
        assert_eq!(route(&event, BOT, &allowed(), |_, _| false), Route::Ignore);
    }

    #[test]
    fn edited_messages_are_ignored() {
        let mut event = base_event();
        event.subtype = Some("message_changed".to_string());
        assert_eq!(route(&event, BOT, &allowed(), |_, _| false), Route::Ignore);
    }

    #[test]
    fn parses_events_api_message_frame() {
        let raw = r#"{
            "type": "events_api",
            "envelope_id": "abc-123",
            "payload": {
                "event": {
                    "type": "message",
                    "channel": "C1",
                    "user": "U_CHRIS",
                    "text": "hi",
                    "ts": "100.1",
                    "channel_type": "channel"
                }
            }
        }"#;
        match parse_frame(raw) {
            Some(SocketFrame::Message { envelope_id, event }) => {
                assert_eq!(envelope_id, "abc-123");
                assert_eq!(event.channel.as_deref(), Some("C1"));
            }
            other => panic!("expected Message frame, got {other:?}"),
        }
    }

    #[test]
    fn parses_hello_frame() {
        let raw = r#"{"type": "hello"}"#;
        assert!(matches!(parse_frame(raw), Some(SocketFrame::Hello)));
    }

    #[test]
    fn non_message_events_api_frames_are_unhandled_but_acked() {
        let raw = r#"{
            "type": "events_api",
            "envelope_id": "abc-123",
            "payload": { "event": { "type": "reaction_added" } }
        }"#;
        match parse_frame(raw) {
            Some(SocketFrame::Unhandled { envelope_id }) => {
                assert_eq!(envelope_id.as_deref(), Some("abc-123"));
            }
            other => panic!("expected Unhandled frame, got {other:?}"),
        }
    }
}
