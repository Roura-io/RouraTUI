//! Stable model-routing policy shared by RouraTUI surfaces.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RouteKind {
    Coding,
    DeepReasoning,
    General,
    Fallback,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RouteDecision {
    pub model: String,
    pub kind: RouteKind,
    pub reason: &'static str,
}

pub const QWEN_38: &str = "qwen3.8:27b-mlx";
pub const NEMOTRON_35: &str = "nemotron-3.5-lightning:30b-mlx";
pub const GPT_OSS: &str = "gpt-oss:120b";
pub const QWEN_36: &str = "qwen3.6:27b-coding-bf16";

const CODING_TERMS: &[&str] = &[
    "code",
    "coding",
    "review",
    "pull request",
    "pr ",
    "commit",
    "branch",
    "git",
    "rust",
    "swift",
    "ios",
    "macos",
    "flutter",
    "dart",
    "bug",
    "debug",
    "test",
    "build",
    "compile",
    "refactor",
    "function",
    "class",
    "file",
    "repository",
    "repo",
    "implement",
    "patch",
    "workflow",
    // Spelled out rather than a bare "ci": `contains_any` matches raw
    // substrings, so "ci" fires on decision/specific/appreciate/precise and
    // quietly sends ordinary chat to the coding model. Plain CI phrasing is
    // still caught by "pipeline"/"workflow"/"build"/"test"/"branch".
    "ci/cd",
    "continuous integration",
    "pipeline",
];

const DEEP_TERMS: &[&str] = &[
    "architecture",
    "architect",
    "deep",
    "thorough",
    "investigate",
    "compare",
    "tradeoff",
    "trade-off",
    "design",
    "plan",
    "research",
    "analyze",
    "analyse",
    "why is",
    "root cause",
    "strategy",
    "migration",
    "evaluate",
    "reasoning",
];

/// Select a model for a new conversation. The caller should keep the result
/// sticky for follow-up turns so a tool-using task does not change models.
#[must_use]
pub fn select(prompt: &str) -> RouteDecision {
    let normalized = prompt.to_ascii_lowercase();
    if contains_any(&normalized, CODING_TERMS) {
        return decision(QWEN_38, RouteKind::Coding, "coding or review task");
    }
    if contains_any(&normalized, DEEP_TERMS) {
        return decision(
            NEMOTRON_35,
            RouteKind::DeepReasoning,
            "deep reasoning or investigation task",
        );
    }
    if normalized.contains('?')
        || contains_any(
            &normalized,
            &[
                "hello",
                "hi ",
                "help me",
                "explain",
                "write",
                "draft",
                "brainstorm",
                "advice",
            ],
        )
    {
        return decision(GPT_OSS, RouteKind::General, "general conversation");
    }
    decision(QWEN_36, RouteKind::Fallback, "stable fallback")
}

fn decision(model: &str, kind: RouteKind, reason: &'static str) -> RouteDecision {
    RouteDecision {
        model: model.to_owned(),
        kind,
        reason,
    }
}

fn contains_any(input: &str, terms: &[&str]) -> bool {
    terms.iter().any(|term| input.contains(term))
}

#[cfg(test)]
mod tests {
    use super::{select, RouteKind, GPT_OSS, NEMOTRON_35, QWEN_36, QWEN_38};

    #[test]
    fn coding_routes_to_new_qwen() {
        let route = select("review this Rust pull request and fix the failing test");
        assert_eq!(route.model, QWEN_38);
        assert_eq!(route.kind, RouteKind::Coding);
    }

    #[test]
    fn deep_work_routes_to_nemotron() {
        let route = select("investigate the root cause and compare the architecture tradeoffs");
        assert_eq!(route.model, NEMOTRON_35);
    }

    #[test]
    fn ordinary_chat_routes_to_gpt_oss() {
        let route = select("What should I cook for dinner tonight?");
        assert_eq!(route.model, GPT_OSS);
    }

    #[test]
    fn everyday_words_do_not_masquerade_as_coding_terms() {
        // "decision" contains "ci". While that was a term of its own, any
        // prompt with a word like it silently routed to the coding model.
        let route = select("What is your decision about dinner?");
        assert_eq!(route.model, GPT_OSS);
        assert_eq!(route.kind, RouteKind::General);
    }

    #[test]
    fn explicit_ci_phrasing_still_routes_to_coding() {
        assert_eq!(select("is the ci/cd job green?").kind, RouteKind::Coding);
    }

    #[test]
    fn unknown_input_has_stable_fallback() {
        let route = select("summarize this");
        assert_eq!(route.model, QWEN_36);
    }
}
