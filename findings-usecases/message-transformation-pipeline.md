# Message Transformation Pipeline — Condensed

**How Pi transforms messages across layers — and the edge cases hooks must account for.**

## Stages

```
AgentMessage (internal — includes custom roles)
    ↓ transformContext (optional hook — prune, inject RAG)
    ↓ convertToLlm (required — filter to user/assistant/toolResult)
LLM Message (canonical format)
    ↓ transformMessages (cross-provider normalization)
Provider SDK Format (Anthropic/OpenAI/Gemini specific)
```

## 5 Cross-Provider Edge Cases

### 1. Tool Call ID Normalization

Different providers have incompatible requirements:
- **OpenAI**: Can generate IDs with `|` characters or long strings
- **Anthropic**: Requires `^[a-zA-Z0-9_-]+$`, max 64 chars
- The `normalizeToolCallId` callback maps IDs AND updates both the `AssistantMessage` (origin) and `ToolResultMessage` (response)

**Hook implication**: If hooks store tool call IDs, use post-normalization IDs for consistency.

### 2. Thinking Content Transformation

When switching models:
- **Redacted thinking** (Anthropic encrypted blocks): **Dropped** for cross-model handoffs
- **Thinking to text**: Valid `ThinkingContent` → `TextContent` when models differ

**Hook implication**: Thinking content may be dropped or converted between turns if the model changes.

### 3. Orphaned Tool Calls → Synthetic Results

All tool calls must have matching tool results. `transformMessages` does a second pass to insert synthetic `"No result provided"` messages for orphaned calls:

```typescript
for (const msg of messages) {
    if (msg.role === "assistant" && msg.toolCalls) {
        for (const tc of msg.toolCalls) {
            if (!hasCorrespondingResult(tc.id)) {
                insertSyntheticResult(tc.id, "No result provided", true);
            }
        }
    }
}
```

**Hook implication**: Blocked tools must produce synthetic results to avoid orphaned calls.

### 4. Image Downgrade

Non-vision models: images replaced with text placeholders.

**Hook implication**: Check model capabilities before relying on image data.

### 5. Gemini 3 Thought Signatures

Gemini 3 requires `thoughtSignature` on all function calls with thinking mode. Cross-provider replays get `SKIP_THOUGHT_SIGNATURE` sentinel.

**Hook implication**: Cross-provider handoffs require per-provider compatibility logic.

## StreamFn Contract

```typescript
type StreamFn = (...args) => AssistantMessageEventStream;
```

Contract:
- **Must not throw** for request/model/runtime failures
- **No rejected promises** — failures encoded as stream protocol events
- **Final message** must include `stopReason: "error"` or `"aborted"` if failed

## AgentMessage vs LLM Message

| Feature | AgentMessage | LLM Message |
|---------|-------------|-------------|
| Roles | `user`, `assistant`, `toolResult`, **custom** | `user`, `assistant`, `toolResult` |
| Metadata | `timestamp`, `id`, custom fields | Content + role only |
| Usage | Internal state, hooks, UI rendering | Wire format for LLM API |

Custom roles from extensions (`custom_message`) are kept in AgentMessage but **filtered during `convertToLlm`**. Hooks that need data visible to the LLM must use standard roles.

## Key Design Lessons

1. **Message inspection vs modification** — hooks receive pre-transformation messages. Changes after `convertToLlm` would need to operate on LLM message format.
2. **Cross-provider safety** — hooks storing tool call IDs or thinking content must account for normalization/transformation between turns.
3. **Orphaned call prevention** — hooks that block tool calls must provide tool results to avoid synthetic insertion.
4. **Model capability awareness** — hooks should check model capabilities (vision, thinking) before relying on data that may be stripped.
5. **AgentMessage space for custom data** — `custom_message` entries are visible in AgentMessage but filtered during LLM conversion. Use standard roles for LLM-visible data.
