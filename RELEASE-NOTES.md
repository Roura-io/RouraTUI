# rouraTUI v3.0.9

Weekly polish roll-up for the week ending 2026-08-13.

## Changes

- chore(polish): note the coupling between HEADER_HEIGHT and the header block

## Validation

- `cargo fmt --all -- --check`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace` after each individual merge and on the integrated tree

---

# rouraTUI v3.0.8

A correctness release. It makes the test suite trustworthy — the workspace has
never been safe to run in parallel, which silently undermined every gate that
depended on it, including this release script — and restores a header line that
has been invisible since RTUI-0027.

## New in 3.0.8

- Adopted evidence-first retrieval (RTUI-0047).

## Fixed in 3.0.8

- Serialized the test suite (RTUI-0049). ~375 call sites across eight crates
  mutate process-global state from test code — `env::set_var`/`remove_var`,
  most often `HOME` and `CLAW_CONFIG_HOME`, plus `env::set_current_dir`. Rust
  runs a binary's tests as threads in one process, so overlapping tests
  corrupted each other. Measured on an idle machine, the parallel suite failed
  2 runs in 5, with a different test each time. Because `cargo test` stops at
  the first failing binary, a single race also skipped 17 of the 43 test
  binaries — runs that looked like partial failures were partial runs. Now
  serialized workspace-wide via `.cargo/config.toml`, which costs about 11
  seconds and applies to every invocation, this release script included.
- Unclipped the session header. RTUI-0027 grew the header to five content
  lines inside a `Borders::ALL` block but raised its constraint only to 6
  rows; five lines plus two borders needs 7, so the keybinding hint line has
  not rendered since. The height is now a named `HEADER_HEIGHT` constant
  showing the 5 + 2 derivation.
- Removed a dead `changed` flag from the in-turn renderer loop. It was
  assigned in six places and discarded via `let _ = changed;`, implying
  conditional redraws that never existed. The unconditional redraw is
  deliberate — the loop is paced at ~20 FPS by `recv_timeout` and the spinner
  animates off `spinner_phase`, so a dirty-flag redraw would freeze the
  spinner exactly while the model is thinking quietly.

## Known issues

- Test serialization is a mitigation, not a cure. The underlying isolation
  debt — those ~375 global-state call sites — remains, and the suite still
  cannot be run in parallel. Paying it down needs its own change.

## Validation

- `cargo fmt --all -- --check`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace` — 43 binaries, 1658 tests, 8 consecutive clean runs

---

# rouraTUI v3.0.7

This release adds the first operator bridge for Rouratui: an approval-aware,
OpenAI-compatible chat server that reuses the same M3 runtime as the
interactive CLI. It also establishes `MAIN.md` as the shared operating and
training contract for Claude, Codex, and Rouratui.

## New in 3.0.7

- Added `rouratui-chat-server` for OpenWebUI-compatible orchestration requests.
- Kept headless turns on a dedicated worker with explicit chat-relayed approval.
- Added shared `MAIN.md`, `AGENTS.md`, and `ROURATUI.md` instruction routing.
- Preserved least-privilege, audit, rollback, and no-focus-stealing requirements.

## Validation

- `cargo fmt --all -- --check`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace`

---

# rouraTUI v2.0.0-alpha.12

Alpha.12 verifies the real Chrome bridge end to end and makes the extension
service worker reconnect automatically after native-host startup races.

## New in alpha.12

- Added native-host reconnect handling on disconnect and startup failure.
- Verified `BrowserStatus` against Chrome and `BrowserSnapshot` against
  `https://example.com` with a real interactive link.

Alpha.11 packages the native browser host and extension with releases and
stages them during `rouratui update`.

## New in alpha.11

- Added the release payload for `rouratui-browser-host` and the Chrome extension.
- Added `scripts/install-browser-bridge.zsh <extension-id>` to register the
  native messaging host for Chrome.
- Added updater staging under `~/.rouratui/bin` and `~/.rouratui/chrome-extension`.
- Kept browser control disabled until `ROURATUI_ENABLE_BROWSER=1` is set.

