/**
 * Mock host bridge client for testing
 * This provides a minimal mock implementation of the gRPC clients used in tests
 */
export const vscodeHostBridgeClient = {
	workspaceClient: {
		getWorkspacePaths: async () => ({ paths: [] }),
		openDiracSidebarPanel: async () => ({}),
		readFile: async () => ({ content: new Uint8Array() }),
		stat: async () => ({ type: 1, size: 0 }),
		readDirectory: async () => [],
	},
	envClient: {
		getHostVersion: async () => ({
			version: "1.0.0",
			platform: "unknown",
			diracVersion: "0.0.0",
			diracType: "cli",
		}),
		debugLog: async () => ({}),
	},
	windowClient: {
		showMessage: async () => ({}),
	},
	diffClient: {
		// Diff service not used in basic tests
	},
}
