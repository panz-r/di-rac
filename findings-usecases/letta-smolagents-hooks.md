# Letta & Smolagents — Two More Hook Architectures

Two additional systems with unique approaches: Letta (config-driven shell hooks with LLM-based evaluation) and Smolagents (step-based callbacks with full agent access).

## Part 1: Letta Hooks (Config-Driven Shell Scripts)

Letta (formerly MemGPT) provides hooks configured via JSON settings files — no Python coding required.

### 10 Hook Events

| Hook | When | Can Block? | Config Style |
|------|------|-----------|-------------|
| `PreToolUse` | Before tool execution | Yes | Command or Prompt |
| `PostToolUse` | After tool succeeds | No (can inject context) | Command or Prompt |
| `PostToolUseFailure` | After tool fails | No | Command or Prompt |
| `PermissionRequest` | Permission dialog appears | Yes | Command or Prompt |
| `UserPromptSubmit` | User submits prompt | Yes | Command or Prompt |
| `Notification` | Notification sent | No | Command or Prompt |
| `Stop` | Agent finishes responding | Yes | Command or Prompt |
| `SubagentStop` | Subagent task completes | Yes | Command or Prompt |
| `PreCompact` | Before context compaction | No | Command or Prompt |
| `SessionStart` | Session begins/resumes | No | Command or Prompt |
| `SessionEnd` | Session terminates | No | Command or Prompt |

### Two Hook Types

#### Command Hooks (shell scripts)

```bash
#!/bin/bash
input=$(cat)
command=$(echo "$input" | jq -r '.tool_input.command')

if echo "$command" | grep -qE 'rm\s+.*-rf'; then
    echo "Blocked: rm -rf must be run manually." >&2
    exit 2  # Blocking error
fi
exit 0  # Allow
```

#### Prompt Hooks (LLM-based evaluation)

```json
{
    "type": "prompt",
    "prompt": "Block any Bash commands that modify files outside src/. Allow read-only operations on system files.",
    "model": "haiku",
    "timeout": 30000
}
```

The LLM receives the hook input and decides allow/block based on natural language instructions.

### Exit Code Protocol

| Code | Behavior |
|------|----------|
| 0 | Allow — action proceeds |
| 1 | Non-blocking error — logged, action continues |
| 2 | Blocking error — action stopped, stderr shown to agent |

### Tool Matchers

```json
"PreToolUse": [
    { "matcher": "Bash", "hooks": [...] },
    { "matcher": "Edit|Write", "hooks": [...] },
    { "matcher": "*", "hooks": [...] }
]
```

Patterns: exact name, regex alternation, regex wildcard, `*` for all.

### Configuration Priority

1. `.letta/settings.local.json` (not committed)
2. `.letta/settings.json` (project, can commit)
3. `~/.letta/settings.json` (user global)

All merged; local hooks run first.

### Context Injection (PostToolUse)

`PostToolUse` hooks can inject additional context by printing JSON to stdout:

```json
{"additionalContext": "Tests passed but code coverage dropped to 72%."}
```

This is fed back to the agent as context after the tool completes.

### Reasoning & Message Context

`PostToolUse` and `Stop` hooks include the agent's reasoning:

```json
{
    "event_type": "Stop",
    "preceding_reasoning": "The user asked about project structure...",
    "assistant_message": "Here's an overview...",
    "user_message": "What does this project look like?"
}
```

---

## Part 2: Smolagents Callbacks (HuggingFace)

Smolagents provides a Python callback system for the ReAct agent loop, with callbacks registered per step type.

### 3 Step Types with Callbacks

| Step Type | When | Common Use |
|-----------|------|------------|
| `PlanningStep` | After plan generation (if `planning_interval` set) | Review/modify plans, human-in-the-loop approval |
| `ActionStep` | After each tool execution + observation | Metrics, memory cleanup, screenshots, logging |
| `FinalAnswerStep` | Once at completion | Logging, cleanup, result validation |

### Registration

