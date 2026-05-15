# LangChain Agent Middleware — A Graph-Based Hook Architecture

LangChain provides a middleware system for agents that offers two distinct hook styles within a graph-based execution model. Unlike linear-loop agents (Pi, Hermes), LangChain's agent runs as a LangGraph state machine with explicit node transitions.

## Two Hook Styles

| Style | Hooks | Purpose | Execution Model |
|-------|-------|---------|----------------|
| **Node-style** | `before_agent`, `before_model`, `after_model`, `after_agent` | Sequential observation and state updates | Run sequentially at fixed points |
| **Wrap-style** | `wrap_model_call`, `wrap_tool_call` | Control flow, retry, caching, transformation | Nest around the actual call like middleware |

## 1. Node-Style Hooks

### Execution Points

```
before_agent (once per invocation)
  │
  ▼  Agent loop starts
before_model (before each LLM call)
  │
  ▼  LLM call
after_model (after each response)
  │
  ▼  If tool calls: tool execution
before_model (next iteration)
  │
  ▼  ...repeat...
after_agent (once per invocation)
```

### Signature

```python
@before_model
def my_hook(state: AgentState, runtime: Runtime) -> dict[str, Any] | None:
    # Return dict to merge into agent state
    # Return None for no update
    # Return {"jump_to": "end"} to exit early
    return None
```

### State Updates

Node-style hooks return a dict that is merged into agent state using the graph's reducers:

```python
@after_model(state_schema=TrackingState)
def increment_counter(state: TrackingState, runtime: Runtime) -> dict[str, Any] | None:
    return {"model_call_count": state.get("model_call_count", 0) + 1}
```

### Jump Targets

Hooks can exit early by returning `{"jump_to": target}`:

| Target | Meaning |
|--------|---------|
| `"end"` | Jump to end of agent execution |
| `"tools"` | Jump to tools node |
| `"model"` | Jump to model node |

```python
@before_model(can_jump_to=["end"])
def check_limit(state: AgentState, runtime: Runtime) -> dict[str, Any] | None:
    if len(state["messages"]) >= 50:
        return {"messages": [AIMessage("Limit reached.")], "jump_to": "end"}
    return None
```

## 2. Wrap-Style Hooks

### Signature

```python
@wrap_model_call
def retry_model(request: ModelRequest, handler: Callable) -> ModelResponse:
    for attempt in range(3):
        try:
            return handler(request)
        except:
            if attempt == 2: raise
```

```python
@wrap_tool_call
def monitor_tool(request: ToolCallRequest, handler: Callable) -> ToolMessage | Command:
    print(f"Executing: {request.tool_call['name']}")
    return handler(request)
```

### `ExtendedModelResponse` for State Updates

Wrap-style hooks return `ExtendedModelResponse` to inject state updates alongside the model response:

```python
@wrap_model_call(state_schema=UsageTrackingState)
def track_usage(request, handler) -> ExtendedModelResponse:
    response = handler(request)
    return ExtendedModelResponse(
        model_response=response,
        command=Command(update={"last_model_call_tokens": 150}),
    )
```

Commands compose through reducers: messages are additive, outer middleware wins on conflicting keys.

### `request.override()` for Modifications

Wrap hooks modify the request by calling `request.override()` with changed fields:

```python
# Override model
handler(request.override(model=complex_model))

# Override tools
handler(request.override(tools=relevant_tools))

# Override system message
handler(request.override(system_message=new_system_message))
```

## Registration Methods

### 1. Decorator-Based

```python
from langchain.agents.middleware import before_model, wrap_model_call

@before_model
def my_before(state, runtime): ...

@wrap_model_call
def my_wrap(request, handler): ...

agent = create_agent(model="gpt-5.4", middleware=[my_before, my_wrap])
```

### 2. Class-Based

```python
from langchain.agents.middleware import AgentMiddleware

class LoggingMiddleware(AgentMiddleware):
    def before_model(self, state, runtime):
        print(f"Calling model with {len(state['messages'])} messages")
        return None

    def after_model(self, state, runtime):
        print(f"Model returned: {state['messages'][-1].content}")
        return None

    async def abefore_model(self, state, runtime):
        # Async version
        return None

agent = create_agent(model="gpt-5.4", middleware=[LoggingMiddleware()])
```

## Execution Order

```python
agent = create_agent(middleware=[m1, m2, m3])
```

```
before_* hooks:  m1 → m2 → m3           (registration order)
after_* hooks:   m3 → m2 → m1           (reverse order)
wrap_* hooks:    m1 wraps m2 wraps m3    (nested)
```

## Custom State Schema

Middleware can extend agent state with custom fields:

```python
from typing_extensions import NotRequired

class CustomState(AgentState):
    model_call_count: NotRequired[int]
    user_id: NotRequired[str]

@before_model(state_schema=CustomState)
def check_limit(state: CustomState, runtime):
    return None if state.get("model_call_count", 0) < 10 else {"jump_to": "end"}
```

## Dynamic Prompt Pattern

```python
@wrap_model_call
def add_context(request, handler):
    new_content = list(request.system_message.content_blocks) + [
        {"type": "text", "text": "Additional context."}
    ]
    new_system_message = SystemMessage(content=new_content)
    return handler(request.override(system_message=new_system_message))
```

