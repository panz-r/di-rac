# Agent Loop Hooks — Quick Reference

**Hook catalogs for all 16 studied systems. See individual system files for full details.**

## Pi (25+ events)

```
input → session_start → before_agent_start → [context, before_provider_request, after_provider_response]
→ LLM → [tool_call (can block/mutate input), tool_execution_start, tool_execution_update, tool_execution_end,
  tool_result (field merge)]
→ turn_end → session_shutdown
```

Plus: `session_before_switch/fork/compact/tree` (cancellable), `model_select`, `user_bash`, `resources_discover`.

## OpenClaw (40+ hooks)

| Area | Hooks |
|------|-------|
| **Model** | `before_model_resolve`, `model_call_started/ended` |
| **Prompt** | `agent_turn_prepare`, `before_prompt_build`, `heartbeat_prompt_contribution` |
| **Agent** | `before_agent_run` (block), `before_agent_reply` (synthetic), `before_agent_finalize` (revision), `agent_end` |
| **Tool** | `before_tool_call` (block/approve), `after_tool_call`, `tool_result_persist` |
| **Message** | `message_received`, `message_sending` (cancel), `message_sent`, `before_dispatch` |
| **Session** | `session_start/end`, `before/after_compaction`, `before_reset` |
| **Subagent** | `subagent_spawning/ended` |
| **Lifecycle** | `gateway_start/stop`, `cron_changed`, `before_install` |

## Hermes (15 + 2 extras)

| Plugin hooks | Gateway hooks | Shell hooks |
|-------------|---------------|-------------|
| `pre_tool_call` (block) | `gateway:startup` | Config-driven subprocess |
| `post_tool_call` | `session:start/end/reset` | JSON stdin/stdout |
| `pre_llm_call` (inject context) | `agent:start/step/end` | Any language |
| `post_llm_call` | `command:*` | 3 exit codes |
| `on_session_start/end/finalize/reset` | Wildcard matching | Can block tools |
| `subagent_stop` | | |
| `pre_gateway_dispatch` (skip/rewrite/allow) | | |
| `pre/post_approval_request` | | |
| `transform_tool_result` / `transform_terminal_output` / `transform_llm_output` | | |

## CrewAI (4 + HTTP interceptor)

| Hook | Block | Return |
|------|-------|--------|
| `before_llm_call` | `return False` | None |
| `after_llm_call` | No | `str` to replace |
| `before_tool_call` | `return False` | None |
| `after_tool_call` | No | `str` to replace |

Plus `BaseInterceptor` for httpx-level request/response modification (OpenAI + Anthropic only).

## LangChain (6 in 2 styles)

**Node-style**: `before_agent`, `before_model`, `after_model`, `after_agent`. Return dict for state, `jump_to: "end"` to block.

**Wrap-style**: `wrap_model_call`, `wrap_tool_call`. Receive `handler` — call `handler(request)` to proceed. Use `request.override(model=..., tools=...)`.

## Vercel AI SDK (2 wrap hooks)

`wrapGenerate`, `wrapStream` on `LanguageModelV4Middleware`. Intercept model calls. Used via `wrapLanguageModel()`.

## Semantic Kernel (3 filter types)

`FunctionInvocationFilter`, `PromptRenderFilter`, `AutoFunctionInvocationFilter`. All use `next()` delegate pattern. Can block by skipping `next()`.

## AG2 (4 active + 5 reserved)

| Active | When | Persistence |
|--------|------|-------------|
| `process_message_before_send` | Before sending to another agent | **Permanent** in history |
| `update_agent_state` | Before reply generation | **Permanent** on state |
| `process_last_received_message` | After receiving, before reply | **Agent-local permanent** |
| `process_all_messages_before_reply` | Before reply functions run | **Temporary** (LLM only) |

Reserved (not yet invoked): `safeguard_tool_inputs/outputs`, `safeguard_llm_inputs/outputs`, `safeguard_human_inputs`.

## Pydantic AI (10 categories × 4 types)

Categories: Run, Node, ModelRequest, ToolValidate, ToolExecute, OutputValidate, OutputProcess, ToolPrep, Deferred, EventStream.

Each has: `before_*`, `after_*`, `wrap_*`, `*_error`. Registration via `@hooks.on.*` decorator.

**Skip exceptions**: `SkipModelRequest(response)`, `SkipToolValidation(args)`, `SkipToolExecution(result)`.

## Haystack (breakpoints)

`Breakpoint(component_name, visit_count)` — pauses execution, captures full pipeline snapshot, resume with `pipeline_snapshot`.

Two agent types: `ChatGenerator` (before LLM) and `ToolInvoker` (before tool, per-tool or all).

## Genkit (3 interception points)

`model`, `tool`, `generate`. Registered via `generateMiddleware()`. Composes like onion layers via `use: [middleware]`.

## OpenAI Agents SDK (7 events × 2 scopes)

`RunHooks`: `on_agent_start/end`, `on_llm_start/end`, `on_tool_start/end`, `on_handoff`. `AgentHooks`: same events scoped to one agent.

**Observation only** — cannot block or modify.

## Letta (10 events)

`PreToolUse`, `PostToolUse`, `PostToolUseFailure`, `PermissionRequest`, `UserPromptSubmit`, `Notification`, `Stop`, `SubagentStop`, `PreCompact`, `SessionStart`, `SessionEnd`.

2 types: command hooks (shell scripts, 3 exit codes) and prompt hooks (LLM-based evaluation).

## Smolagents (3 step types)

`PlanningStep`, `ActionStep`, `FinalAnswerStep`. Callbacks receive full agent instance. `agent.interrupt()` for human-in-the-loop.

## Cline SDK (6 core + 16 stages)

```
input → runtime_event → session_start → run_start → iteration_start → turn_start →
before_agent_start → tool_call_before → tool_call_after → turn_end → stop_error →
iteration_end → run_end → session_shutdown → error
```

Hook policies: `mode`, `timeoutMs`, `retries`, `failureMode` (`fail_closed`/`fail_open`).

## Dify (6 plugin types)

Tool, Model, Datasource, Trigger, Agent Strategy, Endpoint. Container-isolated plugins with reverse invocation.
