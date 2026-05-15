# Tower Middleware for Rust — Condensed

**The idiomatic Rust middleware pattern. Directly applicable to di-core.**

```rust
trait Service<Request> {
    type Response;
    type Future: Future<Output = Result<Self::Response, Self::Error>>;
    fn poll_ready(&mut self, cx: &mut Context) -> Poll<Result<(), Self::Error>>;
    fn call(&mut self, req: Request) -> Self::Future;
}

trait Layer<S> {
    type Service;
    fn layer(&self, inner: S) -> Self::Service;
}
```

`ServiceBuilder::new().layer(A).layer(B).service(C)` — composes zero-cost at compile time via tuples.

| Advantage | Benefit |
|-----------|---------|
| Zero-cost abstraction | No runtime overhead for unused hooks |
| `poll_ready` backpressure | Services signal when busy (rate limited, queue full) |
| 16+ built-in middleware | timeout, retry, rate-limit, load-shed, hedge, filter, buffer |
| Tower-test crate | Mock services for testing hooks in isolation |
| Widely adopted | hyper, tonic, warp, tower-lsp all use it |

3 lessons: use Service/Layer for Rust hook composition (idiomatic, zero-cost), compile-time > runtime for perf-critical hooks, `poll_ready` enables backpressure-aware hook chains.
