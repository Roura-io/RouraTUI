# rouraTUI v2.0.0-alpha.4

This release restores the rouraTUI product identity on the imported Rust
runtime, adds the compact conversation view and coral multiline composer, and
introduces verified self-updates with `rouratui update`.

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
