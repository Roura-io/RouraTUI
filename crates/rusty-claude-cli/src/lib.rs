#![recursion_limit = "256"]
//! Library facade over the rouratui binary's internals.
//!
//! main.rs is included here as a module (rather than duplicated) so that
//! other crates in the workspace -- currently chat-server -- can reuse the
//! exact same provider client (AnthropicRuntimeClient, which also routes
//! OpenAI-compatible backends such as Ollama), tool executor
//! (CliToolExecutor), and runtime construction (build_runtime) that the
//! interactive CLI uses, instead of re-implementing agent wiring.
//!
//! Passing emit_output: false to build_runtime already produces a fully
//! headless runtime (no terminal rendering), which is exactly what a server
//! context needs.
#[path = "main.rs"]
mod app;

pub use app::{build_runtime, AllowedToolSet, BuiltRuntime, InternalPromptProgressReporter};

// Crate-internal only (not re-exported outside this crate): lets
// crate::X references inside main.rs (including nested test modules),
// which were written assuming main.rs itself is the crate root, keep
// resolving correctly now that it is nested as `app`.
pub(crate) use app::{default_permission_mode, init, load_session_reference, print_version};
