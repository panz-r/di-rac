# Testing Patterns — From Your Existing api-gateway Tests

**The `MockHandler` pattern in `provider_test.go` is the testing pattern your hooks should follow.**

## Existing MockHandler Pattern

```go
// From your api-gateway/providers/provider_test.go
type MockHandler struct {
    SendFunc     func(ctx context.Context, req *Request) (*SendResult, error)
    StreamFunc   func(ctx context.Context, req *Request, callback func(StreamChunk) error) error
    SendCalled   int
    StreamCalled int
}

func (h *MockHandler) Send(ctx context.Context, req *Request) (*SendResult, error) {
    h.SendCalled++
    if h.SendFunc != nil { return h.SendFunc(ctx, req) }
    return &SendResult{}, nil
}
```

Key features: injectable function hooks (`SendFunc`/`StreamFunc`), call counters, sensible defaults (returns empty result when no func set).

## Pattern for Hook Testing

The same pattern applied to hooks:

```go
// For hook testing
type MockHook struct {
    BeforeFunc func(ctx *HookContext) error
    AfterFunc  func(ctx *HookContext) error
    Called     int
}

func (h *MockHook) BeforeTool(ctx *HookContext) error {
    h.Called++
    if h.BeforeFunc != nil { return h.BeforeFunc(ctx) }
    return nil // Default: allow
}
```

## 5 Testing Lessons from the Codebase

1. **Injectable function hooks** — `SendFunc`/`StreamFunc` pattern allows test-specific behavior without inheritance
2. **Call counters** — `SendCalled`/`StreamCalled` verify invocation without assertions inside the mock
3. **Sensible defaults** — Returning empty/default results when no func is set prevents cascading test failures
4. **JSON round-trip tests** — The test file extensively tests JSON unmarshaling of all message types. Your hooks need the same for their protocol types.
5. **Error cases tested explicitly** — `TestRegistryGetHandlerNotFound`, `TestHandlerErrors` test the failure paths. Your hooks need comprehensive error case tests.
