/**
 * Lightweight proto stubs for CLI — interfaces and create() only, no protobuf dependency.
 * Includes host service interfaces to eliminate @generated/hosts/ dependency.
 */

export enum Setting {
	DISABLED = 0,
	ENABLED = 1,
	UNRECOGNIZED = -1,
}

export enum ShowMessageType {
	ERROR = 0,
	WARNING = 1,
	INFORMATION = 2,
	UNRECOGNIZED = -1,
}

// --- Diff ---

export interface OpenDiffRequest {
	metadata?: any
	path?: string
	content?: string
}
export const OpenDiffRequest = {
	create(o: Partial<OpenDiffRequest> = {}): OpenDiffRequest { return { ...o } },
}

export interface OpenDiffResponse { diffId?: string }
export const OpenDiffResponse = {
	create(o: Partial<OpenDiffResponse> = {}): OpenDiffResponse { return { ...o } },
}

export interface GetDocumentTextRequest { metadata?: any; diffId?: string }
export const GetDocumentTextRequest = {
	create(o: Partial<GetDocumentTextRequest> = {}): GetDocumentTextRequest { return { ...o } },
}

export interface GetDocumentTextResponse { content?: string }
export const GetDocumentTextResponse = {
	create(o: Partial<GetDocumentTextResponse> = {}): GetDocumentTextResponse { return { content: "", ...o } },
}

export interface ReplaceTextRequest {
	metadata?: any; diffId?: string; content?: string
	startLine?: number; endLine?: number
}
export interface ReplaceTextResponse {}
export const ReplaceTextResponse = {
	create(o: Partial<ReplaceTextResponse> = {}): ReplaceTextResponse { return { ...o } },
}

export interface ScrollDiffRequest { diffId?: string; line?: number }
export interface ScrollDiffResponse {}
export const ScrollDiffResponse = {
	create(o: Partial<ScrollDiffResponse> = {}): ScrollDiffResponse { return { ...o } },
}

export interface TruncateDocumentRequest { metadata?: any; diffId?: string; endLine?: number }
export interface TruncateDocumentResponse {}
export const TruncateDocumentResponse = {
	create(o: Partial<TruncateDocumentResponse> = {}): TruncateDocumentResponse { return { ...o } },
}

export interface SaveDocumentRequest { metadata?: any; diffId?: string }
export interface SaveDocumentResponse {}
export const SaveDocumentResponse = {
	create(o: Partial<SaveDocumentResponse> = {}): SaveDocumentResponse { return { ...o } },
}

export interface CloseAllDiffsRequest {}
export interface CloseAllDiffsResponse {}
export const CloseAllDiffsResponse = {
	create(o: Partial<CloseAllDiffsResponse> = {}): CloseAllDiffsResponse { return { ...o } },
}

export interface ContentDiff { filePath?: string; leftContent?: string; rightContent?: string }
export interface OpenMultiFileDiffRequest { title?: string; diffs: ContentDiff[] }
export interface OpenMultiFileDiffResponse {}
export const OpenMultiFileDiffResponse = {
	create(o: Partial<OpenMultiFileDiffResponse> = {}): OpenMultiFileDiffResponse { return { ...o } },
}

// --- Env ---

export interface GetHostVersionResponse {
	platform?: string; version?: string; diracType?: string; diracVersion?: string
}
export const GetHostVersionResponse = {
	create(o: Partial<GetHostVersionResponse> = {}): GetHostVersionResponse { return { ...o } },
}

export interface GetTelemetrySettingsResponse { isEnabled: Setting; errorLevel?: string }
export const GetTelemetrySettingsResponse = {
	create(o: Partial<GetTelemetrySettingsResponse> = {}): GetTelemetrySettingsResponse {
		return { isEnabled: Setting.ENABLED, ...o }
	},
}

export interface TelemetrySettingsEvent { isEnabled: Setting; errorLevel?: string }
export const TelemetrySettingsEvent = {
	create(o: Partial<TelemetrySettingsEvent> = {}): TelemetrySettingsEvent {
		return { isEnabled: Setting.ENABLED, ...o }
	},
}

// --- Window ---

export interface ShowTextDocumentRequest { path: string; options?: any }
export interface TextEditorInfo { documentPath: string; viewColumn?: number; isActive: boolean }
export const TextEditorInfo = {
	create(o: Partial<TextEditorInfo> = {}): TextEditorInfo {
		return { documentPath: "", isActive: false, ...o }
	},
}

