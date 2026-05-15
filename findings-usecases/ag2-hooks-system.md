# AG2 (AutoGen) Hook System — Multi-Agent Conversation Hooks

AG2 (formerly AutoGen) by Microsoft provides a hook system designed for **multi-agent conversations**, where hooks fire during message sending, receiving, and reply generation in agent-to-agent communication.

## Per-Agent Hook Architecture

Unlike other systems where hooks are global or per-loop, AG2 hooks are registered **per-agent instance**:

```python
my_agent.register_hook("HOOK_NAME", my_function)
```

Each `ConversableAgent` maintains its own `hook_lists` dictionary. Hooks execute in registration order.

## The 4 Active Hooks

| Hook | When It Fires | Scope of Changes | Purpose |
|------|---------------|-----------------|---------|
| `process_message_before_send` | Before message is sent to another agent | **Permanent** — changes persist in chat history | Transform outgoing messages |
| `update_agent_state` | Before reply generation | **Permanent** on agent state | Update system message, agent config |
| `process_last_received_message` | After receiving a message, before reply | **Permanent** on this agent's view only | Modify the last message for this agent |
| `process_all_messages_before_reply` | Before reply functions run | **Temporary** — changes discarded after reply | Transform full conversation for LLM context |

### Hook Execution Order During a Conversation

```
Agent A sends message to Agent B:
  1. A.process_message_before_send()  ← Transform outgoing message
  2. A sends → B receives
  3. B receives message, appends to B._oai_messages[A]
  4. B.process_last_received_message()  ← Modify last message for B's view
  5. B.generate_reply():
     a. B.process_all_messages_before_reply()  ← Transform all messages (temporary)
     b. B.update_agent_state()  ← Update B's system message
     c. Reply functions run (LLM, tool calls, etc.)
  6. B sends reply → goes to step 1 for B→A
```

### Permanent vs Temporary Changes

A unique aspect of AG2 hooks is the **permanent vs temporary distinction**:

| Hook | Changes Persist? | Visible To |
|------|-----------------|------------|
| `process_message_before_send` | Yes — in chat history | All agents |
| `update_agent_state` | Yes — on agent state | This agent only |
| `process_last_received_message` | Yes — in this agent's `_oai_messages` | This agent only (other agents see original) |
| `process_all_messages_before_reply` | No — discarded after reply | LLM only (during this turn) |

## Hook Signatures

```python
# process_message_before_send
def hook(sender: ConversableAgent, message: Union[dict, str],
         recipient: Agent, silent: bool) -> Union[dict, str]:
    # Return modified message (permanent change)
    return message

# update_agent_state
def hook(agent: ConversableAgent, messages: list[dict]) -> None:
    # Mutate agent state (system_message, etc.)
    agent.update_system_message(new_prompt)

# process_last_received_message
def hook(content: Union[str, list[dict]]) -> str:
    # Return modified content for this agent's view
    return modified_content

# process_all_messages_before_reply
def hook(messages: list[dict]) -> list[dict]:
    # Return modified messages (temporary, for LLM only)
    return modified_messages
```

### `update_agent_state` Special Helper: `UpdateSystemMessage`

```python
from autogen import ConversableAgent, UpdateSystemMessage

agent = ConversableAgent(
    name="calendar",
    llm_config=llm_config,
    update_agent_state_before_reply=[
        UpdateSystemMessage(
            content_updater="You are a calendar agent. Today is {current_date}.",
            context_variables={"current_date": "2025-02-24"}
        )
    ],
)
```

Supports both string templates (with `{variable}` substitution) and callable functions.

## The 5 Reserved "Safeguard" Hooks

AG2 defines 5 safeguard hooks that are **not currently invoked** by default but serve as reserved extension points:

| Hook | Intended Purpose |
|------|-----------------|
| `safeguard_tool_inputs` | Validate tool arguments before execution |
| `safeguard_tool_outputs` | Validate tool results after execution |
| `safeguard_llm_inputs` | Validate/modify messages before LLM API call |
| `safeguard_llm_outputs` | Validate/modify LLM responses |
| `safeguard_human_inputs` | Validate/sanitize human user input |

These can be registered but must be manually invoked in custom agent overrides.

## Sequential Pipeline Processing

All hooks execute sequentially in registration order, with each hook's output becoming the next hook's input:

```python
# Processing pipeline for process_all_messages_before_reply:
def process_all_messages_before_reply(self, messages):
    for hook in self.hook_lists["process_all_messages_before_reply"]:
        messages = hook(messages)
    return messages
```

## Comparison with Other Systems

| Dimension | Pi | Hermes | CrewAI | AG2 |
|-----------|----|--------|--------|-----|
| **Hook scope** | Global | Per-plugin | Global + per-crew | **Per-agent instance** |
| **Hook count** | 25+ | 15+ | 4 | 4 active + 5 reserved |
| **Persistence model** | Custom entries | Transform hooks | Field merge | **Permanent vs temporary** distinction |
| **Message transform** | `tool_result` | `transform_tool_result` | `after_tool_call` | **Send & receive hooks** |
| **State update** | `before_agent_start` | `pre_llm_call` | `before_llm_call` | `update_agent_state` |
| **Multi-agent** | No | No | Yes (crews) | **Yes (native)** |
| **Block protocol** | `{ block: true }` | `{"action": "block"}` | `return False` | Via transform (override message) |
| **Safeguard hooks** | No | No | No | **Yes — 5 reserved slots** |

## Key Takeaways for DSL Design

### 1. Per-Instance Hook Registration

AG2's per-agent hooks are unique — each agent instance has its own hook list. This is natural for multi-agent systems where different agents need different behaviors.

**Lesson**: If your system has multiple agents, consider per-instance hook registration. Global hooks for cross-cutting concerns, per-instance hooks for agent-specific behavior.

### 2. Permanent vs Temporary Changes

AG2 makes the persistence model of each hook explicit:
- **Permanent**: Changes survive in chat history / agent state
- **Temporary**: Changes only for the current LLM call, discarded after

**Lesson**: Make the persistence semantics of each hook point explicit. Hooks that transform messages for the LLM should not permanently alter the conversation history.

### 3. Send and Receive Hooks

AG2 hooks fire at message **send** and **receive** boundaries between agents:

```
Agent A sends → process_message_before_send → Agent B receives → process_last_received_message
```

**Lesson**: If your system has agent-to-agent communication, define hooks at the message boundary (before send, after receive), not just at the LLM call boundary.

### 4. Reserved Safeguard Hooks

AG2 defines safeguard hooks that are **not yet invoked** but are reserved for future use. This is a forward-compatibility pattern:

```python
# Defined in hook_lists but not called by default:
"safeguard_tool_inputs"
"safeguard_tool_outputs"
"safeguard_llm_inputs"
"safeguard_llm_outputs"
"safeguard_human_inputs"
```

**Lesson**: Pre-defining hook points (even if not yet invoked) signals to developers where the system intends to support hooks in the future. It can also support some developer workflows immediately by allowing manual invocation in custom code.

### 5. Template-Based System Message Updates

`UpdateSystemMessage` with string templates and `context_variables` provides a clean pattern for dynamic system prompts without code:

```python
UpdateSystemMessage(
    content_updater="Today is {current_date}. User: {user_name}",
    context_variables={"current_date": "...", "user_name": "..."}
)
```

**Lesson**: Consider supporting template-based state updates in addition to code-based hooks. Templates are safer for configuration-driven customization.
