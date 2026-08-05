//! Chat-based, asynchronous replacement for the CLI's synchronous terminal
//! permission prompt.
//!
//! A terminal session has a human physically present to approve or deny a
//! tool call inline. A chat message hitting a public hostname does not —
//! there is no live checkpoint. Instead, a gated tool call pauses (by
//! denying it back into the conversation with an explanation), and the
//! user's *next* chat message becomes the approval/denial that lets the
//! same action actually run.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::Mutex;

use runtime::{PermissionPromptDecision, PermissionPrompter, PermissionRequest};

pub type ConversationKey = u64;

/// Stable identity for "the same Open WebUI chat thread" derived from its
/// first user message. OpenAI-style chat completions are stateless (the
/// full history is resent every call), so this is the cheapest way to
/// recognize "same conversation, one message later" without needing an
/// Open WebUI-specific session id.
///
/// Known v1 limitation: editing/regenerating the first message, or two
/// distinct conversations that happen to open with identical text, would
/// collide. Acceptable for a first version.
pub fn conversation_key_from_first_message(first_user_message: &str) -> ConversationKey {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    first_user_message.hash(&mut hasher);
    hasher.finish()
}

#[derive(Debug, Clone)]
pub struct PendingApproval {
    pub tool_name: String,
}

#[derive(Default)]
pub struct PendingApprovals {
    inner: Mutex<HashMap<ConversationKey, PendingApproval>>,
}

impl PendingApprovals {
    pub fn new() -> Self {
        Self::default()
    }

    fn get(&self, key: ConversationKey) -> Option<PendingApproval> {
        self.inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(&key)
            .cloned()
    }

    fn set(&self, key: ConversationKey, approval: PendingApproval) {
        self.inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(key, approval);
    }

    fn clear(&self, key: ConversationKey) {
        self.inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(&key);
    }
}

/// Classification of the newest user message in a turn, used to decide
/// whether it is resolving a pending approval rather than a fresh request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReplyIntent {
    Approve,
    Deny,
    Other,
}

pub fn classify_reply(user_input: &str) -> ReplyIntent {
    match user_input.trim().to_lowercase().as_str() {
        "approve" | "yes" | "y" | "approved" => ReplyIntent::Approve,
        "deny" | "no" | "n" | "denied" | "cancel" => ReplyIntent::Deny,
        _ => ReplyIntent::Other,
    }
}

/// `PermissionPrompter` impl that turns a `DangerFullAccess` escalation
/// request into a paused, chat-relayed approval instead of a blocking
/// terminal read.
pub struct ChatApprovalPrompter<'a> {
    key: ConversationKey,
    store: &'a PendingApprovals,
    reply_intent: ReplyIntent,
}

impl<'a> ChatApprovalPrompter<'a> {
    pub fn new(
        key: ConversationKey,
        store: &'a PendingApprovals,
        reply_intent: ReplyIntent,
    ) -> Self {
        Self {
            key,
            store,
            reply_intent,
        }
    }
}

impl PermissionPrompter for ChatApprovalPrompter<'_> {
    fn decide(&mut self, request: &PermissionRequest) -> PermissionPromptDecision {
        if let Some(pending) = self.store.get(self.key) {
            if pending.tool_name == request.tool_name {
                match self.reply_intent {
                    ReplyIntent::Approve => {
                        self.store.clear(self.key);
                        return PermissionPromptDecision::Allow;
                    }
                    ReplyIntent::Deny => {
                        self.store.clear(self.key);
                        return PermissionPromptDecision::Deny {
                            reason: "The user denied this action.".to_string(),
                        };
                    }
                    ReplyIntent::Other => {
                        // A pending action exists but the newest message
                        // didn't resolve it; fall through and re-ask.
                    }
                }
            }
        }

        self.store.set(
            self.key,
            PendingApproval {
                tool_name: request.tool_name.clone(),
            },
        );
        PermissionPromptDecision::Deny {
            reason: format!(
                "This action requires your approval before it can run.\n\nTool: {}\nInput: {}\n\nReply \"approve\" to proceed or \"deny\" to cancel.",
                request.tool_name, request.input
            ),
        }
    }
}
