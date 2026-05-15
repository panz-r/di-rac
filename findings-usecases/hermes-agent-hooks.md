# Hermes Agent Hook System — Condensed Reference

A Python agent framework with three parallel hook systems. 110k+ GitHub stars.

## Three Hook Systems

| System | Registration | Runs In | Language |
|--------|-------------|---------|----------|
| **Plugin hooks** | `ctx.register_hook(event, callback)` | CLI + Gateway | Python |
| **Gateway hooks** | `HOOK.yaml` + `handler.py` in `~/.hermes/hooks/` | Gateway only | Python |
| **Shell hooks** | `hooks:` block in `config.yaml` | CLI + Gateway | Any (subprocess) |

All three are non-blocking (errors caught and logged, never crash).

## Plugin Hooks — 15 Events

| Hook | Can Block? | Return Value |
|------|-----------|-------------|
| `pre_tool_call` | Yes | `{"action": "block", "message"}` |
| `post_tool_call` | No | ignored |
| `pre_llm_call` | No | `{"context": str}` (injected into **user message**) |
| `post_llm_call` | No | ignored (successful turns only) |
| `on_session_start/end/finalize/reset` | No | ignored |
| `subagent_stop` | No | ignored (per child) |
| `pre_gateway_dispatch` | Yes | `skip/rewrite/allow` |
| `pre/post_approval_request` | No | ignored |
| `transform_tool_result` | No | `str` to replace, `None` to keep |
| `transform_terminal_output` | No | `str` to replace (pre-truncation) |
| `transform_llm_output` | No | `str` to replace (pre-delivery) |

## Three-Stage Transform Pipeline

```
Terminal tool: raw output → transform_terminal_output → truncation → ANSI strip → redact → model
Other tools:   raw result → transform_tool_result → model
LLM output:    final text → transform_llm_output → user
```

Each stage fires at a different point and receives different context.

## Context Injection Philosophy

`pre_llm_call` injects context into the **user message**, not the system prompt:

```python
def memory_recall(session_id, user_message, **kwargs):
    memories = retrieve(user_message)
    if not memories:
        return None
    return {"context": "Recalled:\n" + "\n".join(f"- {m}" for m in memories)}
```

**Why**: Preserves the prompt cache — system prompt stays identical across turns. The system prompt is the framework's territory; plugins contribute alongside user input.

## Block Protocol

```python
def dangerous_tool_block(tool_name, args, **kwargs):
    if tool_name == "terminal" and "rm -rf" in args.get("command", ""):
        return {"action": "block", "message": "Dangerous command blocked"}
    # Any other return = allow

def register(ctx):
    ctx.register_hook("pre_tool_call", dangerous_tool_block)
```

First matching block wins (plugins first, then shell hooks).

## Gateway Dispatch Interception

```python
def deny_unauthorized_dms(event, **kwargs):
    src = event.source
    if src.chat_type == "dm" and not _is_approved_user(src.user_id):
        return {"action": "skip", "reason": "unauthorized-dm"}
    return None  # allow

def buffer_ambient(event, **kwargs):
    key = (event.source.platform, event.source.chat_id)
    buf = _buffers.setdefault(key, [])
    if _bot_mentioned(event.text):
        combined = "\n".join(buf + [event.text]); buf.clear()
        return {"action": "rewrite", "text": combined}
    buf.append(event.text)
    return {"action": "skip", "reason": "buffered"}
```

Three dispatch actions: `skip` (drop silently), `rewrite` (modify then continue), `allow` (normal flow).

## Gateway Hooks

Directory-based: `~/.hermes/hooks/<name>/HOOK.yaml` + `handler.py`

```yaml
# HOOK.yaml
name: command-logger
events:
  - command:*
```

```python
# handler.py
async def handle(event_type: str, context: dict):
    # Must be named 'handle'
    pass
```

Gateway events: `gateway:startup`, `session:start/end/reset`, `agent:start/step/end`, `command:*` (with wildcard matching).

## Shell Hooks

Config-driven subprocess hooks in any language:

```yaml
hooks:
  pre_tool_call:
    - matcher: "terminal"
      command: "~/.hermes/agent-hooks/check.sh"
      timeout: 10
  post_tool_call:
    - command: "~/.hermes/agent-hooks/log.sh"
```

JSON stdin/stdout protocol:
- stdin: `{"hook_event_name": "pre_tool_call", "tool_name": "terminal", "tool_input": {"command": "..."}}`
- stdout: `{"action": "block", "message": "..."}` (for blocking hooks)

Shell hooks can block tools, inject context, and observe events. Timeout defaults to 60s, capped at 300s.

## Plugin System

### Plugin Types

| Type | Selection | Purpose |
|------|-----------|---------|
| General | Multi-select | Tools, hooks, commands |
| Memory | Single-select (exclusive) | Memory backend |
| Context engine | Single-select (exclusive) | Context compression |
| Model provider | Multi-register, user picks | Inference backends |

### Plugin Registration API

```python
def register(ctx):
    ctx.register_tool(name="my_tool", schema={...}, handler=fn)
    ctx.register_hook("pre_tool_call", callback)
    ctx.register_command(name="my_cmd", handler=fn, description="...")
    ctx.register_skill("my-skill", path)
    ctx.inject_message("Hello", role="user")  # Interrupts mid-turn
    ctx.llm.complete(messages, ...)           # Borrow user's model/auth
```

### Plugin Discovery

| Source | Path |
|--------|------|
| Bundled | `<repo>/plugins/` |
| User | `~/.hermes/plugins/` |
| Project | `.hermes/plugins/` (requires `HERMES_ENABLE_PROJECT_PLUGINS=true`) |
| pip | `hermes_agent.plugins` entry_points |

Opt-in via `plugins.enabled` allowlist. Sub-category routing: `plugins/platforms/`, `plugins/memory/`, `plugins/context_engine/`, `plugins/model-providers/`.

## Key Design Lessons

1. **Context in user message, not system prompt** — preserves prompt cache
2. **Three transform stages** — terminal output → tool result → LLM output
3. **`**kwargs` forward compatibility** — all callbacks accept extra kwargs
4. **Shell hooks for any-language** — subprocess-based, JSON protocol
5. **Gateway dispatch interception** — `skip/rewrite/allow` for message-flow policy
6. **Approval lifecycle hooks** — `pre_approval_request` + `post_approval_response`
