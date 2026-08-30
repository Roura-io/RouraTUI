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
    "ci",
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

/// Terms match at the start of a word, not as bare substrings.
///
/// Plain `contains` fires inside ordinary English: `ci` in "precise", `git` in
/// "digital", `test` in "latest", `ios` in "scenarios", `file` in "profile".
/// `CODING_TERMS` is checked first, so those accidental hits silently outrank
/// every other route and pull plain conversation onto the coding model.
///
/// Only the leading edge is checked, so inflections still match ("tests",
/// "commits", "branches"). The cost is that a term buried behind a prefix
/// ("rebuild") no longer matches, which is the cheaper mistake: a missed coding
/// prompt falls through to `QWEN_36`, itself a coding model, whereas a false
/// coding hit takes ordinary chat away from `GPT_OSS`.
fn contains_any(input: &str, terms: &[&str]) -> bool {
    terms.iter().any(|term| starts_a_word(input, term))
}

fn starts_a_word(input: &str, term: &str) -> bool {
    let bytes = input.as_bytes();
    input
        .match_indices(term)
        .any(|(start, _)| start == 0 || !bytes[start - 1].is_ascii_alphanumeric())
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
    fn unknown_input_has_stable_fallback() {
        let route = select("summarize this");
        assert_eq!(route.model, QWEN_36);
    }

    #[test]
    fn coding_terms_do_not_fire_inside_ordinary_words() {
        // "precise" ends in `ci`, "latest" ends in `test`, "digital" holds
        // `git`, "scenarios" ends in `ios` — none of them are coding prompts.
        let route = select("give me precise advice on the latest digital scenarios");
        assert_eq!(route.kind, RouteKind::General);
        assert_eq!(route.model, GPT_OSS);
    }

    #[test]
    fn coding_terms_still_match_inflected_forms() {
        let route = select("the failing tests are on two branches");
        assert_eq!(route.kind, RouteKind::Coding);
        assert_eq!(route.model, QWEN_38);
    }
}
