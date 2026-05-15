# Hook Points — All 5 Processes

Two hook types: **Actions** (observe, void return) + **Filters** (transform/block, return modified)

## di-core (Rust Engine)

| # | Hook Point | Scope | Type | Phase | Current Implementation |
|---|-----------|-------|------|-------|----------------------|
| 1 | `on_before_turn` | per-turn | Action | pre | `run_turn()` start |
| 2 | `filter_gateway_request` | per-turn | Filter | pre | `frame` building |
| 3 | `on_after_stream_chunk` | per-chunk | Action | mid | `chunk` match in streaming loop |
| 4 | `filter_tool_call` | per-call | Filter | pre | `run_preflight_firewall()` |
| 5 | `on_before_tool_exec` | per-call | Action | pre | circuit breaker check |
| 6 | `on_after_tool_exec` | per-call | Action | post | result processing |
| 7 | `filter_tool_result` | per-call | Filter | post | envelope wrapping |
| 8 | `filter_context_frame` | per-turn | Filter | pre | system prompt assembly |
| 9 | `on_before_compaction` | per-turn | Action | mid | lifecycle evaluation |
| 10 | `on_after_compaction` | per-turn | Action | mid | required file reload |
| 11 | `filter_approval_policy` | per-call | Filter | pre | approval manager |
| 12 | `on_error` | per-error | Action | post | error router |
| 13 | `on_recovery` | per-error | Action | post | circuit breaker |
| 14 | `on_session_start` | once | Action | init | SpawnAgent |
| 15 | `on_session_shutdown` | once | Action | exit | TaskFinished |
| 16 | `filter_tool_definitions` | per-turn | Filter | pre | tool definition list |

## api-gateway (Go)

| # | Hook Point | Scope | Type | Phase | Existing Mechanism |
|---|-----------|-------|------|-------|-------------------|
| 17 | `ModifyRequest` | per-request | Filter | pre | `OpenAICompatConfig` |
| 18 | `ModifyHeaders` | per-request | Filter | pre | `OpenAICompatConfig` |
| 19 | `ModifyMessages` | per-request | Filter | pre | `OpenAICompatConfig` |
| 20 | `on_before_send` | per-request | Action | pre | — |
| 21 | `on_after_send` | per-request | Action | post | — |
| 22 | `on_stream_chunk` | per-chunk | Action | mid | Minimax stream pipe |
| 23 | `filter_response` | per-request | Filter | post | — |
| 24 | `on_rate_limit` | per-request | Action | pre | — |

## command-daemon (C)

| # | Hook Point | Scope | Type | Phase | Current Implementation |
|---|-----------|-------|------|-------|----------------------|
| 25 | `on_before_exec` | per-command | Action | pre | — |
| 26 | `filter_command` | per-command | Filter | pre | blocked patterns |
| 27 | `on_after_exec` | per-command | Action | post | result formatting |
| 28 | `filter_output` | per-command | Filter | post | truncation |

## central-daemon (C)

| # | Hook Point | Scope | Type | Phase | Current Implementation |
|---|-----------|-------|------|-------|----------------------|
| 29 | `on_config_change` | on-change | Action | post | — |
| 30 | `on_before_route` | per-request | Action | pre | — |
| 31 | `filter_route` | per-request | Filter | pre | trie routing |

## treesitter-daemon (C)

| # | Hook Point | Scope | Type | Phase | Current Implementation |
|---|-----------|-------|------|-------|----------------------|
| 32 | `filter_symbol_query` | per-command | Filter | pre | .scm query files |
| 33 | `on_before_parse` | per-file | Action | pre | — |
| 34 | `on_after_parse` | per-file | Action | post | — |