Alpha.10 connects the existing visible Chrome bridge to first-class RouraTUI
browser tools. Browser tools are disabled by default and require
`ROURATUI_ENABLE_BROWSER=1` after the extension and native host are installed.

## New in alpha.10

- Added opt-in BrowserStatus, BrowserTabs, BrowserSnapshot, BrowserNavigate,
  BrowserPoint, BrowserClick, and BrowserType tools.
- Kept navigation, clicks, and typing behind the existing in-TUI approval
  cards; read-only status, tabs, snapshots, and cursor pointing remain safe.
- Added browser-tool registry coverage and bridge-unavailable diagnostics.

Alpha.9 keeps tool activity and permission decisions inside the full-screen
conversation instead of falling back to raw terminal output.

## New in alpha.9

- Added gold tool activity cards with completed-state indicators.
- Added coral approval cards with tool, required mode, reason, and input detail.
- Added `Y`/Enter allow and `N`/Escape deny controls inside the TUI.
- Expanded footer activity states for thinking, running tools, approvals, and denials.
- Added focused tests for cards and approval decision routing.

## New in alpha.8

Alpha.8 streams each response into the fixed transcript as it arrives and
labels the live response with the active agent's name.

## New in alpha.8

- Connected provider text deltas directly to the full-screen transcript.
- Kept the bottom composer fixed while the active response grows above it.
- Preserved the final response when a provider returns a non-streamed fallback.
- Added focused tests for ordered deltas and agent-labelled transcript output.

## New in alpha.7

Alpha.7 restores the full-screen conversation shell with a multiline composer
permanently pinned to the bottom.

## New in alpha.7

- Added a Claude-inspired coral transcript, header, and status footer.
- Kept responses in the scrolling viewport while preserving composer position.
- Added active agent, permission mode, branch, and activity status at a glance.
- Preserved terminal cleanup when the user exits with Control-C.
- Staged disabled Chrome-control foundations for later prereleases; this release
  neither installs nor enables browser control.

## New in alpha.6

Alpha.6 restores the original unboxed conversation layout and bottom `> `
composer, identifies the active responding agent by its model name, and fixes
duplicate final-answer rendering after streamed responses.

## New in alpha.6

- Restored the original bottom text field and streamlined startup layout.
- Replaced generic Claw response identity with the active agent model name.
- Removed the duplicate copy of each streamed final response.

## New in alpha.5

This release restores the rouraTUI product identity on the imported Rust
runtime, adds the compact conversation view and coral multiline composer, and
introduces verified self-updates with `rouratui update`.

Alpha.5 points the updater and product metadata at the canonical
`Roura-io/RouraTUI` GitHub repository.

## New in alpha.4

- The package and executable are now named `rouratui`.
- The interactive experience uses the rouraTUI conversation card and composer.
- `rouratui update --check` checks the latest GitHub release.
- `rouratui update` downloads the Apple silicon artifact, verifies its SHA-256
  checksum, and atomically replaces the installed binary.

## Included

- Streaming Anthropic and OpenAI-compatible provider flows, including local
  Ollama routing.
- Workspace-scoped file and shell tools with permission enforcement.
- MCP, plugins, skills, sessions, compaction, sub-agents, and structured CLI
  output from the imported Rust kernel.
- Native Apple silicon release binary installed as `rouratui`.
- Local-only launcher targeting Ollama at `10.0.10.3:11434` with
  `qwen3.6:27b-coding-bf16`; no cloud credentials are required.
- Mandatory orchestrator routing for every prompt, with visible TodoWrite
  checklists and synchronous Explore, Plan, or Verification specialists.
- Analysis-only delegation keeps mutations and approvals at the orchestrator
  boundary.
- Preserved Go v1.2.1 history on `legacy/go-v1.2.1`.

## Alpha limitations

- This is the kernel baseline, not full UX parity with the Go rouraTUI.
- Several surfaces and configuration paths still use the inherited `rouratui`
  naming internally.
- The ratatui interface and rouraTUI-specific brief, Slack, network, Xcode, and
  gear integrations remain migration work.

## Validation

