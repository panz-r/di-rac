# Vercel AI SDK & Semantic Kernel — Two More Hook Architectures

Two additional reference architectures representing TypeScript middleware wrapping and enterprise-grade C#/Python filter pipelines.

## Part 1: Vercel AI SDK Middleware (TypeScript)

The AI SDK provides a middleware system that intercepts model calls at runtime for cross-cutting concerns like guardrails, caching, RAG, and logging.

### Architecture

```
generateText / streamText
    │
    ▼
wrapLanguageModel(model, [middleware1, middleware2])
    │
    ▼  Middleware applied in order
middleware1.wrapGenerate() / .wrapStream()
    │
middleware2.wrapGenerate() / .wrapStream()
    │
provider.doGenerate() / .doStream()
```

Middleware wraps the model in layers: `firstMiddleware(secondMiddleware(baseModel))`.

### Core Wrapper

```typescript
import { wrapLanguageModel } from "ai";

const wrappedModel = wrapLanguageModel({
    model: baseModel,
    middleware: [myMiddleware, cacheMiddleware],
    modelId: "custom-id",    // optional
    providerId: "custom-provider", // optional
});
```

### Middleware Interface

```typescript
interface LanguageModelV4Middleware {
    wrapGenerate?: (options: {
        doGenerate: () => Promise<GenerateResult>;
        params: GenerateParams;
        model: LanguageModelV4;
    }) => Promise<GenerateResult>;

    wrapStream?: (options: {
        doStream: () => Promise<StreamResult>;
        params: StreamParams;
        model: LanguageModelV4;
    }) => Promise<StreamResult>;
}
```

Each middleware implements `wrapGenerate` (non-streaming) and/or `wrapStream` (streaming). Both receive the actual `doGenerate`/`doStream` function as a callable — call it to proceed with the next middleware or provider.

### Caching Example

```typescript
const cacheMiddleware: LanguageModelV4Middleware = {
    wrapGenerate: async ({ doGenerate, params }) => {
        const key = JSON.stringify(params);
        const cached = await cache.get(key);
        if (cached) return cached;
        const result = await doGenerate();
        await cache.set(key, result);
        return result;
    },
    wrapStream: async ({ doStream, params }) => {
        const key = JSON.stringify(params);
        const cached = await cache.get(key);
        if (cached) return simulateReadableStream(cached);
        const result = await doStream();
        await cache.set(key, result);
        return result;
    },
};
```

### Built-in Middleware

| Middleware | Purpose |
|-----------|---------|
| `extractReasoningMiddleware` | Extracts `<think>` reasoning blocks into separate property |
| `simulateStreamingMiddleware` | Simulates streaming for non-streaming models |
| `extractJsonMiddleware` | Strips markdown code fences from JSON output |
| `addToolInputExamplesMiddleware` | Serializes tool examples into descriptions for non-native providers |
| `defaultSettingsMiddleware` | Applies default temperature/maxTokens if not explicitly set |

### Provider Integration

```typescript
const customProvider = createProvider({
    baseUrl: "...",
    models: {
        "my-model": wrapLanguageModel({
            model: baseModel,
            middleware: [guardrailsMiddleware],
        }),
    },
});
```

### Key Design Characteristics

| Aspect | Behavior |
|--------|----------|
| **Wrap count** | 2 hooks (`wrapGenerate`, `wrapStream`) |
| **Execution model** | Middleware wraps provider calls — not agent-loop events |
| **Modification** | Call `doGenerate()`/`doStream()` to proceed; modify result before returning |
| **Caching** | Primary use case — bypass provider entirely on cache hit |
| **Composition** | Multiple middleware nest like onion layers |
| **Error handling** | try/catch within middleware; middleware decides retry/fallback |

### Comparison with LangChain Middleware

| Aspect | Vercel AI SDK | LangChain |
|--------|--------------|-----------|
| **Scope** | Model call only (no agent loop) | Full agent loop + model calls |
| **Hook points** | `wrapGenerate`, `wrapStream` | 6 hooks (node + wrap styles) |
| **State updates** | Not supported | State reducer via `Command` |
| **Graph awareness** | No | Yes (LangGraph nodes) |
| **Tool wrapping** | Via provider | `wrap_tool_call` hook |
| **Provider management** | `wrapLanguageModel` + `customProvider` | `create_agent(middleware=...)` |

---

## Part 2: Semantic Kernel Filters (C#/Python)

Microsoft's Semantic Kernel provides a filter system for enterprise-grade AI applications, with three filter types operating on function invocation and prompt rendering.

### Three Filter Types

| Filter | Scope | When It Fires | Can Block? |
|--------|-------|---------------|------------|
| **Function Invocation** | Every `KernelFunction` call | Before/after any function | Yes (skip `next()`) |
| **Prompt Render** | Prompt rendering only | Before prompt sent to AI | Yes (override result) |
| **Auto Function Invocation** | Automatic function calling loop | During auto function calling | Yes (`context.Terminate = true`) |

### Filter Pattern (Middleware Pipeline)

```
Filter 1: before
    │
    ▼
Filter 2: before
    │
    ▼
    Function execution
    │
    ▼
Filter 2: after
    │
    ▼
Filter 1: after
```

Filters use the `next` delegate pattern — call `await next(context)` to proceed:

```csharp
public async Task OnFunctionInvocationAsync(
    FunctionInvocationContext context,
    Func<FunctionInvocationContext, Task> next)
{
    // Before function
    await next(context);
    // After function
}
```

### Function Invocation Filter

