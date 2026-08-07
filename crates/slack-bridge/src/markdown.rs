//! Converts the model's CommonMark-flavored replies into Slack's `mrkdwn`
//! syntax before posting. The system prompt in `main.rs` already asks the
//! model to emit `mrkdwn` directly, but it doesn't reliably comply — this
//! is the enforcement backstop, run unconditionally on every reply.

use std::sync::LazyLock;

use regex::Regex;

static CODE_FENCE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?s)```.*?```").unwrap());
static INLINE_CODE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"`[^`\n]+`").unwrap());
static HEADER: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?m)^#{1,6}[ \t]+(.+)$").unwrap());
static LINK: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\[([^\]\n]+)\]\((https?://[^)\s]+)\)").unwrap());
static BOLD_STAR: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\*\*([^*\n]+?)\*\*").unwrap());
static BOLD_UNDERSCORE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"__([^_\n]+?)__").unwrap());

pub fn to_slack_mrkdwn(input: &str) -> String {
    transform_outside_code(input, |segment| {
        let text = convert_tables(segment);
        let text = HEADER.replace_all(&text, "*$1*").into_owned();
        let text = LINK.replace_all(&text, "<$2|$1>").into_owned();
        let text = BOLD_STAR.replace_all(&text, "*$1*").into_owned();
        BOLD_UNDERSCORE.replace_all(&text, "*$1*").into_owned()
    })
}

/// Applies `transform` to every part of `input` that falls outside a fenced
/// or inline code span, leaving code spans byte-for-byte untouched.
fn transform_outside_code(input: &str, transform: impl Fn(&str) -> String) -> String {
    let mut protected: Vec<(usize, usize)> = CODE_FENCE
        .find_iter(input)
        .map(|m| (m.start(), m.end()))
        .collect();
    for m in INLINE_CODE.find_iter(input) {
        let already_covered = protected
            .iter()
            .any(|&(start, end)| m.start() >= start && m.end() <= end);
        if !already_covered {
            protected.push((m.start(), m.end()));
        }
    }
    protected.sort_unstable();

    let mut result = String::with_capacity(input.len());
    let mut cursor = 0;
    for (start, end) in protected {
        if start > cursor {
            result.push_str(&transform(&input[cursor..start]));
        }
        result.push_str(&input[start..end]);
        cursor = end;
    }
    if cursor < input.len() {
        result.push_str(&transform(&input[cursor..]));
    }
    result
}

/// Slack doesn't render Markdown tables at all, so each data row becomes a
/// bullet line with `*header*: cell` pairs instead.
fn convert_tables(input: &str) -> String {
    let lines: Vec<&str> = input.lines().collect();
    let mut out: Vec<String> = Vec::with_capacity(lines.len());
    let mut i = 0;
    while i < lines.len() {
        if i + 1 < lines.len()
            && looks_like_table_row(lines[i])
            && looks_like_table_separator(lines[i + 1])
        {
            let headers = split_table_row(lines[i]);
            i += 2;
            while i < lines.len() && looks_like_table_row(lines[i]) {
                let cells = split_table_row(lines[i]);
                let parts: Vec<String> = headers
                    .iter()
                    .zip(cells.iter())
                    .map(|(header, cell)| {
                        if header.is_empty() {
                            cell.clone()
                        } else {
                            format!("*{header}*: {cell}")
                        }
                    })
                    .collect();
                out.push(format!("- {}", parts.join(" \u{2014} ")));
                i += 1;
            }
            continue;
        }
        out.push(lines[i].to_string());
        i += 1;
    }
    // `str::lines()` doesn't report a final trailing newline, so `join`
    // alone would silently drop it (and, for callers that concatenate this
    // output next to an unrelated span, run two lines together).
    let mut result = out.join("\n");
    if input.ends_with('\n') {
        result.push('\n');
    }
    result
}

fn looks_like_table_row(line: &str) -> bool {
    line.trim().starts_with('|') && line.trim().len() > 1
}

fn looks_like_table_separator(line: &str) -> bool {
    let trimmed = line.trim().trim_matches('|');
    !trimmed.is_empty() && trimmed.chars().all(|c| matches!(c, '-' | ':' | '|' | ' '))
}

fn split_table_row(line: &str) -> Vec<String> {
    line.trim()
        .trim_matches('|')
        .split('|')
        .map(|cell| cell.trim().to_string())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::to_slack_mrkdwn;

    #[test]
    fn converts_double_asterisk_bold_to_single_asterisk() {
        assert_eq!(
            to_slack_mrkdwn("The **Yankees** are playing **today**"),
            "The *Yankees* are playing *today*"
        );
    }

    #[test]
    fn converts_double_underscore_bold_to_single_asterisk() {
        assert_eq!(to_slack_mrkdwn("__bold__ text"), "*bold* text");
    }

    #[test]
    fn converts_markdown_links_to_slack_link_syntax() {
        assert_eq!(
            to_slack_mrkdwn("watch on [DAZN](https://dazn.com)"),
            "watch on <https://dazn.com|DAZN>"
        );
    }

    #[test]
    fn converts_headers_to_bold_lines() {
        assert_eq!(
            to_slack_mrkdwn("### Where to watch\nDAZN"),
            "*Where to watch*\nDAZN"
        );
    }

    #[test]
    fn leaves_bullet_lists_untouched() {
        let input = "- **Matchup:** Braves at Yankees\n- **Time:** 7:05 PM ET";
        assert_eq!(
            to_slack_mrkdwn(input),
            "- *Matchup:* Braves at Yankees\n- *Time:* 7:05 PM ET"
        );
    }

    #[test]
    fn converts_a_simple_table_to_a_bullet_list() {
        let input = "| Broadcast | Platform |\n|---|---|\n| YES Network | DAZN |";
        assert_eq!(
            to_slack_mrkdwn(input),
            "- *Broadcast*: YES Network \u{2014} *Platform*: DAZN"
        );
    }

    #[test]
    fn does_not_touch_bold_markers_inside_fenced_code_blocks() {
        let input = "before\n```\nlet x = **not bold**;\n```\nafter **bold**";
        assert_eq!(
            to_slack_mrkdwn(input),
            "before\n```\nlet x = **not bold**;\n```\nafter *bold*"
        );
    }

    #[test]
    fn does_not_touch_bold_markers_inside_inline_code_spans() {
        assert_eq!(
            to_slack_mrkdwn("run `echo **not bold**` then **do bold**"),
            "run `echo **not bold**` then *do bold*"
        );
    }

    #[test]
    fn leaves_already_correct_slack_mrkdwn_unchanged() {
        let input = "*bold* and _italic_ and <https://example.com|a link>";
        assert_eq!(to_slack_mrkdwn(input), input);
    }
}
