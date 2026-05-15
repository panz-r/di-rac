# Declarative Hook Manifest Schema

VS Code/Eclipse/IntelliJ-style manifest. Discover capabilities without loading code.
Used by all 5 processes to auto-discover available hooks on startup.

## Manifest File

Each hook/extension lives in `~/.dirac/hooks/<name>/manifest.json`:

```json
{
  "name": "my-security-hook",
  "version": "1.0.0",
  "description": "Blocks dangerous curl-pipe-bash patterns before execution",

  "author": "user@example.com",
  "license": "MIT",

  "activations": [
    {
      "point": "filter_tool_call",
      "layer": "di-core",
      "hook_type": "filter",
      "fail_mode": "closed",
      "priority": 20,
      "when": {
        "tool_name": "bash"
      }
    },
    {
      "point": "on_after_tool_exec",
      "layer": "di-core",
      "hook_type": "action",
      "fail_mode": "open"
    }
  ],

  "wasm": {
    "path": "hook.wasm",
    "entrypoint": "filter_tool_call",
    "memory_max_bytes": 16777216,
    "timeout_ms": 5000
  }
}
```

## Lazy Activation

Hooks are NOT loaded at startup. The manifest is scanned to discover what hook
points are available. The hook code is loaded ONLY when its event fires
(confirmed by VS Code as the correct pattern for large ecosystems):

```rust
pub struct LazyHookRegistry {
    /// Manifests discovered at startup (no code loaded)
    manifests: Vec<HookManifest>,

    /// Lazily-instantiated hooks (keyed by hook point name)
    loaded: HashMap<&'static str, Vec<LazyLoadedHook>>,
}

impl LazyHookRegistry {
    /// Called once at startup. Scans ~/.dirac/hooks/*/manifest.json.
    /// NO code is loaded — just JSON parsing.
    pub fn discover() -> Self;

    /// Called when a hook point fires. Loads the Wasm/Rust code if not yet loaded.
    /// Once loaded, stays cached for the session.
    pub async fn get_hooks_for_point(&mut self, point: &str) -> &[LazyLoadedHook];
}
```

## Activation Conditions (`when`)

Optional conditions in the manifest that MUST match for the hook to fire:

```json
{
  "when": {
    "tool_name": "bash",
    "agent_mode": "act",
    "turn_number_gt": 0,
    "file_pattern": "*.sh"
  }
}
```

Conditions are checked BEFORE loading the hook code. This avoids loading
Wasm modules for events the hook wouldn't act on.

## Discovery Paths

```
~/.dirac/hooks/*/manifest.json       — user-installed hooks
/etc/dirac/hooks/*/manifest.json     — system-wide hooks
./.dirac/hooks/*/manifest.json       — project-level hooks
```

Loaded in order: project → user → system. Later activations with the same
hook point and priority append after existing ones.

## Manifest Validation

On startup, `divrr` (TUI) validates all manifests:

1. JSON schema validation
2. Wasm binary existence check
3. Entrypoint function existence (via Wasm imports)
4. Memory budget check (per-hook max)
5. Activation point must be in known hook points list

Invalid manifests are skipped with a warning, not fatal.
