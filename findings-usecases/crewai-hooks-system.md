# CrewAI Hook System — Condensed Reference

A Python multi-agent orchestration framework with two hook layers.

## Two-Layer Architecture

| Layer | What It Intercepts | Scope |
|-------|-------------------|-------|
| **Agent-Level Hooks** | LLM calls and tool execution | Pre/post LLM and tool calls |
| **HTTP-Level Interceptors** | Raw HTTP to LLM providers | Transport (OpenAI + Anthropic only) |

## Agent-Level Hooks — 4 Types

| Hook | Can Block? | Return |
|------|-----------|--------|
| `before_llm_call` | Yes | `False` blocks, `None`/`True` continues |
| `after_llm_call` | No | `str` to replace response, `None` to keep |
| `before_tool_call` | Yes | `False` blocks, `None`/`True` continues |
| `after_tool_call` | No | `str` to replace result, `None` to keep |

### Block Protocol

```
Pre-hooks execute in registration order:
  Hook 1 → Hook 2 → ... → Hook N
  If ANY returns False → block, remaining hooks skipped
Execution proceeds (if not blocked):
Post-hooks execute in registration order:
  Each can return replacement string or None
```

### Context Objects

```python
class LLMCallHookContext:
    executor: CrewAgentExecutor   # Full executor access
    messages: list                # Mutable (in-place)
    agent: Agent                  # Current agent
    task: Task                    # Current task
    crew: Crew                    # Crew instance
    llm: BaseLLM                  # LLM instance
    iterations: int               # Current iteration
    response: str | None          # Response (post only)

class ToolCallHookContext:
    tool_name: str                # Tool being called
    tool_input: dict              # Mutable (in-place)
    tool: CrewStructuredTool      # Tool instance
    agent: Agent | None           # Agent executing
    task: Task | None             # Current task
    crew: Crew | None             # Crew instance
    tool_result: str | None       # Result (post only)
```

### In-Place Mutation Required

```python
# ✅ Correct:
def sanitize(context):
    context.tool_input['query'] = context.tool_input['query'].lower()

# ❌ Wrong — replaces dict reference:
def wrong(context):
    context.tool_input = {'query': 'new query'}
```

### Human-in-the-Loop

```python
@before_tool_call
def require_approval(context):
    if context.tool_name in SENSITIVE_TOOLS:
        response = context.request_human_input(
            prompt=f"Approve {context.tool_name}?",
        )
        if response.lower() != 'yes':
            return False
    return None
```

## Registration Methods

### 1. Global Decorators (Recommended)

```python
from crewai.hooks import before_tool_call, after_llm_call

@before_tool_call
def safety_check(context):
    if context.tool_name == "delete_db":
        return False
    return None

@after_llm_call
def sanitize(context):
    if "API_KEY" in (context.response or ""):
        return context.response.replace("API_KEY", "[REDACTED]")
    return None
```

### 2. Programmatic Registration

```python
from crewai.hooks import register_before_tool_call_hook, register_after_llm_call_hook

def my_hook(context):
    return None  # Allow

register_before_tool_call_hook(my_hook)
```

### 3. Crew-Scoped Registration

```python
@CrewBase
class MyCrew:
    @before_tool_call_crew
    def validate_inputs(self, context):
        # Only applies to THIS crew
        return None

    @after_tool_call_crew
    def log_results(self, context):
        return None
```

### 4. Hook Management API

```python
from crewai.hooks import (
    clear_all_global_hooks,            # Reset all
    clear_before_llm_call_hooks,       # Clear specific type
    unregister_before_tool_call_hook,  # Remove specific function
    get_before_tool_call_hooks,        # List registered
)
```

## HTTP-Level Interceptors

```python
class BaseInterceptor:
    def on_outbound(self, message: TRequest) -> TRequest:
        """Modify request before it leaves."""
    def on_inbound(self, message: TResponse) -> TResponse:
        """Modify response before it reaches completion."""
```

Provider support: OpenAI ✓, Anthropic ✓, Gemini ✗, Azure ✗, Bedrock ✗.

Implementation: Custom `httpx` transport wrapping the SDK's HTTP client.

## Use Cases

| Use Case | Hook | Pattern |
|----------|------|---------|
| Safety guardrails | `before_tool_call` | Block by tool name/input |
| Human approval | `before_tool_call` | `context.request_human_input()` |
| Input validation | `before_tool_call` | Mutate `tool_input` in-place |
| Result sanitization | `after_tool_call` | Return replacement string |
| Iteration limiting | `before_llm_call` | Check `context.iterations` |
| Cost tracking | `after_llm_call` | Log token counts |
| Rate limiting | `before_tool_call` | Track call frequency |
| Result caching | `after_tool_call` | Store + return cached |

## Key Design Lessons

1. **Block via return value, modify via mutation** — clean separation of concerns
2. **Rich context objects** — full agent/task/crew references enable informed decisions
3. **Hook management API** — `clear_all_global_hooks()`, `unregister_*`, `get_*` — essential for testing
4. **Scoped vs global** — `@before_tool_call_crew` for instance-specific, global decorators for system-wide
5. **HTTP-level + agent-level** — separate infrastructure concerns from business logic
6. **Iteration awareness** — `context.iterations` enables loop guards without external state
