# Treesitter-Daemon — Condensed

**Your complete 5-process architecture.**

## 16+ Commands, 2 AST Strategies

| Strategy | Languages | Method |
|----------|-----------|--------|
| Manual tree walk | C, C++, Java, C#, Ruby, PHP | `ts_node_type()`, field name matching |
| TSQuery-based | Python, Rust, Go, TS/JS, Bash | `ts_query_new()`, capture names |

## Hook Opportunities

1. **User-defined `.scm` query files** loaded at startup from hooks directory
2. **Transform callbacks** with JSON templates for matched patterns
3. **Lua/Wasm embedding** for logic hooks (Extism C SDK)
4. **New `run-hook` command** with hook name + source + language + args

## Your Complete Architecture

| Process | Lang | Protocol | Hook Potential |
|---------|------|----------|---------------|
| **di-core** | Rust | Internal API | Tower Service/Layer traits |
| **api-gateway** | Go | Unix socket NDJSON | Extend existing `ModifyRequest/Headers/Messages` |
| **command-daemon** | C | stdin/stdout NDJSON | Pre/post execute hooks |
| **central-daemon** | C | Unix socket NDJSON | Config change hooks (broadcast already works) |
| **treesitter-daemon** | C | stdin/stdout NDJSON | User-defined query files + Wasm |
