# C Daemon Hook Protocol — command-daemon, central-daemon, treesitter-daemon

## Design

JSON-over-stdin/stdout protocol extension. Daemons read hook config on startup
and call external hook scripts/executables at hook points. Each hook is a
separate process (subprocess) with JSON-in/JSON-out.

Why subprocess: C daemons cannot load dynamic hooks at runtime. Subprocess
is the natural sandbox. The existing JSON protocol makes this trivial.

## Protocol

```json
// Hook execution request (daemon → hook process):
{
  "hook": "on_before_exec",
  "request_id": "req-001",
  "phase": "pre",
  "hook_type": "filter",
  "context": {
    "command": "rm -rf /tmp/test",
    "timeout_ms": 30000,
    "cwd": "/home/user/project",
    "uid": 1000
  }
}

// Hook execution response (hook process → daemon):
// For Action hooks:
{ "status": "ok" }

// For Filter hooks:
{
  "status": "ok",
  "action": "continue",
  "modified_context": {
    "command": "rm -rf /tmp/test",
    "timeout_ms": 30000
  }
}

// Block:
{
  "status": "ok",
  "action": "deny",
  "reason": "Dangerous recursive deletion without safety check"
}

// Error:
{
  "status": "error",
  "message": "Hook script crashed: segfault"
}
```

## Hook Configuration

Daemons read hook config from a JSON file on startup. The config path is
passed via environment variable `DIRAC_HOOK_CONFIG`.

```json
{
  "hooks": [
    {
      "point": "on_before_exec",
      "hook_type": "filter",
      "command": "/etc/dirac/hooks/check-dangerous.sh",
      "timeout_ms": 5000,
      "fail_mode": "closed",
      "priority": 10
    },
    {
      "point": "on_after_exec",
      "hook_type": "action",
      "command": "/etc/dirac/hooks/log-execution.sh",
      "timeout_ms": 2000,
      "fail_mode": "open",
      "priority": 5
    }
  ]
}
```

## Command-Daemon Integration

```c
// In command-daemon execute handler:
static bool run_hooks(const char *hook_point, json_value *context,
                      json_value **modified_context, char **deny_reason) {
    const HookConfig *hooks = get_hooks_for_point(hook_point);
    for (size_t i = 0; i < hook_count; i++) {
        HookResult result = run_hook_process(&hooks[i], context);
        switch (result.type) {
            case HOOK_OK:
                if (result.modified) {
                    *modified_context = result.modified;
                    context = result.modified;
                }
                break;
            case HOOK_DENY:
                *deny_reason = strdup(result.reason);
                return false;  // deny-wins
            case HOOK_ERROR:
                if (hooks[i].fail_mode == FAIL_CLOSED) {
                    *deny_reason = strdup("Hook error (fail closed)");
                    return false;
                }
                break;  // fail open: continue
        }
    }
    return true;  // allow
}
```

## Hook Points Per Daemon

### command-daemon

| Hook Point | Type | Context | Modified Fields |
|-----------|------|---------|-----------------|
| `on_before_exec` | Action | command, timeout_ms, cwd, uid | — |
| `filter_command` | Filter | command, timeout_ms, cwd, uid | all fields |
| `on_after_exec` | Action | command, exit_code, stdout_size, stderr_size, duration_ms | — |
| `filter_output` | Filter | stdout, stderr, exit_code | stdout, stderr |

### central-daemon

| Hook Point | Type | Context | Modified Fields |
|-----------|------|---------|-----------------|
| `on_config_change` | Action | config_key, old_value, new_value | — |
| `on_before_route` | Action | path, client_id | — |
| `filter_route` | Filter | path, client_id, trie_node | path |

### treesitter-daemon

| Hook Point | Type | Context | Modified Fields |
|-----------|------|---------|-----------------|
| `filter_symbol_query` | Filter | file_path, query_text, language | query_text |
| `on_before_parse` | Action | file_path, language, file_size | — |
| `on_after_parse` | Action | file_path, language, symbol_count, duration_ms | — |
