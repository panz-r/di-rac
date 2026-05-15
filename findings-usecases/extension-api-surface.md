# Extension API Surface — Pi

**Condensed reference from actual source types — for complete details see `extension-runner-internals.md` and individual system files.**

## Extension Module

```typescript
export default function (pi: ExtensionAPI) {
    pi.on("session_start", async (event, ctx) => { ... });
    pi.registerTool({ name: "my_tool", ... });
    pi.registerCommand("my_command", { handler: async (args, ctx) => { ... } });
}
```

TypeScript loaded via `jiti` (no compile step). Extensions auto-discovered from `~/.pi/agent/extensions/*.ts` and `.pi/extensions/*.ts`.

## Registration Methods

| Method | Purpose |
|--------|---------|
| `pi.on(event, handler)` | Subscribe to lifecycle events (25+ types) |
| `pi.registerTool(def)` | Register LLM-callable tool (with TypeBox schema) |
| `pi.registerCommand(name, opts)` | Register slash command |
| `pi.registerShortcut(key, opts)` | Register keyboard shortcut |
| `pi.registerFlag(name, opts)` | Register CLI flag |
| `pi.registerMessageRenderer(type, renderer)` | Custom message rendering in TUI |
| `pi.registerProvider(name, config)` | Register/override LLM provider |
| `pi.unregisterProvider(name)` | Remove a registered provider |
| `pi.sendMessage(msg, opts?)` | Send custom message (inject into context) |
| `pi.sendUserMessage(content, opts?)` | Send user message (triggers turn) |
| `pi.appendEntry(type, data?)` | Persist extension state (LLM-invisible) |
| `pi.setSessionName(name)` | Set session display name |
| `pi.setLabel(entryId, label)` | Bookmark an entry |
| `pi.getActiveTools()` / `setActiveTools(names)` | Manage active tools |
| `pi.setModel(model)` / `getThinkingLevel()` / `setThinkingLevel(level)` | Model control |
| `pi.events` | Inter-extension event bus (pub/sub) |

## Event Catalog (25+ types via `pi.on`)

### Session Lifecycle

| Event | Return | Can Cancel? |
|-------|--------|-------------|
| `session_start` with `reason` (startup/reload/new/resume/fork) | void | No |
| `session_before_switch` with `reason` (new/resume) | `{ cancel? }` | Yes |
| `session_before_fork` with `entryId`, `position` | `{ cancel?, skipConversationRestore? }` | Yes |
| `session_before_compact` with `preparation` | `{ cancel?, compaction? }` | Yes |
| `session_compact` with `compactionEntry` | void | No |
| `session_shutdown` with `reason` (quit/reload/new/resume/fork) | void | No |
| `session_before_tree`, `session_tree` | Various | Yes/No |

### Agent Turn

| Event | Return | Purpose |
|-------|--------|---------|
| `context` with `messages` | `{ messages? }` | Modify messages before LLM call |
| `before_provider_request` with `payload` | Modified payload | Modify raw HTTP request |
| `after_provider_response` with `status`, `headers` | void | Observe provider response |
| `before_agent_start` with `prompt`, `systemPrompt` | `{ message?, systemPrompt? }` | Inject messages, modify system prompt |
| `agent_start` / `agent_end` with `messages` | void | Observe agent lifecycle |

### Turn & Tool Lifecycle

| Event | Event Object |
|-------|-------------|
| `turn_start` / `turn_end` | `{ turnIndex, message?, toolResults? }` |
| `message_start` / `message_update` / `message_end` | `{ message }` |
| `tool_execution_start` / `tool_execution_update` / `tool_execution_end` | `{ toolCallId, toolName, args, result? }` |
| `tool_call` (typed per tool: Bash/Read/Edit/Write/Grep/Find/Ls/Custom) | `{ block?, reason? }` — event.input is **mutable** |
| `tool_result` (typed per tool) | `{ content?, details?, isError? }` — field-by-field merge |

### Model & User Events

| Event | Event Object |
|-------|-------------|
| `model_select` with `model`, `previousModel`, `source` | void |
| `user_bash` with `command`, `excludeFromContext` | `{ operations?, result? }` |
| `input` with `text`, `images`, `source` | `{ action: "continue" | "transform" | "handled" }` |
| `resources_discover` with `cwd`, `reason` | `{ skillPaths?, promptPaths?, themePaths? }` |

## Tool Definition (Actual Source Types)

```typescript
interface ToolDefinition {
    name: string;
    label: string;
    description: string;
    promptSnippet?: string;          // One-line for "Available tools" section
    promptGuidelines?: string[];     // Bullets for "Guidelines" section
    parameters: TSchema;            // TypeBox
    prepareArguments?: (args) => Static<TParams>;  // Pre-validation shim
    executionMode?: "sequential" | "parallel";     // Per-tool override
    renderCall? / renderResult?;    // Custom TUI rendering
    execute(id, params, signal, onUpdate, ctx): Promise<AgentToolResult>;
}
```

## Provider Registration

```typescript
pi.registerProvider("my-provider", {
    baseUrl?, apiKey?, api?,       // Connection
    streamSimple?                  // Custom stream handler
    models: [{ id, name, reasoning, cost, contextWindow, maxTokens }],
    oauth: { name, login, refreshToken, getApiKey, modifyModels? },
});
```

## Conflict Resolution

| Resource | Rule |
|----------|------|
| Shortcuts | 16 reserved keys cannot be overridden |
| Commands | Built-in always wins; duplicates get `:N` suffixes |
| Tools | First registration wins; extension tools override built-ins by same name |
| Flags | First registration wins |

## Context Types

```typescript
// Event handlers: ExtensionContext (read-only session)
interface ExtensionContext {
    ui: ExtensionUIContext;    // hasUI check required
    hasUI: boolean;
    cwd: string;
    sessionManager: ReadonlySessionManager;
    modelRegistry: ModelRegistry;
    model: Model | undefined;
    signal: AbortSignal | undefined;
    isIdle(), abort(), shutdown(), compact(), getSystemPrompt();
}

// Commands: ExtensionCommandContext (adds session control)
//  waitForIdle(), newSession(), fork(), navigateTree(), switchSession(), reload()

// After session transitions: ReplacedSessionContext
//  sendMessage(), sendUserMessage()
```

**Context staleness**: After `newSession()`/`fork()`/`switchSession()`/`reload()`, accessing any property on the old context **throws**.

## Extension UI Context (`ctx.ui`) — Mode Adaptability

| Method | Interactive | RPC | Print |
|--------|-------------|-----|-------|
| `select`, `confirm`, `input`, `editor` | Full TUI | JSON request/response | Returns undefined/false |
| `setStatus`, `notify`, `setTitle` | TUI update | Fire-and-forget JSON | No-op |
| `setFooter`, `setHeader`, `custom`, `setEditorComponent` | Full TUI | Not supported | No-op |
| `onTerminalInput` | Supported | Not supported | No-op |
| Widgets (component) | Full TUI | String arrays only | No-op |

## Key Design Lessons (from comparison with 6 other systems)

1. **Lazy context factory**: Context created at execution time (not registration). Prevents stale references.
2. **Self-describing tools**: `promptSnippet` + `promptGuidelines` on tool definitions — no extra hooks needed.
3. **Mutable event input**: `event.input` mutation for argument patching — simpler than return-value transforms.
4. **`defineTool()` type inference**: Preserves TypeScript types through tool definitions.
5. **Two-tier state**: `custom_message` entries (LLM-visible) vs `custom` entries (LLM-invisible).
6. **Delivery modes**: `sendMessage()` supports `steer`/`followUp`/`nextTurn` — different timing semantics.
