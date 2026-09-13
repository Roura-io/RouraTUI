//! Stable model-routing policy shared by RouraTUI surfaces.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RouteKind {
    Coding,
    DeepReasoning,
    General,
    /// No rule matched, so the stable default model was used.
    Fallback,
    /// Routing was skipped entirely because the caller pinned a model.
    Explicit,
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

const GENERAL_TERMS: &[&str] = &[
    "hello",
    "help me",
    "explain",
    "write",
    "draft",
    "brainstorm",
    "advice",
];

/// Terms too short to match as substrings. The lists above are all long
/// enough that an accidental hit is unlikely and a stem hit is welcome
/// ("test" in "tests", "repo" in "repos"), but a two-letter term is buried
/// inside ordinary words — "ci" in "recipe" and "decision", "pr" in
/// "surprise", "hi" in "sushi" — so these are matched as whole words. The
/// previous guard was a trailing space ("pr "), which both fired on
/// unrelated text and missed the common case of the term ending a sentence.
const CODING_WORD_TERMS: &[&str] = &["ci", "pr"];
const GENERAL_WORD_TERMS: &[&str] = &["hi"];

/// Select a model for a new conversation. The caller should keep the result
/// sticky for follow-up turns so a tool-using task does not change models.
#[must_use]
pub fn select(prompt: &str) -> RouteDecision {
    let normalized = prompt.to_ascii_lowercase();
    if contains_any(&normalized, CODING_TERMS) || contains_word_any(&normalized, CODING_WORD_TERMS)
    {
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
        || contains_any(&normalized, GENERAL_TERMS)
        || contains_word_any(&normalized, GENERAL_WORD_TERMS)
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

/// `input` is already ASCII-lowercased; anything that is not an ASCII
/// alphanumeric separates words, so punctuation-hugged terms ("the PR.",
/// "ci/cd") still match.
fn contains_word_any(input: &str, terms: &[&str]) -> bool {
    input
        .split(|character: char| !character.is_ascii_alphanumeric())
        .any(|word| terms.contains(&word))
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
    fn short_terms_do_not_match_inside_ordinary_words() {
        // "recipe" contains "ci", "surprise" contains "pr", "sushi" contains
        // "hi" — none of them are about code.
        for prompt in [
            "what is a good recipe for dinner tonight",
            "plan a surprise party for my sushi loving brother",
        ] {
            assert_ne!(select(prompt).kind, RouteKind::Coding, "prompt: {prompt}");
        }
    }

    #[test]
    fn short_terms_still_match_as_whole_words() {
        assert_eq!(select("is ci green").kind, RouteKind::Coding);
        assert_eq!(select("take a look at the pr.").kind, RouteKind::Coding);
        assert_eq!(select("hi").kind, RouteKind::General);
    }
}
