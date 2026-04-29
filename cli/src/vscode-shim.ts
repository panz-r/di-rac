/**
 * VS Code shim for CLI
 * Re-exports from cli-shim for compatibility with shared code
 */

// Re-export EventEmitter from Node.js
import { EventEmitter } from "events"

export { EventEmitter }

// Re-export shutdownEvent from cli-shim
export { shutdownEvent } from "./cli-shim"

// Re-export window and CLI_LOG_FILE from cli-shim
export { window, CLI_LOG_FILE } from "./cli-shim"