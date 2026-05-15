# Go Hook System — api-gateway

## Design

Extends existing `OpenAICompatConfig` function hook pattern. New `HookRegistry`
with typed hook points matching the existing idiom (function fields on a config struct).

No new interfaces — idiomatic Go: function fields with zero values meaning "no hook".

## Hook Config

```go
// HookConfig extends the existing pattern with standardized fields.
type HookConfig struct {
    // Existing (unchanged):
    ModifyRequest   func(req *Request, result map[string]interface{})
    ModifyHeaders   func(httpReq *http.Request, req *Request)
    ModifyMessages  func(messages []map[string]interface{}, req *Request) []map[string]interface{}

    // New:
    OnBeforeSend      func(ctx context.Context, req *Request) error
    OnAfterSend       func(ctx context.Context, req *Request, resp *SendResult, err error)
    OnStreamChunk     func(ctx context.Context, chunk *StreamChunk) (*StreamChunk, error)
    FilterResponse    func(ctx context.Context, resp *http.Response) error
    OnRateLimit       func(ctx context.Context, retryAfter time.Duration) error
}

// Pipeline middleware — wraps the stream callback with pre/post hooks.
func WrapStreamWithHooks(
    config *HookConfig,
    inner func(StreamChunk) error,
) func(StreamChunk) error {
    return func(chunk StreamChunk) error {
        if config.OnStreamChunk != nil {
            modified, err := config.OnStreamChunk(context.TODO(), &chunk)
            if err != nil {
                return err
            }
            if modified != nil {
                return inner(*modified)
            }
        }
        return inner(chunk)
    }
}
```

## Integration

Each provider's `Send`/`Stream` method already calls `ModifyRequest` etc. New code:

```go
// In openai_compat.go Send():
if h.config.OnBeforeSend != nil {
    if err := h.config.OnBeforeSend(ctx, req); err != nil {
        return nil, fmt.Errorf("hook blocked: %w", err)
    }
}
result, err := h.sendHTTP(ctx, req)
if h.config.OnAfterSend != nil {
    h.config.OnAfterSend(ctx, req, result, err)
}
```

## Global Registry (Provider-Level)

```go
// GlobalProviderHooks store hooks that run for ALL providers.
var GlobalProviderHooks HookConfig

// Merge combines global + provider-specific hooks.
func (c *HookConfig) Merge(global HookConfig) HookConfig {
    // Provider-specific takes precedence, global is fallback.
    merged := global
    if c.ModifyRequest != nil { merged.ModifyRequest = c.ModifyRequest }
    if c.ModifyHeaders != nil { merged.ModifyHeaders = c.ModifyHeaders }
    if c.ModifyMessages != nil { merged.ModifyMessages = c.ModifyMessages }
    if c.OnBeforeSend != nil { merged.OnBeforeSend = c.OnBeforeSend }
    if c.OnAfterSend != nil { merged.OnAfterSend = c.OnAfterSend }
    if c.OnStreamChunk != nil { merged.OnStreamChunk = c.OnStreamChunk }
    if c.FilterResponse != nil { merged.FilterResponse = c.FilterResponse }
    if c.OnRateLimit != nil { merged.OnRateLimit = c.OnRateLimit }
    return merged
}
```

## Existing Pattern (preserved)

30+ providers already use `ModifyRequest`/`ModifyHeaders`/`ModifyMessages` in their
`OpenAICompatConfig` (see api-gateway/providers/*.go). These remain the primary
extension mechanism. New hook points are optional additions.
