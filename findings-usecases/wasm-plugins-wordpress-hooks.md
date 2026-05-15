# Wasm & WordPress — Mature Extensibility Models

**Extism (WebAssembly for Rust sandboxed plugins) and WordPress (20+ year battle-tested hook system).**

## Extism: Wasm Plugin System for Rust

```
Rust Host  →  libextism runtime  →  .wasm Plugins (any language)
```

Plugins compile to `.wasm` binary. Sandboxed (no host memory/filesystem). Host functions are explicit opt-in.

```rust
// Plugin side (any language → .wasm)
#[plugin_fn]
pub fn before_tool(input: String) -> FnResult<String> {
    let config = get_config()?;
    // Return: allow, deny with reason, or transform
    Ok(json!({"action": "allow"}).to_string())
}

// Host side (Rust)
let plugin = Plugin::new(wasm_bytes, functions: true)?;
let result: String = plugin.call("before_tool", json_input)?;
```

**Key**: Language-agnostic, sandboxed, near-native speed. Perfect for third-party hooks in a Rust agent engine.

## WordPress: Two Hook Types, 20+ Years

| Type | Purpose | Returns |
|------|---------|---------|
| **Action** | "Do something" | void |
| **Filter** | "Modify something" | Modified value |

```php
// Action: register, do nothing with return value
add_action('before_tool', function($tool, $args) { log($tool); });

// Filter: register, return modified value
add_filter('tool_result', function($result, $tool) {
    return sanitize($result);
}, 10, 2);

// Declare hook point
do_action('before_tool', $tool, $args);
$result = apply_filters('tool_result', $result, $tool);
```

| Feature | WordPress | For Your DSL |
|---------|-----------|-------------|
| Two types | Actions + Filters | Observe + Transform |
| Priority | int, lower=earlier, default 10 | Default 10, early 5, late 15 |
| Namespacing | `plugin/hook_name` | `component/hook_name` |
| Remove hooks | `remove_action()` / `remove_filter()` | `unregister_hook()` |
| Global by default | Simplest model | Scoped when needed |
| Plugin directory | `wp-content/plugins/` | `~/.dirac/hooks/` |

## 8 Combined Lessons

1. **Two hook types are sufficient** — Actions (observe, void) and Filters (transform, return). Maps every use case across 20+ systems.
2. **Numeric priority with defaults** — Default 10. Lower = earlier. Battle-tested for 20+ years.
3. **Namespacing prevents conflicts** — `component/hook_name` with 40+ hooks, ambiguity is fatal.
4. **Hook removal is essential** — For testing, debugging, overriding third-party behavior.
5. **Wasm for sandboxed third-party hooks** — Any language, no recompilation, sandboxed execution.
6. **Explicit host functions for permissions** — HTTP, filesystem, logging are opt-in per plugin.
7. **Binary distribution** — `.wasm` is self-contained, versioned, distributable.
8. **Your gateway already has function hooks** — Build on existing `ModifyRequest`/`ModifyHeaders`/`ModifyMessages` pattern.