```csharp
// C#
public class LoggingFilter : IFunctionInvocationFilter
{
    public async Task OnFunctionInvocationAsync(
        FunctionInvocationContext context,
        Func<FunctionInvocationContext, Task> next)
    {
        Console.WriteLine($"Calling {context.Function.PluginName}.{context.Function.Name}");
        await next(context);
        Console.WriteLine($"Completed");
    }
}

// Register
kernel.FunctionInvocationFilters.Add(new LoggingFilter());
```

### Prompt Render Filter

```csharp
public class SafePromptFilter : IPromptRenderFilter
{
    public async Task OnPromptRenderAsync(
        PromptRenderContext context,
        Func<PromptRenderContext, Task> next)
    {
        await next(context);
        context.RenderedPrompt = "Safe prompt: " + context.RenderedPrompt;
    }
}
```

### Auto Function Invocation Filter

```csharp
public class EarlyTerminationFilter : IAutoFunctionInvocationFilter
{
    public async Task OnAutoFunctionInvocationAsync(
        AutoFunctionInvocationContext context,
        Func<AutoFunctionInvocationContext, Task> next)
    {
        await next(context);
        if (context.Result.GetValue<string>() == "desired result")
            context.Terminate = true;
    }
}
```

### Filter Registration

```python
# Python — decorator style
@kernel.filter(FilterTypes.FUNCTION_INVOCATION)
async def logger_filter(context, next):
    print(f"Calling {context.function.plugin_name}.{context.function.name}")
    await next(context)
    print("Completed")

# Or programmatic
kernel.add_filter(FilterTypes.FUNCTION_INVOCATION, logger_filter)
```

### Python Execution Order

```python
@kernel.filter(FilterTypes.FUNCTION_INVOCATION)
async def filter1(context, next):
    print('before filter 1')
    await next(context)
    print('after filter 1')

@kernel.filter(FilterTypes.FUNCTION_INVOCATION)
async def filter2(context, next):
    print('before filter 2')
    await next(context)
    print('after filter 2')

# Output:
# before filter 1
# before filter 2
# function
# after filter 2
# after filter 1
```

### Streaming Support

Filters handle both streaming and non-streaming modes:

```python
@kernel.filter(FilterTypes.FUNCTION_INVOCATION)
async def streaming_filter(context, next):
    await next(context)
    if not context.is_streaming:
        return

    async def override_stream(stream):
        try:
            async for partial in stream:
                yield partial
        except Exception as e:
            yield [StreamingChatMessageContent(role="assistant", content=f"Error: {e}")]

    context.result = FunctionResult(
        function=context.result.function,
        value=override_stream(context.result.value)
    )
```

### Key Design Characteristics

| Aspect | Behavior |
|--------|----------|
| **Filter count** | 3 types (function, prompt, auto-invoke) |
| **Execution model** | `next()` delegate — before/after with skip support |
| **Blocking** | Skip `next()` to block execution entirely |
| **Termination** | `context.Terminate = true` stops auto function calling loop |
| **Streaming** | Separate handling for streaming vs non-streaming results |
| **Registration** | Kernel property lists + dependency injection |
| **Order guarantee** | Python: registration order; C# DI: not guaranteed |
| **Override** | Modify context properties before/after next() |

## Key Takeaways for DSL Design

### 1. Wrap the Provider, Not Just the Loop

Vercel AI SDK's middleware wraps the **model/provider call**, not the agent loop. This enables caching at the provider boundary — before any tokens are consumed.

**Lesson**: Consider whether your DSL should have middleware at the provider call boundary in addition to the agent loop boundary. Provider-level middleware enables caching, retry, and fallback without touching loop logic.

### 2. The `next()` Delegate Pattern

Semantic Kernel's `next(context)` pattern is simple and powerful:
- Before `next()` = pre-execution hook
- After `next()` = post-execution hook
- Skip `next()` entirely = block execution
- Modify context before/after = state transformation

**Lesson**: The `next()` delegate is one of the cleanest patterns for filter/hook design. It naturally handles before/after semantics with zero configuration.

### 3. Streaming Awareness

Semantic Kernel explicitly handles streaming vs non-streaming differently:

```python
if context.is_streaming:
    # Override with async generator
else:
    # Override with direct value
```

**Lesson**: If your loop supports streaming, hooks must be aware of whether they're operating in streaming or non-streaming mode. The same hook may need different logic for each.

### 4. Multiple Registration Systems

Semantic Kernel supports both direct property addition and dependency injection:

```csharp
kernel.FunctionInvocationFilters.Add(new MyFilter());       // Direct
builder.Services.AddSingleton<IFunctionInvocationFilter, MyFilter>();  // DI
```

**Lesson**: Support multiple registration methods — programmatic for simple cases, DI/injection for complex applications.

### 5. Automatic Function Calling Loop Awareness

Semantic Kernel's `AutoFunctionInvocationFilter` is specifically for the tool-calling loop, not individual function calls:

```csharp
context.Terminate = true;  // Stop the auto-calling loop early
```

**Lesson**: If your loop has an automatic tool-calling sub-loop, consider a dedicated hook for loop-level decisions (terminate early, skip remaining tools) vs per-tool hooks.

### 6. Provider-Level Composition

Vercel's middleware composes at the provider level:

```typescript
const model = wrapLanguageModel(
    myProvider.languageModel("gpt-4"),
    [middleware1, middleware2]
);
```

This means the same middleware stack can be reused across different providers, models, and applications.

**Lesson**: Design middleware to be provider-agnostic when possible. A caching middleware should work with any language model, not just one provider.
