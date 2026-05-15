# Haystack Pipeline Breakpoints — A Debugger-Inspired Hook Pattern

Haystack provides a fundamentally different approach to agent/pipeline hooks: **breakpoints** that pause execution, capture full state snapshots, and allow resumption — like a debugger.

## Breakpoints vs Traditional Hooks

| Aspect | Traditional Hooks (Pi, Hermes, etc.) | Haystack Breakpoints |
|--------|--------------------------------------|----------------------|
| **Execution** | Fire and continue | **Pause** execution |
| **State access** | Limited context object | **Full pipeline snapshot** (all inputs, outputs, visit counts) |
| **Modification** | Return modified value | Modify snapshot, resume from modified state |
| **Inspection** | Hook-specific event object | **Complete intermediate state** |
| **Resumption** | Not applicable | Resume from exact point with modified state |

## Setting Breakpoints

### On Regular Pipeline Components

```python
from haystack.dataclasses.breakpoints import Breakpoint

break_point = Breakpoint(
    component_name="llm",
    visit_count=0,          # 0 = first visit, 1 = second, etc.
    snapshot_file_path="/path/to/snapshots",  # Optional: save to file
)

try:
    result = pipeline.run(data=input_data, break_point=break_point)
except BreakpointException as e:
    print(f"Breakpoint at component: {e.component}")
    print(f"Component inputs: {e.inputs}")
    print(f"Pipeline results so far: {e.results}")
```

### On Agent Components (Two Types)

Haystack supports two agent breakpoint types:

**1. Chat Generator Breakpoint** — pauses before LLM calls:

```python
chat_bp = Breakpoint(component_name="chat_generator", visit_count=0)
agent_breakpoint = AgentBreakpoint(break_point=chat_bp, agent_name="my_agent")
```

**2. Tool Invoker Breakpoint** — pauses before specific tool execution:

```python
tool_bp = ToolBreakpoint(
    component_name="tool_invoker",
    visit_count=0,
    tool_name="weather_tool",    # Specific tool, or None for any
)
agent_breakpoint = AgentBreakpoint(break_point=tool_bp, agent_name="my_agent")
```

## Snapshot Callbacks

For custom snapshot handling (DB, remote service, custom logging):

```python
def my_snapshot_callback(snapshot: PipelineSnapshot) -> None:
    print(f"Snapshot at component: {snapshot.break_point}")
    # Save to DB, send to API, etc.

try:
    result = pipeline.run(data=input_data, break_point=break_point,
                          snapshot_callback=my_snapshot_callback)
except BreakpointException as e:
    print(f"Breakpoint triggered: {e.component}")
```

## Resuming from Snapshots

From memory:
```python
result = pipeline.run(data={}, pipeline_snapshot=snapshot)
```

From disk:
```python
from haystack.core.pipeline.breakpoint import load_pipeline_snapshot

snapshot = load_pipeline_snapshot("llm_2025_05_03_11_23_23.json")
result = pipeline.run(data={}, pipeline_snapshot=snapshot)
```

## Error Recovery with Snapshots

On pipeline failure, the system automatically captures the last valid state:

```python
from haystack.core.errors import PipelineRuntimeError

try:
    pipeline.run(data=input_data)
except PipelineRuntimeError as e:
    snapshot = e.pipeline_snapshot
    if snapshot:
        outputs = snapshot.pipeline_state.pipeline_outputs
        # Inspect and fix issue, then resume
        result = pipeline.run(data={}, pipeline_snapshot=snapshot)
```

## Unique Design Characteristics

### Snapshot Contains Full State

The `PipelineSnapshot` includes:
- Component inputs at the breakpoint
- Pipeline outputs up to this point
- Visit counts for each component
- Intermediate outputs from all executed components

### Resume with Modified State

Unlike hooks that can only modify return values, breakpoints let you modify the entire captured state before resuming:

```python
snapshot = load_pipeline_snapshot("snapshot.json")
# Modify intermediate outputs
snapshot.pipeline_state.pipeline_outputs["llm"]["replies"] = [modified_reply]
# Resume with modified state
result = pipeline.run(data={}, pipeline_snapshot=snapshot)
```

### Pipeline Component Structure

Haystack pipelines are directed graphs of components. Breakpoints can be set on:
- **Any component** in the pipeline (by component name)
- **Agent ChatGenerator** (before LLM calls)
- **Agent ToolInvoker** (before tool calls, optionally per-tool)

## Comparison with Hook Systems

| Feature | Traditional Hooks | Haystack Breakpoints |
|---------|------------------|---------------------|
| **Interception** | Function callback | **Execution pause** |
| **State visibility** | Hook context object | **Full pipeline snapshot** |
| **State modification** | Return new value | **Modify snapshot, resume** |
| **Conditional triggers** | Inside handler logic | **Visit count + component name** |
| **Error recovery** | try/catch per handler | **Automatic snapshot on failure** |
| **Multiple trigger points** | Per-hook registration | **Per-component + per-visit** |
| **Tool-specific** | Check tool_name in handler | **`ToolBreakpoint(tool_name=...)`** |

## Key Design Lessons

### 1. Pause-and-Resume vs Fire-and-Forget

Traditional hooks fire and the execution continues immediately. Breakpoints **pause** execution, give full access to state, and require explicit resumption.

**Lesson**: Consider whether your DSL needs observational hooks (fire and continue) or interceptive breakpoints (pause, inspect, resume). They serve different use cases and could coexist.

### 2. Visit Count as Hook Condition

Rather than hooking "every time," breakpoints trigger on a specific visit count:

```python
Breakpoint(component_name="llm", visit_count=2)  # Only on the 3rd visit
```

**Lesson**: Hook conditions (visit count, tool name, iteration index) reduce handler boilerplate. Consider supporting conditions in hook registration.

### 3. Full State Snapshots

Breakpoints capture the **entire pipeline state** — not just a context object. This enables debugging of complex multi-component interactions.

**Lesson**: For complex loops, consider whether hooks should receive the full execution state or a curated context object. Full state is more powerful but has privacy/performance implications.

### 4. Error Recovery via Snapshots

Automatic snapshot-on-failure enables "inspect, fix, resume" workflows:

```python
try:
    pipeline.run(data)
except PipelineRuntimeError as e:
    snapshot = e.pipeline_snapshot  # Inspect
    # Fix the issue
    pipeline.run(data={}, pipeline_snapshot=snapshot)  # Resume
```

**Lesson**: If your loop can fail mid-execution, consider automatic state capture for debugging and resumption — not just error logging.

### 5. Tool-Specific Breakpoints

`ToolBreakpoint` with a specific `tool_name` focuses breakpoints on individual tools rather than all tool calls:

```python
ToolBreakpoint(component_name="tool_invoker", tool_name="delete_file")
```

**Lesson**: Tool-specific interception is more useful than "intercept all tool calls and check the name in handler code." Provide native tool filtering.
