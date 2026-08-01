# rouraTUI v2.0.0-alpha.9

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