export interface ShowOpenDialogueRequest { canSelectMany?: boolean; openLabel?: string; filters?: any }
export interface SelectedResources { paths: string[] }
export const SelectedResources = {
	create(o: Partial<SelectedResources> = {}): SelectedResources { return { paths: [], ...o } },
}

export interface ShowMessageRequest { type: ShowMessageType; message: string; options?: any }
export interface SelectedResponse { selectedOption?: string }
export const SelectedResponse = {
	create(o: Partial<SelectedResponse> = {}): SelectedResponse { return { ...o } },
}

export interface ShowInputBoxRequest { title: string; prompt?: string; value?: string }
export interface ShowInputBoxResponse { response?: string }
export const ShowInputBoxResponse = {
	create(o: Partial<ShowInputBoxResponse> = {}): ShowInputBoxResponse { return { response: "", ...o } },
}

export interface ShowSaveDialogRequest { options?: any }
export interface ShowSaveDialogResponse { selectedPath?: string }
export const ShowSaveDialogResponse = {
	create(o: Partial<ShowSaveDialogResponse> = {}): ShowSaveDialogResponse { return { selectedPath: "", ...o } },
}

export interface OpenFileRequest { filePath: string }
export interface OpenFileResponse {}
export const OpenFileResponse = {
	create(o: Partial<OpenFileResponse> = {}): OpenFileResponse { return { ...o } },
}

export interface OpenSettingsRequest { query?: string }
export interface OpenSettingsResponse {}
export const OpenSettingsResponse = {
	create(o: Partial<OpenSettingsResponse> = {}): OpenSettingsResponse { return { ...o } },
}

export interface GetOpenTabsRequest {}
export interface GetOpenTabsResponse { paths: string[] }
export const GetOpenTabsResponse = {
	create(o: Partial<GetOpenTabsResponse> = {}): GetOpenTabsResponse { return { paths: [], ...o } },
}

export interface GetVisibleTabsRequest {}
export interface GetVisibleTabsResponse { paths: string[] }
export const GetVisibleTabsResponse = {
	create(o: Partial<GetVisibleTabsResponse> = {}): GetVisibleTabsResponse { return { paths: [], ...o } },
}

export interface GetActiveEditorRequest {}
export interface GetActiveEditorResponse { filePath?: string }
export const GetActiveEditorResponse = {
	create(o: Partial<GetActiveEditorResponse> = {}): GetActiveEditorResponse { return { ...o } },
}

// --- Workspace ---

export interface GetWorkspacePathsRequest { id?: string }
export interface GetWorkspacePathsResponse { id?: string; paths: string[] }
export const GetWorkspacePathsResponse = {
	create(o: Partial<GetWorkspacePathsResponse> = {}): GetWorkspacePathsResponse {
		return { paths: [], ...o }
	},
}

export interface SaveOpenDocumentIfDirtyRequest { filePath?: string }
export interface SaveOpenDocumentIfDirtyResponse { wasSaved?: boolean }
export const SaveOpenDocumentIfDirtyResponse = {
	create(o: Partial<SaveOpenDocumentIfDirtyResponse> = {}): SaveOpenDocumentIfDirtyResponse { return { ...o } },
}

export interface OpenProblemsPanelRequest {}
export interface OpenProblemsPanelResponse {}
export const OpenProblemsPanelResponse = {
	create(o: Partial<OpenProblemsPanelResponse> = {}): OpenProblemsPanelResponse { return { ...o } },
}

export interface OpenInFileExplorerPanelRequest { path: string }
export interface OpenInFileExplorerPanelResponse {}
export const OpenInFileExplorerPanelResponse = {
	create(o: Partial<OpenInFileExplorerPanelResponse> = {}): OpenInFileExplorerPanelResponse { return { ...o } },
}

export interface OpenDiracSidebarPanelRequest {}
export interface OpenDiracSidebarPanelResponse {}
export const OpenDiracSidebarPanelResponse = {
	create(o: Partial<OpenDiracSidebarPanelResponse> = {}): OpenDiracSidebarPanelResponse { return { ...o } },
}

export interface OpenTerminalRequest {}
export interface OpenTerminalResponse {}
export const OpenTerminalResponse = {
	create(o: Partial<OpenTerminalResponse> = {}): OpenTerminalResponse { return { ...o } },
}

export interface ExecuteCommandInTerminalRequest { command: string }
export interface ExecuteCommandInTerminalResponse { success: boolean }
export const ExecuteCommandInTerminalResponse = {
	create(o: Partial<ExecuteCommandInTerminalResponse> = {}): ExecuteCommandInTerminalResponse {
		return { success: false, ...o }
	},
}