```python
from smolagents import CodeAgent

def my_callback(memory_step, agent):
    print(f"Step completed: {type(memory_step).__name__}")
    print(f"Agent state: {agent.state}")

agent = CodeAgent(
    tools=[...],
    model=model,
    step_callbacks={
        ActionStep: [my_callback],
        PlanningStep: [plan_review_callback],
    }
)
```

Legacy list format also supported: `step_callbacks=[callback]` registers for `ActionStep` only.

### Callback Signature

```python
def callback(memory_step, agent):
    # memory_step: The completed step (ActionStep, PlanningStep, or FinalAnswerStep)
    # agent: Full agent instance (access memory, state, tools, etc.)
    pass
```

Callbacks receive the **full agent instance**, giving access to all agent properties.

### Agent Lifecycle with Callback Execution

```
Planning Step (if planning_interval):
    → Plan generated
    → PlanningStep callbacks execute (can modify plan, pause for human review)

Action Step (each step):
    → Tool executes → observation recorded
    → ActionStep callbacks execute (metrics, cleanup, logging)

Final Answer Step:
    → Final answer generated
    → FinalAnswerStep callbacks execute
```

### Common Patterns

#### Human-in-the-Loop Plan Review

```python
def plan_review_callback(step, agent):
    print("Plan:", step.plan)
    response = input("Approve plan? (y/n): ")
    if response.lower() != 'y':
        agent.interrupt()  # Pauses execution
```

#### Screenshot Cleanup (Memory Management)

```python
def cleanup_screenshots(step, agent):
    if len(agent.memory.observations) > 5:
        # Keep only last 3 screenshots
        agent.memory.observations = agent.memory.observations[-3:]
```

### Key Design Characteristics

| Aspect | Behavior |
|--------|----------|
| **Callback count** | 3 step types |
| **Registration** | Dict per step type (or legacy list) |
| **Agent access** | Full agent instance passed to callback |
| **Can modify state?** | Yes — memory, agent state, etc. |
| **Error handling** | Exceptions propagate to agent (blocks execution) |
| **Thread safety** | Synchronous, single-threaded |
| **Built-in callback** | `update_metrics` registered automatically for ActionStep |
| **Streaming** | Callbacks fire at same points, after step finalized |

## Cross-System Comparison

| Feature | Letta | Smolagents |
|---------|-------|------------|
| **Hook language** | Shell scripts or natural language (LLM) | Python functions |
| **Configuration** | JSON settings files | Python code |
| **Tool filtering** | Matcher patterns (exact, regex, wildcard) | Per-step type binding |
| **Block protocol** | Exit codes (0/1/2) | `agent.interrupt()` |
| **Context injection** | stdout JSON (`additionalContext`) | Modify agent.memory |
| **Reasoning access** | Yes — `preceding_reasoning` field | Full agent instance |
| **LLM-based hooks** | Yes — prompt hooks evaluate via LLM | No |
| **No-code hooks** | Yes — JSON + shell scripts | No — requires Python |

## Key Design Lessons

### Letta
1. **Config-driven hooks** — no Python coding required, hooks configured in JSON
2. **Two hook types** — shell scripts for performance, prompt hooks for flexible LLM-based decisions
3. **Tool matchers** — declarative tool filtering (exact name, regex alternation, wildcard)
4. **3-value block protocol** — 0=allow, 1=log, 2=block — cleaner than binary allow/deny
5. **Context injection from hooks** — `PostToolUse` can inject `additionalContext` via stdout
6. **Reasoning visibility** — hooks see agent's `preceding_reasoning` for observability

### Smolagents
7. **Step-typed callbacks** — register per step type (PlanningStep/ActionStep/FinalAnswerStep) for precise lifecycle binding
8. **Full agent access** — callbacks receive the complete agent instance for maximum flexibility
9. **Agent interruption** — `agent.interrupt()` from callbacks enables human-in-the-loop
10. **Automatic built-in callback** — monitor metrics registered automatically for ActionStep
