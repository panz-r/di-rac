import type { HostBridgeClientProvider } from "@/hosts/host-provider-types"

const noOp = async () => ({})

export const vscodeHostBridgeClient = {
	workspaceClient: {
		getWorkspacePaths: async () => ({ paths: [] }),
		openDiracSidebarPanel: noOp,
		readFile: async () => ({ content: new Uint8Array() }),
		stat: async () => ({ type: 1, size: 0 }),
		readDirectory: async () => [],
		saveOpenDocumentIfDirty: noOp,
		openProblemsPanel: noOp,
		openInFileExplorerPanel: noOp,
		openTerminalPanel: noOp,
		executeCommandInTerminal: async () => ({ success: true }),
		openFolder: async () => ({ success: true }),
	},
	envClient: {
		clipboardWriteText: noOp,
		clipboardReadText: async () => ({ value: "" }),
		getHostVersion: async () => ({
			version: "1.0.0",
			platform: "unknown",
			diracVersion: "0.0.0",
			diracType: "cli",
		}),
		getIdeRedirectUri: async () => ({ value: "http://localhost:1234/callback" }),
		getTelemetrySettings: async () => ({ isEnabled: false, userId: "" }),
		subscribeToTelemetrySettings: () => () => {},
		shutdown: noOp,
		debugLog: noOp,
		openExternal: noOp,
	},
	windowClient: {
		showMessage: noOp,
		showTextDocument: noOp,
		showOpenDialogue: async () => ({ files: [] }),
		showInputBox: async () => ({ value: "" }),
		showSaveDialog: async () => ({ files: [] }),
		setStatusBarMessage: noOp,
		getActiveEditor: async () => ({ filePath: "", isSelected: false, visibleRange: undefined }),
		subscribeToActiveEditorChanges: () => () => {},
		subscribeToWindowStateChanges: () => () => {},
		getWindowState: async () => ({ focused: true }),
	},
	diffClient: {
		openDiff: noOp,
		getDocumentText: async () => ({ content: "" }),
		replaceText: noOp,
		scrollDiff: noOp,
		closeDiff: noOp,
		revertFiles: async () => ({ success: true }),
		areFilesEqual: async () => ({ equal: false }),
		closeAllDiffs: noOp,
		openCommentReview: noOp,
	},
} as unknown as HostBridgeClientProvider
