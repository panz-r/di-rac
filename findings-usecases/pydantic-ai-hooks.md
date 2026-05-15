# Pydantic AI Hooks — Condensed

**10 categories × 4 types. Unique skip exceptions pattern.**

## 10 Categories × 4 Types Each

| Category | before | after | wrap | error |
|----------|--------|-------|------|-------|
| Run | `before_run` | `after_run` | `run` | `run_error` |
| Node | `before_node_run` | `after_node_run` | `node_run` | `node_run_error` |
| Model Request | `before_model_request` | `after_model_request` | `model_request` | `model_request_error` |
| Tool Validate | `before_tool_validate` | `after_tool_validate` | `tool_validate` | `tool_validate_error` |
| Tool Execute | `before_tool_execute` | `after_tool_execute` | `tool_execute` | `tool_execute_error` |
| Output Validate | `before_output_validate` | `after_output_validate` | `output_validate` | `output_validate_error` |
| Output Process | `before_output_process` | `after_output_process` | `output_process` | `output_process_error` |
| Tool Prep | `prepare_tools`, `prepare_output_tools` | | | |
| Deferred | `deferred_tool_calls` | | | |
| Event Stream | `run_event_stream` | `event` | | |

Registration: `@hooks.on.before_tool_execute(tools=['email'], timeout=5.0)`.

## Unique Skip Exceptions

| Exception | Effect |
|-----------|--------|
| `SkipModelRequest(response)` | Skip LLM call, return synthetic response |
| `SkipToolValidation(args)` | Skip argument validation |
| `SkipToolExecution(result)` | Skip tool execution, return synthetic result |

## Key Lessons

1. **Category-organized hooks** reduce cognitive overhead — 10 groups × 4 types is easier to navigate than 40 flat events
2. **Skip exceptions** are cleaner than return-value block protocols — just raise and the system handles it
3. **Per-hook `tools=` filter** targets specific tools without boilerplate `if tool_name == "x"`
4. **Wrap hooks with `handler` delegate** enable setup/teardown, retry, caching
5. **Error hooks with raise/return semantics** — raise = propagate, return = recover