- `cargo fmt --all -- --check`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace`
- Clean-environment mock provider parity harness
# 2.0.0-alpha.7

- Restores the full-screen conversation shell with a multiline composer permanently pinned to the bottom.
- Adds a Claude-inspired coral visual treatment, transcript viewport, agent identity, permission mode, branch, and activity footer.
- Keeps responses in the scrolling transcript while preserving composer position and terminal cleanup on exit.
- Stages disabled Chrome-control foundations for later prereleases; no browser-control capability is installed or enabled in this release.
## 2.0.0-alpha.13

- Adds explicit `tabId` targeting to Chrome browser commands so RouraTUI can
  operate on a background tab instead of whichever tab happens to be active.
- Background-tab operations remain auditable and approval-gated; visible cursor
  animation is shown whenever the target tab is active and visible.
## 2.0.0-alpha.14

- Adds explicit `focus: true` support for browser commands targeting a tab.
- Background automation remains the default; callers can opt in to focusing
  the target window/tab when visible cursor feedback is desired.
## 2.0.0-alpha.15

- Moves the orange composer caret into the input field for a familiar prompt experience.
- Adds a live model-state indicator with a lightweight thinking glyph.
- Refreshes the header with product/version, model state, permissions, branch, and session context.
## 2.0.0-alpha.16

- Adds a real animated braille spinner while the model is generating.
- Moves the active loading state to the footer beneath the composer.
- Keeps the model identity in the header and avoids duplicating it in the loading row.
## 2.0.0-alpha.17

- Adds a clearly visible composer caret when the input is focused and empty.
- Uses a coral block caret/underline for typed input so the insertion point remains obvious.
- Refines the composer focus treatment without changing the anchored layout.
## 2.0.0-alpha.18

- Keeps the orange `❯` prompt visible while typing.
- Places the insertion cursor immediately beside the prompt without overlap.
- Gives the composer a little more breathing room for the prompt and input text.
## 2.0.0-alpha.19

- Adds a small visual gap between the orange prompt and insertion caret.
- Deselects the composer caret when clicking outside the input area.
- Restores composer focus on keyboard input.
## 2.0.0-alpha.20

- Keeps the empty placeholder text clean instead of painting it with the block cursor.
- Uses a clear two-space gap between the orange prompt and custom caret.
- Retains the high-contrast cursor once text has been entered.
## 2.0.0-alpha.21

- Replaces the chunky block-like empty-field caret with a thin insertion bar.
- Renders the prompt, spacing, and caret separately for consistent alignment.
## 2.0.0-alpha.22

- Separates the orange `❯` prompt from the actual text insertion point.
- Adds one real blank cell before text begins.
- Keeps the normal cursor exactly at the text start when the composer is empty.
## 2.0.0-alpha.23

- Uses the same one-cell coral block cursor in both empty and typed states.
- Keeps the cursor aligned at the text start after the prompt gap.
## 2.0.0-alpha.24

- Removes the line-wide underline from composer text.
- Keeps only the one-cell cursor block as the active insertion indicator.
## 2.0.0-alpha.25

- Reworks the top metadata into a boxed agent-session header.
- Adds the active working directory to the header.
- Organizes model, status, workspace, mode, branch, and shortcuts into clear rows.
## 3.0.0

- Classifies bash tool calls by their actual command instead of always
  requiring danger-full-access, so plain reads like `ls`/`git status` no
  longer need an unnecessary approval.
- Fixes the TUI dropping all keyboard input for the full duration of a
  turn; PageUp/PageDown now work while the model is thinking, and
  scrollback position is preserved instead of being yanked to the bottom
  on every streamed token.
- Adds a global instruction-file tier: CLAUDE.md/AGENTS.md/RouraTUI.md in
  `$HOME` or `$HOME/.claw` now load into every project's system prompt,
  for standing, cross-project preferences.
- Adds `chat-server`, an OpenAI-compatible bridge that exposes the same
  orchestrator the interactive CLI drives over HTTP, so it can be
  registered as a model connection inside Open WebUI. Tool calls stay
  approval-gated, relayed to the chat client instead of a terminal
  prompt.
