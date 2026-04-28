## Important

**di‑rac is strictly a CLI agent.**  
The VS Code extension and headless browser tool from upstream Dirac are removed or unsupported. The terminal‑based workflow remains.
If you want the GUI or browser features, use the original [dirac‑run/dirac](https://github.com/dirac-run/dirac).

# di‑rac: Progress Report

This is a fork of [dirac‑run/dirac](https://github.com/dirac-run/dirac), a coding agent focused on efficiency and context curation.  
**di‑rac** pushes further toward determinism, token economy, and CLI‑native execution.

## What’s already in place

- **Single‑token base‑32 content‑hash anchors**  
  3‑char `[0-9a-v]` anchors — deterministic, collision‑resistant, and always a single token for every major LLM tokeniser.

- **Progressive file exploration (cost ladder)**  
  `skeleton` / `outline` / `expand` modes let the model pay for structure first, bodies later.

- **Multi‑range & diff‑aware reads**  
  One call can fetch several non‑contiguous slices; unchanged files return a compact “unchanged” signal.

- **Volume‑based auto‑expand**  
  Repeated full‑file reads of the same file automatically return a larger preview.

- **Optional --bash-tool**  
  Restricted bash with binary allowlist, output truncation, timeout, merge‑conflict guard, and filename‑length guard.
  Auto‑approval behaviour is configurable via --bash-auto-approve / --no-bash-auto-approve.

- **Structured error handling**  
  All tool responses use a discriminated `ToolResponse<T>` with machine‑parseable error codes.

- **Optional --rewrite-paths**  
  Absolute paths are silently normalised.

- **DeepSeek‑V4 compatibility**  
  Full support for DeepSeek‑V4’s API and tokeniser behaviour.

- **Improved C/C++ build‑system awareness**  
  Better handling of mixed Meson/CMake/Make projects and dependency graphs.

- **Memory‑leak & stability fixes**  
  The original Dirac had several long‑running memory leaks and concurrency bugs; these are resolved.

## License & credits

Apache 2.0 — same as upstream.  
Huge thanks to the Dirac authors for a solid foundation.
