# rouraTUI v2.0.0-alpha.2

This is the first runnable Rust migration release.

## Included

- Streaming Anthropic and OpenAI-compatible provider flows, including local
  Ollama routing.
- Workspace-scoped file and shell tools with permission enforcement.
- MCP, plugins, skills, sessions, compaction, sub-agents, and structured CLI
  output from the imported Rust kernel.
- Native Apple silicon release binary installed as `rouratui`.
- Local-only launcher targeting Ollama at `10.0.10.3:11434` with
  `qwen3.6:27b-coding-bf16`; no cloud credentials are required.
- Preserved Go v1.2.1 history on `legacy/go-v1.2.1`.

## Alpha limitations

- This is the kernel baseline, not full UX parity with the Go rouraTUI.
- Several surfaces and configuration paths still use the inherited `claw`
  naming internally.
- The ratatui interface and rouraTUI-specific brief, Slack, network, Xcode, and
  gear integrations remain migration work.

## Validation

- `cargo fmt --all -- --check`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace`
- Clean-environment mock provider parity harness
