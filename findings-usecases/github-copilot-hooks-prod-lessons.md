# GitHub Copilot Hooks & Production Deployment Lessons

**Real-world usage patterns and hard-won production lessons from the agent hooks ecosystem.**

## Part 1: GitHub Copilot Hooks

GitHub Copilot provides hooks for coding agents that fire at tool-use time — intercepting agents before they write files, run commands, or make changes.

### 6 Hook Events (Coding Agent + CLI)

| Event | When | Can Block? |
|-------|------|-----------|
| `preToolUse` | Before tool execution | Yes |
| `postToolUse` | After tool completes | No |
| `onNotification` | Notification sent | No |
| `onStart` | Session starts | No |
| `onStop` | Session ends | No |
| `onCompletion` | Generation completes | No |

### Hook Configuration (JSON-based, per-repo)

```json
// .github/copilot-hooks/my-layer-check.json
{
    "trigger": "preToolUse",
    "command": "./hooks/check-layers.sh",
    "timeout": 10000,
    "failOnError": true
}
```

### Key Insight: Hooks as Enforcement, Not Just Observation

From a practitioner's blog post about controlling AI agents:

> "Instructions tell agents what to do. Agent hooks ensure they actually do it."
> — "Agent Hooks: The Secret to Controlling AI Agents in Your Codebase"

The author built pre-tool-use hooks for two purposes:
1. **Enforce layer import policies** — Each file belongs to a numbered layer (L0-L7), can only import from lower layers
2. **Enforce mock policies** — Test files can only mock what their test type allows

**Result**: "Layer violations went from constant to zero. My code reviews focus on logic and design instead of architecture violations."

### The Three Pillars of Agentic DevOps

| Pillar | Purpose | Example |
|--------|---------|---------|
| **Enablement** | Give agents knowledge and tools | Instructions, docs, custom agents, MCP servers |
| **Enforcement** | Make it impossible to break rules | **Agent hooks**, specs, orchestration layers |
| **Final Gate** | Traditional CI/CD | Tests, static analysis, security scanning |

> "Most teams have pillar 3. Some have pillar 1. Almost nobody has pillar 2."

### What to Enforce with Hooks

- Import restrictions (architecture layers)
- API usage patterns (no deprecated calls)
- File organization rules (co-located tests)
- Naming conventions (event handlers start with `on`)
- Security patterns (no hardcoded credentials)
- Mock policies (test quality enforcement)

### Differences from Letta's Hook System

| Aspect | GitHub Copilot Hooks | Letta Hooks |
|--------|---------------------|-------------|
| Configuration | JSON files per-repo | JSON settings files |
| Events | 6 (preToolUse, postToolUse, etc.) | 11 (PreToolUse, PostToolUse, Stop, etc.) |
| Can block? | Yes (preToolUse) | Yes (PreToolUse, Stop, etc.) |
| Script language | Shell scripts | Shell scripts or LLM prompt |
| Block protocol | Exit code ≠ 0 | Exit code 2 |
| Scope | Per-repo | Per-user + per-project |

---

## Part 2: Production Deployment Lessons

### The Harness Anti-Pattern

> "The pattern underlying most agent production failures is misallocated engineering effort. Teams invest heavily in model selection and prompt optimization — the visible, exciting parts — and underinvest in the harness — the infrastructure that determines whether the system actually works."

**Lesson**: The agent loop harness (hooks, tool execution, error handling, state management) matters more than model choice or prompt design for production reliability.

### 6 Critical Production Mistakes

From "Agentic AI Engineering Guide: 6 Critical Mistakes" (decodingai.com):

1. **Infinite loops on tool failure** — Hook catches timeout/error, agent re-tries forever
2. **Using training data cutoff as current knowledge** — No hook to inject current context
3. **No circuit breakers on subagent spawning** — Unbounded subagent creation
4. **No budget enforcement for tool calls** — Agent drains API quota
5. **Shared mutable state across hooks** — Race conditions in hook execution order
6. **No observability in hooks** — Failures are silent; debugging is impossible

### The Framework is the Thinnest Layer (Repeated Lesson)

> "The framework is the thinnest layer of your agent stack. Beneath it, you need execution infrastructure that actually works."

Systems that allocate disproportionate effort to framework choice vs execution infrastructure end up with production failures regardless of framework quality.

### Hook Testing Gap

Multiple sources confirm: **hook testing is the most underdeveloped practice** across all systems. No system provides first-class testing utilities for hooks. Teams either:
- Test hooks manually in production
- Build ad-hoc test harnesses
- Skip testing entirely

### Migration Pain Points

Microsoft's migration guides for AutoGen → Agent Framework and Semantic Kernel → Agent Framework reveal common patterns:
- Filter APIs change between versions (hook signatures not stable)
- Event ordering differences cause subtle bugs
- Context object shapes change, breaking extensions
- Plugin reload behavior differs

## Key Design Lessons

1. **Enforcement > Instructions** — Hooks that block violations are more effective than instructions asking agents to follow rules
2. **Invest in the harness** — The agent loop infrastructure (hooks, tools, error handling) matters more than model choice
3. **Circuit breakers are essential** — Unbounded loops and subagent spawning must have built-in limits
4. **Budget enforcement belongs in hooks** — Tool call budgets, token limits, and cost caps should be hook-enforced
5. **Hook observability is critical** — Every hook should emit structured telemetry (ran, blocked, timed out, errored)
6. **Hook signatures must be stable** — Versioned hook APIs prevent migration pain
7. **Test hooks in isolation** — Provide test harnesses and mocking utilities as a first-class feature
