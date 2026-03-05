# OpenSlicky — Project Instructions

## Plans

- Every plan created during plan mode must be saved as a numbered file in `/docs/plans/` (e.g., `002-ci-cd-setup.md`, `003-config-file.md`).
- Plans are the source of truth for implementation decisions and should be committed alongside the code they describe.

## Code Standards

- See `/docs/plans/001-full-stack-scaffold.md` for full coding rules (formatting, linting, error handling, naming, testing, dependencies, git conventions).

## Quick Reference

- `cargo fmt --all` before every commit
- `cargo clippy --workspace -- -D warnings` — treat all warnings as errors
- `cargo test --workspace` — all tests must pass
- Conventional commits: `feat:`, `fix:`, `refactor:`, `docs:`, `test:`, `chore:`

## Development Workflow

### Roles

| Role | Subagent | Responsibility |
|------|----------|----------------|
| Implementer | `codex-rust-engineer` | Writes Rust code, implements features |
| Reviewer | `codex-rust-reviewer` | Reviews Rust code, reports issues in JSON |
| Deep Reviewer | `superpowers:code-reviewer` | Architectural review, type safety, logic errors |
| Coordinator | Claude Code | Orchestrates workflow, triages issues, runs checks |

### Code Implementation

**For any code implementation task:**

1. [Claude Code] Understand requirements — read affected files
2. [Claude Code → Codex] Dispatch `codex-rust-engineer` subagent for implementation
3. [Claude Code] Review and apply the generated code
4. [Claude Code] Verify with `cargo fmt --all` and `cargo clippy --workspace -- -D warnings`
5. Proceed to Code Review

### Code Review

**After any code modification:**

1. [Claude Code → Codex] Dispatch `codex-rust-reviewer` subagent
2. [Claude Code] If FAIL, triage each issue — accept and fix, or reject with justification
3. [Claude Code → Codex] Re-review after fixes until PASS
4. [Claude Code] Run `cargo test --workspace` to ensure all tests pass

### Completion Criteria

Task is NOT complete until:
- Code compiles (`cargo build --workspace`)
- No clippy warnings (`cargo clippy --workspace -- -D warnings`)
- Code is formatted (`cargo fmt --all`)
- Tests pass (`cargo test --workspace`)
- `codex-rust-reviewer` review passes