export interface OpenFolderRequest { path: string; newWindow: boolean }
export interface OpenFolderResponse { success: boolean }
export const OpenFolderResponse = {
	create(o: Partial<OpenFolderResponse> = {}): OpenFolderResponse { return { success: false, ...o } },
}

// --- Service interfaces (replaces @generated/hosts/host-bridge-client-types) ---

export interface DiffServiceClientInterface {
	openDiff(request: OpenDiffRequest): Promise<OpenDiffResponse>
	getDocumentText(request: GetDocumentTextRequest): Promise<GetDocumentTextResponse>
	replaceText(request: ReplaceTextRequest): Promise<ReplaceTextResponse>
	scrollDiff(request: ScrollDiffRequest): Promise<ScrollDiffResponse>
	truncateDocument(request: TruncateDocumentRequest): Promise<TruncateDocumentResponse>
	saveDocument(request: SaveDocumentRequest): Promise<SaveDocumentResponse>
	closeAllDiffs(request: CloseAllDiffsRequest): Promise<CloseAllDiffsResponse>
	openMultiFileDiff(request: OpenMultiFileDiffRequest): Promise<OpenMultiFileDiffResponse>
}

export interface EnvServiceClientInterface {
	clipboardWriteText(request: import("./dirac").StringRequest): Promise<import("./dirac").Empty>
	clipboardReadText(request: import("./dirac").EmptyRequest): Promise<import("./dirac").String>
	getHostVersion(request: import("./dirac").EmptyRequest): Promise<GetHostVersionResponse>
	getIdeRedirectUri(request: import("./dirac").EmptyRequest): Promise<import("./dirac").String>
	getTelemetrySettings(request: import("./dirac").EmptyRequest): Promise<GetTelemetrySettingsResponse>
	subscribeToTelemetrySettings(request: import("./dirac").EmptyRequest, callbacks: StreamingCallbacks<TelemetrySettingsEvent>): () => void
	shutdown(request: import("./dirac").EmptyRequest): Promise<import("./dirac").Empty>
	debugLog(request: import("./dirac").StringRequest): Promise<import("./dirac").Empty>
	openExternal(request: import("./dirac").StringRequest): Promise<import("./dirac").Empty>
}

export interface WindowServiceClientInterface {
	showTextDocument(request: ShowTextDocumentRequest): Promise<TextEditorInfo>
	showOpenDialogue(request: ShowOpenDialogueRequest): Promise<SelectedResources>
	showMessage(request: ShowMessageRequest): Promise<SelectedResponse>
	showInputBox(request: ShowInputBoxRequest): Promise<ShowInputBoxResponse>
	showSaveDialog(request: ShowSaveDialogRequest): Promise<ShowSaveDialogResponse>
	openFile(request: OpenFileRequest): Promise<OpenFileResponse>
	openSettings(request: OpenSettingsRequest): Promise<OpenSettingsResponse>
	getOpenTabs(request: GetOpenTabsRequest): Promise<GetOpenTabsResponse>
	getVisibleTabs(request: GetVisibleTabsRequest): Promise<GetVisibleTabsResponse>
	getActiveEditor(request: GetActiveEditorRequest): Promise<GetActiveEditorResponse>
}

export interface WorkspaceServiceClientInterface {
	getWorkspacePaths(request: GetWorkspacePathsRequest): Promise<GetWorkspacePathsResponse>
	saveOpenDocumentIfDirty(request: SaveOpenDocumentIfDirtyRequest): Promise<SaveOpenDocumentIfDirtyResponse>
	openProblemsPanel(request: OpenProblemsPanelRequest): Promise<OpenProblemsPanelResponse>
	openInFileExplorerPanel(request: OpenInFileExplorerPanelRequest): Promise<OpenInFileExplorerPanelResponse>
	openDiracSidebarPanel(request: OpenDiracSidebarPanelRequest): Promise<OpenDiracSidebarPanelResponse>
	openTerminalPanel(request: OpenTerminalRequest): Promise<OpenTerminalResponse>
	executeCommandInTerminal(request: ExecuteCommandInTerminalRequest): Promise<ExecuteCommandInTerminalResponse>
	openFolder(request: OpenFolderRequest): Promise<OpenFolderResponse>
}

export interface StreamingCallbacks<T = any> {
	onResponse: (response: T) => void
	onError?: (error: Error) => void
	onComplete?: () => void
}

export interface HostBridgeClientProvider {
	workspaceClient: WorkspaceServiceClientInterface
	envClient: EnvServiceClientInterface
	windowClient: WindowServiceClientInterface
	diffClient: DiffServiceClientInterface
}
