//! Chat-based, asynchronous replacement for the CLI's synchronous terminal
//! permission prompt — the Slack analogue of `chat-server`'s
//! `approval.rs`. A gated tool call pauses (denying it back into the
//! conversation with an explanation posted to the thread), and the family
//! member's *next* message in that same thread becomes the approval or
//! denial that lets the same action actually run.
//!
//! Unlike the Open WebUI bridge, Slack threads already carry a stable
//! identity (`channel:thread_ts`), so the conversation key here is that
//! string directly rather than a hash of the first message.

use std::collections::HashMap;
use std::sync::Mutex;

use runtime::{PermissionPromptDecision, PermissionPrompter, PermissionRequest};

pub type ConversationKey = String;

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

    fn get(&self, key: &ConversationKey) -> Option<PendingApproval> {
        self.inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(key)
            .cloned()
    }

    fn set(&self, key: ConversationKey, approval: PendingApproval) {
        self.inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(key, approval);
    }

    fn clear(&self, key: &ConversationKey) {
        self.inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(key);
    }
}

/// Classification of the newest message in a thread, used to decide
/// whether it's resolving a pending approval rather than a fresh request.
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

/// `PermissionPrompter` impl that turns a gated escalation request into a
/// paused, thread-relayed approval instead of a blocking terminal read.
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
        if let Some(pending) = self.store.get(&self.key) {
            if pending.tool_name == request.tool_name {
                match self.reply_intent {
                    ReplyIntent::Approve => {
                        self.store.clear(&self.key);
                        return PermissionPromptDecision::Allow;
                    }
                    ReplyIntent::Deny => {
                        self.store.clear(&self.key);
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
            self.key.clone(),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_common_approval_replies() {
        assert_eq!(classify_reply("approve"), ReplyIntent::Approve);
        assert_eq!(classify_reply("Yes"), ReplyIntent::Approve);
        assert_eq!(classify_reply("deny"), ReplyIntent::Deny);
        assert_eq!(classify_reply("no"), ReplyIntent::Deny);
        assert_eq!(classify_reply("what does this do?"), ReplyIntent::Other);
    }

    #[test]
    fn first_ask_denies_and_records_pending_approval() {
        let store = PendingApprovals::new();
        let mut prompter =
            ChatApprovalPrompter::new("ch:C1:1".to_string(), &store, ReplyIntent::Other);
        let request = PermissionRequest {
            tool_name: "bash".to_string(),
            input: "rm -rf /tmp/x".to_string(),
            current_mode: runtime::PermissionMode::WorkspaceWrite,
            required_mode: runtime::PermissionMode::DangerFullAccess,
            reason: None,
        };
        let decision = prompter.decide(&request);
        assert!(matches!(decision, PermissionPromptDecision::Deny { .. }));
        assert!(store.get(&"ch:C1:1".to_string()).is_some());
    }

    #[test]
    fn approving_reply_allows_and_clears_pending_state() {
        let store = PendingApprovals::new();
        let request = PermissionRequest {
            tool_name: "bash".to_string(),
            input: "rm -rf /tmp/x".to_string(),
            current_mode: runtime::PermissionMode::WorkspaceWrite,
            required_mode: runtime::PermissionMode::DangerFullAccess,
            reason: None,
        };
        ChatApprovalPrompter::new("ch:C1:1".to_string(), &store, ReplyIntent::Other)
            .decide(&request);

        let decision =
            ChatApprovalPrompter::new("ch:C1:1".to_string(), &store, ReplyIntent::Approve)
                .decide(&request);
        assert_eq!(decision, PermissionPromptDecision::Allow);
        assert!(store.get(&"ch:C1:1".to_string()).is_none());
    }
}
