/**
 * VS Code context shim for CLI
 * Provides the context initialization that the VS Code extension would normally provide
 */

import * as path from "path"
import * as os from "os"
import { createStorageContext, type StorageContext } from "@/shared/storage/storage-context"

// Re-export StorageContext for consumers
export type { StorageContext }

/**
 * CLI-specific context result that mirrors VS Code extension context
 */
export interface CliContextResult {
	extensionContext: any
	storageContext: StorageContext
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

	// Create proper file-backed storage context with globalState, secrets, workspaceState
	const storageContext = createStorageContext({
		diracDir,
		workspacePath: options.workspaceDir || process.cwd(),
	})

	return {
		extensionContext,
		storageContext,
		DATA_DIR,
		EXTENSION_DIR,
	}
}
