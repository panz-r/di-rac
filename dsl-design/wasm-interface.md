# Wasm/Extism Interface — Third-Party Sandboxed Hooks

## Design

Extism PDK for language-agnostic Wasm hooks. Every hook is a Wasm module with
a single exported function per hook point.

Why Extism: works in all 5 processes (Rust via `extism-rust`, Go via `extism-go`,
C via `extism-c`). No recompilation of the host process needed. Sandboxed by
default (no file system, no network without explicit grants).

## Host Interface (Rust — di-core)

```rust
use extism::{Plugin, PluginBuilder, Manifest, Wasm};

pub struct WasmHook {
    plugin: Plugin,
    fail_mode: FailMode,
    timeout_ms: u64,
}

impl WasmHook {
    pub fn load(manifest: &HookManifest) -> Result<Self> {
        let wasm_path = &manifest.wasm.path;
        let wasm = Wasm::file(wasm_path);
        let manifest = Manifest::new([wasm]);
        let plugin = PluginBuilder::new(manifest)
            .with_timeout(Duration::from_millis(manifest.wasm.timeout_ms))
            .with_memory_max(manifest.wasm.memory_max_bytes)
            .build()?;
        Ok(Self {
            plugin,
            fail_mode: manifest.activations[0].fail_mode,
            timeout_ms: manifest.wasm.timeout_ms,
        })
    }

    pub fn call_action(&mut self, hook_point: &str, input: &str) -> Result<()> {
        // Returns "" on success, error message string on failure
        let result = self.plugin.call(hook_point, input)?;
        let result_str = result.unwrap_or_default();
        if result_str.is_empty() {
            Ok(())
        } else {
            Err(anyhow!("Wasm hook error: {}", result_str))
        }
    }

    pub fn call_filter(&mut self, hook_point: &str, input: &str) -> Result<FilterResult<String>> {
        // Returns "continue:<modified_json>" or "deny:<reason>" or "error:<msg>"
        let result = self.plugin.call(hook_point, input)?;
        let output = result.unwrap_or_default();
        if let Some(rest) = output.strip_prefix("continue:") {
            Ok(FilterResult::Continue(rest.to_string()))
        } else if let Some(reason) = output.strip_prefix("deny:") {
            Ok(FilterResult::Deny { reason: reason.to_string() })
        } else {
            Err(anyhow!("Wasm hook returned unexpected format: {}", output))
        }
    }
}
```

## Wasm SDK (for extension authors)

Extensions are written in any language that compiles to Wasm (Rust, Go, C,
TypeScript via AssemblyScript, Python via Extism PDK).

### Example: Rust extension

```rust
// Cargo.toml: extism-pdk = "1.0"

use extism_pdk::*;

#[plugin_fn]
pub fn filter_tool_call(input: String) -> FnResult<String> {
    let ctx: serde_json::Value = serde_json::from_str(&input)?;
    let tool_name = ctx["call"]["name"].as_str().unwrap_or("");

    if tool_name == "bash" {
        let cmd = ctx["call"]["args"]["command"].as_str().unwrap_or("");
        if cmd.contains("curl") && cmd.contains("| bash") {
            // Deny dangerous patterns
            return Ok(format!("deny:curl-pipe-bash detected and blocked"));
        }
    }

    Ok(format!("continue:{}", input))
}
```

### Example: Go extension (using Extism PDK)

```go
package main

import (
    "github.com/extism/go-pdk"
)

//export on_before_exec
func onBeforeExec() int32 {
    input := pdk.InputString()
    // ... check command, return "continue:<modified>" or "deny:<reason>"
    return 0
}

func main() {}
```

### Example: C extension

```c
#include "extism-pdk.h"

int32_t EXPORT("filter_command") filter_command() {
    ExtismString input = extism_input();
    // Parse JSON, check, return result
    extism_output_str("continue:...");
    return 0;
}
```

## Limits & Isolation

| Property | Value |
|----------|-------|
| Default memory | 16MB per hook |
| Max execution time | 5000ms per call |
| File system access | None (require explicit grant) |
| Network access | None |
| Host process crash | Impossible (Wasm is sandboxed) |
| FFI overhead | ~0.1ms per call (measured) |

## Cross-Process Consistency

The same Wasm module works in all 5 processes because all 5 use Extism:

| Process | Extism SDK | Language |
|---------|-----------|----------|
| di-core | extism-rust | Rust |
| api-gateway | extism-go | Go |
| command-daemon | extism-c | C |
| central-daemon | extism-c | C |
| treesitter-daemon | extism-c | C |

The host serializes the hook context to JSON, passes it to the Wasm module,
and receives the result. All 5 processes use the same protocol:

```
input:  JSON string of hook context (varies per hook point)
output: "continue:<modified_json>" | "deny:<reason>" | "error:<msg>"  (filters)
        "" (ok) | "<error_msg>" (action)
```
