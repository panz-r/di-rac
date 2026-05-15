# Go Middleware & IntelliJ — Condensed

**Two patterns directly relevant to your system.**

## Go HTTP Middleware (Your api-gateway already uses this)

Standard pattern: `func(http.Handler) http.Handler` — compose via nesting: `m1(m2(m3(handler)))`.

6 categories: logging (observe), auth/rate-limit (block), header injection (modify), redirect (route), panic recovery, response caching.

3 lessons for your DSL:
1. **`func(next) next` wrap pattern** — hook receives context AND `next` callable. Enables setup/teardown, retry, caching.
2. **Context propagation** — `context.Context` carries cancellation + cross-hook data. Your hooks need equivalent.
3. **Short-circuit by skipping `next`** — Don't call `next.ServeHTTP()`, write own response. Maps to block/deny.

## IntelliJ Plugin Architecture (1115 Extension Points, 212 Listeners)

| Mechanism | Purpose | Registration | Lifecycle |
|-----------|---------|-------------|-----------|
| **Extension Points** | Add behavior | Declarative XML | Loaded on start |
| **Listeners** | Subscribe to events | Declarative XML | **Stateless, no lifecycle** |

Key constraint: **Listeners must be stateless** — prevents resource leaks. Design lesson: if hooks don't need state, make them stateless functions.

4 lessons:
1. **Stateless listeners prevent bugs** — If your hook doesn't need state, make it a pure function.
2. **1115 extension points is too many** — Start small, add deliberately.
3. **Extension points > listeners for behavior** — EPs define WHAT, listeners observe WHEN.
4. **XML discovery without code loading** — IntelliJ scans XML only, loads plugin class lazily.