## Dynamic Tool Selection

```python
@wrap_model_call
def select_tools(request, handler):
    relevant = select_relevant_tools(request.state, request.runtime)
    return handler(request.override(tools=relevant))
```

## Dynamic Model Selection

```python
complex_model = init_chat_model("claude-sonnet-4-6")
simple_model = init_chat_model("claude-haiku-4-5")

@wrap_model_call
def dynamic_model(request, handler):
    model = complex_model if len(request.messages) > 10 else simple_model
    return handler(request.override(model=model))
```

## Key Design Characteristics

### Graph-Based, Not Loop-Based

LangChain's agent executes as a LangGraph state machine. Middleware hooks fire at node boundaries, not at implicit loop iterations. `before_model` and `after_model` fire per LLM call within the graph, not per "turn" in the traditional sense.

### State Update via Reducers

Returned dicts are merged into agent state through LangGraph's reducer system. This means:
- Messages are additive (append-based reducer)
- Non-reducer fields use last-writer-wins
- Custom reducers can be defined for specific fields

### `request.override()` Pattern

Rather than mutating arguments in-place (Pi's approach) or returning replacement values (Hermes's approach), LangChain provides an explicit `request.override()` method that returns a new request with changed fields:

```python
# Pi: event.input.command = "safe"  (mutation)
# Hermes: return {"context": "..."}  (return value)
# LangChain: handler(request.override(model=new_model))  (override)
```

This is immutable — the original request is not modified.

### Sync/Async Pairs

Class-based middleware can define both sync and async versions:

```python
class MyMiddleware(AgentMiddleware):
    def before_model(self, state, runtime): ...     # sync
    async def abefore_model(self, state, runtime): ...  # async
```

The runtime calls the appropriate version based on execution context.

## Use Cases

| Use Case | Hook | Pattern |
|----------|------|---------|
| Iteration limit | `before_model` | Check state, `jump_to: "end"` |
| Dynamic prompt | `wrap_model_call` | `request.override(system_message=...)` |
| Dynamic tools | `wrap_model_call` | `request.override(tools=...)` |
| Dynamic model | `wrap_model_call` | `request.override(model=...)` |
| Retry logic | `wrap_model_call` | Try handler in loop |
| Token tracking | `wrap_model_call` | `ExtendedModelResponse` with Command |
| Prompt caching | `wrap_model_call` | Add `cache_control` to content blocks |
| Monitoring | `wrap_tool_call` | Log before/after handler call |
| Short-circuit | `before_model` | Return messages + `jump_to: "end"` |

## Comparison with Other Systems

| Dimension | Pi | Hermes | CrewAI | LangChain |
|-----------|----|--------|--------|-----------|
| **Execution model** | Linear loop | Linear loop | Linear loop | Graph (LangGraph) |
| **Hook styles** | Event-based | Event-based | Event-based | Node-style + Wrap-style |
| **Block protocol** | `{ block: true }` | `{"action": "block"}` | `return False` | `jump_to: "end"` |
| **State updates** | Context mutation | Return values | Context mutation | Reducer-merged dict |
| **Request modification** | In-place mutation | Transform hooks | In-place mutation | `request.override()` |
| **Middleware composition** | Registration order | Registration order | Registration order | Nested wraps + ordered nodes |
| **Async hooks** | Implicit (async) | Explicit (async/def) | Async supported | Sync/async pairs |
| **Custom state** | Extension context | Plugin variables | Hook context | Custom AgentState schema |

## Key Takeaways for DSL Design

### 1. Node-Style vs Wrap-Style Separation

LangChain's two styles serve different purposes:
- **Node-style**: Sequential, fire-and-forget logging/validation. Simple return dict for state updates.
- **Wrap-style**: Around each call, can short-circuit, retry, or transform. Has access to the handler function.

**Lesson**: Consider whether your DSL needs both sequential hooks (fire at points in the loop) and wrapping hooks (around each call with access to the execution function).

### 2. `request.override()` for Immutable Modification

Rather than mutating arguments in-place, LangChain provides `request.override()` which returns a new immutable request:

```python
handler(request.override(model=new_model, tools=subset, system_message=msg))
```

**Lesson**: Immutable modification is safer and easier to reason about than in-place mutation. Consider an `override()` pattern for hook-based request modification.

### 3. State Updates via Reducers

Returned dicts are merged through graph reducers, not directly applied:

```python
return {"model_call_count": 5}  # merged via reducer
```

**Lesson**: If your loop has complex state, use a reducer pattern for state updates rather than direct assignment. This enables additive behavior for collections and conflict resolution for scalar fields.

### 4. Graph-Based Hook Points

Unlike linear loops, LangChain hooks fire at graph node boundaries. `before_model` fires before each LLM call, even if the agent loops back to the model multiple times within one invocation.

**Lesson**: If your loop is graph-based (with explicit nodes and edges), define hooks at node boundaries rather than at implicit loop positions.

### 5. Nested Wrap-Style Middleware

Wrap-style middleware composes like onion layers:

```python
middleware = [m1, m2, m3]
# Execution: m1.wrap → m2.wrap → m3.wrap → handler → m3.after → m2.after → m1.after
```

**Lesson**: Nested wrapping is a natural pattern for cross-cutting concerns where order matters (auth outside, caching inside, actual call innermost).
