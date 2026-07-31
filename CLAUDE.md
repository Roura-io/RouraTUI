# CLAUDE.md

This file provides guidance to Claw Code (clawcode.dev) when working with code in this repository.

## Detected stack
- Languages: Rust.
- Frameworks: none detected from the supported starter markers.

## Verification
- From the repository root, run Rust formatting with `scripts/fmt.sh` (or `scripts/fmt.sh --check` for CI-style checks). From this `rust/` directory, the equivalent command is `../scripts/fmt.sh`. Root-level `cargo fmt --manifest-path rust/Cargo.toml` is not the supported formatting command.
- From this `rust/` directory, run Rust verification with `cargo clippy --workspace --all-targets -- -D warnings` and `cargo test --workspace`.

## Working agreement
- Prefer small, reviewable changes and keep generated bootstrap files aligned with actual repo workflows.
- Keep shared defaults in `.claw.json`; reserve `.claw/settings.local.json` for machine-local overrides.
- Do not overwrite existing `CLAUDE.md` content automatically; update it intentionally when repo workflows change.

## Roura.io Git workflow

- All Roura.io work belongs in the `Roura-io` GitHub organization.
- `main` contains released versions only. Changes reach it through a release pull request from `dev` with a semantic-version title such as `0.0.1` or `1.0.0`, followed by a matching version tag.
- `dev` is the linear integration branch. Completed feature work reaches `dev` through a pull request after local AI review and project verification.
- Create short-lived work from current `dev` using `feature/`, `fixup/`, `hotfix/`, or `docs/` branches.
- Keep history linear and interactively rebase work onto current `dev` before merge. Do not create merge commits.
- Use commit and feature pull-request subjects in the form `[PROJECT_ACRONYM-####] core|feat|fixup|hotfix|docs: one-line description`.
- Follow every commit subject with a blank line and concise `- ` bullet details.
- Never push directly to a protected `main` or `dev` branch.
