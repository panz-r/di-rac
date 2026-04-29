/**
 * VS Code context shim for CLI
 * Provides the context initialization that the VS Code extension would normally provide
 */

import * as path from "path"
import * as os from "os"
import { EventEmitter } from "events"

// Re-export EventEmitter for convenience
export { EventEmitter }

/**
 * CLI-specific context result that mirrors VS Code extension context
 */
export interface CliContextResult {
	extensionContext: any
	storageContext: any
	DATA_DIR: string
	EXTENSION_DIR: string
}

/**
 * Options for initializing CLI context
 */
export interface CliContextOptions {
	diracDir?: string
	workspaceDir?: string
}

/**
 * Initialize CLI context for standalone usage
 * Creates directory structure and context needed by Dirac core
 */
export function initializeCliContext(options: CliContextOptions = {}): CliContextResult {
	// Determine Dirac directory (config override or default)
	const diracDir = options.diracDir || path.join(os.homedir(), ".dirac")

	// Data directory for local storage
	const DATA_DIR = path.join(diracDir, "data")

	// Extension directory for bundled resources
	const EXTENSION_DIR = path.join(diracDir, "extension")

	// Create extension context (simplified mock for CLI)
	const extensionContext = {
		// Minimal extension context properties that Dirac core might use
		extensionPath: EXTENSION_DIR,
		extensionUri: `file://${EXTENSION_DIR}`,
		extension: {
			id: "dirac.cli",
			packageJSON: { version: "0.0.0" },
		},
		// Event emitter for compatibility with VS Code API
		subscriptions: [],
	}

	// Create storage context for state persistence
	const storageContext = {
		// Storage paths for CLI
		globalStoragePath: path.join(DATA_DIR, "globalStorage"),
		workspaceStoragePath: path.join(DATA_DIR, "workspaceStorage"),
		// Extension URI for storage
		extensionUri: `file://${DATA_DIR}`,
		workspace: {
			// Minimal workspace info
			name: path.basename(options.workspaceDir || process.cwd()),
			uri: `file://${options.workspaceDir || process.cwd()}`,
		},
		// Environment
		environmentVariableCollection: {
			persistent: false,
		},
	}

	return {
		extensionContext,
		storageContext,
		DATA_DIR,
		EXTENSION_DIR,
	}
}