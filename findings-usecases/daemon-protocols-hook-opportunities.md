# Daemon Hook Opportunities — Condensed

**Command-daemon and central-daemon analysis.**

## Command-Daemon

JSON-over-piped-stdin/stdout. 4 request types: execute, walk, recent_files, session_info. Hardcoded `if/else if` dispatch in `protocol.c`.

**Natural hook points**: `handle_execute()` before/after `executor_fork()` — inspect/modify command before fork, observe result after. Could add shell-script hooks (Letta pattern) or Wasm hooks (Extism C SDK).

## Central-Daemon

JSON-over-Unix-socket. 5 methods: acquire, release, set_config, get_config, status. One existing callback: `send_granted_cb` (function pointer + void* ctx, used for waiter notification).

Already has `broadcast_config_update()` — pushes config changes to all clients. This is already an event notification system.

## Both Daemons

Share the same `json.h` (zero-copy inline parser). No hooks/plugins. Natural extension: generalize `function pointer + void* ctx` pattern from central-daemon to both daemons.
