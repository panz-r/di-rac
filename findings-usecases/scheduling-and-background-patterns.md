# Scheduling & Background Patterns — Condensed

**From OpenClaw's cron/heartbeat/webhook systems — for complex agent loops.**

## 3 Scheduler Types

| Type | Persistence | Use |
|------|-------------|-----|
| **Cron** | `jobs.json` | Scheduled reports, recurring tasks |
| **Heartbeat** | Session state | Background monitoring, check-ins |
| **Webhook** | None | External triggers, CI |

## 4 Session Execution Styles

| Style | History | Hook State |
|-------|---------|------------|
| **Main session** | Full conversation | Normal session hooks |
| **Isolated** | Fresh each run | **Hook state starts empty** |
| **Current** | Bound at creation | Accumulated state |
| **Custom** | Named session | Cross-run context |

## Background-Specific Hooks

```
"heartbeat_prompt_contribution" — ONLY on heartbeat turns (not user turns)
"cron_changed" — Schedule lifecycle (added/removed/started/finished)
```

## Skip Conditions (Heartbeat)

`no-tasks-due`, `empty-file`, `busy`, `outside-hours`, `alerts-disabled`

## Retry Config (Cron)

```json5
{ maxAttempts: 3, backoffMs: [60000, 120000, 300000],
  retryOn: ["rate_limit", "overloaded", "network", "server_error"] }
```

## Response Contract

`HEARTBEAT_OK` = nothing to report (filtered, not delivered). Alert text = delivered. Remaining content ≤ `ackMaxChars` (default 300) dropped.

## Cost Optimization

```json5
{ isolatedSession: true,  // ~2-5K tokens vs ~100K
  lightContext: true,      // Only HEARTBEAT.md
  model: "ollama/llama3.2:1b" }
```

## 6 Lessons

1. **Session isolation levels** — support `normal`/`isolated`/`custom` for background work
2. **Background-specific hooks** — separate monitors from user-facing hooks
3. **Skip/retry strategy** — document conditions, policies, permanent errors
4. **Response contract** — define "nothing to report" tokens
5. **Delivery vs execution** — separate "run" from "deliver"
6. **Cost awareness** — isolated sessions + light context + cheaper models
