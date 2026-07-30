# Kroa — Agent Context

This repository includes a **static specification** designed to be pasted into AI agent system prompts when generating or repairing Kroa code.

- **Canonical agent context (English):** [`docs/en/agent-spec.md`](docs/en/agent-spec.md)
- **Spanish explanation of the same rules:** [`docs/es/agent-spec.md`](docs/es/agent-spec.md)

## Quick start for agents

```bash
kroa build path/to/file.kroa --message-format json
```

Parse NDJSON diagnostics (`code`, `file`, `line`, `column`, `message`, `help`) and apply the smallest fix. Repeat until exit code 0.

Grammar is strict and unambiguous: use `and` / `or` / `not` (never `&&` / `||` / `!`), spaces only (never tabs).
