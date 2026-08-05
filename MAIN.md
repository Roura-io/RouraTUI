# MAIN.md

This file is the shared source of truth for Claude, Codex, and rouratui when working in this repository.

`CLAUDE.md`, `AGENTS.md`, and `ROURATUI.md` intentionally point to this file. Read `MAIN.md` before making changes.

## Mission and operating principles

- Protect family connectivity and preserve the time and reliability of the Roura.io household and business systems.
- Prefer secure, reversible, documented changes.
- Inspect before changing. State assumptions and distinguish observations from recommendations.
- Never expose or commit passwords, private keys, tokens, recovery codes, or sensitive documents.
- Do not delete, reset, overwrite, restart, or publish anything without explicit scope and a recoverable plan.
- Keep infrastructure access separate from GitHub identities.

## Identity model

- `elGordoRoura`: the user's personal GitHub identity; no automation should use it.
- `roura-pair`: the rouratui GitHub identity; used by rouratui on M3.
- `roura-ai`: the shared GitHub identity for Codex and Claude workflows.
- Infrastructure SSH identities are separate from all GitHub identities.

GitHub account aliases must use explicit SSH `Host` entries and `IdentitiesOnly yes`; never rely on whichever default key happens to exist.

## Infrastructure inventory

| System | Address | User | Role |
| --- | --- | --- | --- |
| M3 Studio | `10.0.10.3` | `roura-io-server` | Primary development, Ollama, rouratui |
| UNAS | `10.0.2.2` | `root` | Storage and repository mirrors |
| Pi-infra | `10.0.10.4` | `roura-io` | Home Assistant, n8n, public services |
| UDM Pro Max - Roura-io | `10.0.0.1` | `root` | Primary network/security gateway |
| UDM SE - Alpine | `192.168.0.1` | `root` | Alpine network/security gateway and cameras |
| VPS | `72.60.173.8` | `root` | Hostinger VPS; harden before production use |

The M1 and Mac mini are not yet part of the active inventory. Add stable hostnames or DHCP reservations before depending on them.

## M3 services

- Ollama is the local model runtime.
- rouratui is the orchestration CLI/service.
- M3 is the canonical development machine; other machines may run beta clients or deployment targets.

When auditing services, record bind address, authentication, launch mechanism, logs, backup coverage, and upgrade path.

## Roura.io Git workflow

- All Roura.io work belongs in the `Roura-io` GitHub organization.
- `main` contains released versions only. Changes reach it through a release pull request from `dev` with a semantic-version title such as `0.0.1` or `1.0.0`, followed by a matching version tag.
- `dev` is the linear integration branch. Completed feature work reaches `dev` through a pull request after local AI review and project verification.
- Create short-lived work from current `dev` using `feature/`, `fixup/`, `hotfix/`, or `docs/` branches.
- Keep history linear and interactively rebase work onto current `dev` before merge. Do not create merge commits.
- Use commit and feature pull-request subjects in the form `[PROJECT_ACRONYM-####] core|feat|fixup|hotfix|docs: one-line description`.
- Follow every commit subject with a blank line and concise `- ` bullet details.
- Never push directly to a protected `main` or `dev` branch.

## Rouratui operator-parity contract

Rouratui is the local M3 operator counterpart to Claude and Codex. Its goal is
to make the same approved workflows available from the user’s own hardware,
not to bypass review or grant itself unrestricted authority.

- Prefer capability adapters with explicit scopes: SSH, GitHub, repository work,
  infrastructure inspection, browser automation, and operational reporting.
- Every new capability must document its command surface, allowed targets,
  approval boundary, audit output, failure behavior, and rollback path.
- Reuse the same identities and least-privilege model established for Claude
  and Codex; never copy private keys or tokens into source, prompts, logs, or
  release artifacts.
- Treat `MAIN.md` as the shared training context. When a workflow is proven,
  record it here and add a focused Rouratui test or playbook before release.
- Background automation must not steal keyboard or mouse focus from the user.
- Destructive, external, credential-changing, or production-impacting actions
  require an explicit approval step and a recoverable plan.

## Verification

- From the repository root, run Rust formatting with `scripts/fmt.sh` or `scripts/fmt.sh --check` for CI-style validation.
- Run Rust verification with `cargo clippy --workspace --all-targets -- -D warnings` and `cargo test --workspace`.
- For infrastructure changes, verify the intended service, logs, backup path, and rollback path.

## Change record

Record material changes in commits or a dated operational note. Include what changed, why, how it was verified, and how to undo it.
