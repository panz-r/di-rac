# Developer Experience — Condensed

**What users love, what hurts, what they wish for.**

## What Users Love

| Feature | Why | Source |
|---------|-----|--------|
| Minimal core (4 tools) | "Does not get in your way" | Reddit/HN consensus |
| Extension API | "No forks, no patches" | Medium developer |
| Hot reload (`/reload`) | Iterate without restarting | Community |
| jiti (no compile) | Drop a `.ts` file, it works | Pi users |
| Session trees | Branch without wasting context | Power users |
| Model choice (20+ providers) | Use whatever works | Community |

## Common Pain Points

| Pain Point | Root Cause |
|-----------|------------|
| Lost state on compaction | No compaction-aware state API |
| `session_start` async gotcha | Awaiting blocks startup |
| Stale context after transition | Context captured at registration |
| No hook ordering | Registration-order only |
| Print mode silent failures | `ctx.ui` returns defaults with no warning |
| Extension ecosystem fragmented | 200+ packages, hard to evaluate |

## 8 Most-Wanted Features

Hook testing utilities, hook ordering control, compilation-free plugins, dry-run mode, declarative hook config, versioned hook APIs, centralized marketplace, curated starter pack.

## Production Lessons

1. **Enforcement > instructions** — A pre-tool hook that blocks violations beats 10 pages of agent instructions
2. **Invest in the harness** — Tool execution infrastructure > model choice > prompt design
3. **Circuit breakers essential** — Unbounded loops and subagent spawning need built-in limits
4. **Hook observability is critical** — Every hook should emit structured telemetry
5. **Stable hook signatures** — Versioned APIs prevent migration pain
6. **Test hooks in isolation** — Most underdeveloped practice across all systems
