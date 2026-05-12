// Replaces proto-generated host/window types with plain TypeScript.

export enum ShowMessageType {
    ERROR = 0,
    INFORMATION = 1,
    WARNING = 2,
}

// --- ShowTextDocument ---

export interface ShowTextDocumentOptions {
    preview?: boolean
    preserveFocus?: boolean
    viewColumn?: number
}
export const ShowTextDocumentOptions = {
    create(o: Partial<ShowTextDocumentOptions> = {}): ShowTextDocumentOptions {
        return { ...o }
    },
}

export interface ShowTextDocumentRequest {
    path: string
    options?: ShowTextDocumentOptions
}
export const ShowTextDocumentRequest = {
    create(o: Partial<ShowTextDocumentRequest> = {}): ShowTextDocumentRequest {
        return { path: o.path ?? "" }
    },
}

export interface TextEditorInfo {
    documentPath: string
    viewColumn?: number
    isActive: boolean
}
export const TextEditorInfo = {
    create(o: Partial<TextEditorInfo> = {}): TextEditorInfo {
        return { documentPath: "", isActive: false, ...o }
    },
}

// --- ShowOpenDialogue ---

export interface ShowOpenDialogueFilterOption {
    files: string[]
}
export const ShowOpenDialogueFilterOption = {
    create(o: Partial<ShowOpenDialogueFilterOption> = {}): ShowOpenDialogueFilterOption {
        return { files: o.files ?? [] }
    },
}

export interface ShowOpenDialogueRequest {
    canSelectMany?: boolean
    openLabel?: string
    filters?: ShowOpenDialogueFilterOption
}
export const ShowOpenDialogueRequest = {
    create(o: Partial<ShowOpenDialogueRequest> = {}): ShowOpenDialogueRequest {
        return { ...o }
    },
}

export interface SelectedResources {
    paths: string[]
}
export const SelectedResources = {
    create(o: Partial<SelectedResources> = {}): SelectedResources {
        return { paths: o.paths ?? [] }
    },
}

// --- ShowMessage ---

export interface ShowMessageRequestOptions {
    items?: string[]
    modal?: boolean
    detail?: string
}
export const ShowMessageRequestOptions = {
    create(o: Partial<ShowMessageRequestOptions> = {}): ShowMessageRequestOptions {
        return { ...o }
    },
}

export interface ShowMessageRequest {
    type: ShowMessageType
    message: string
    options?: ShowMessageRequestOptions
}
export const ShowMessageRequest = {
    create(o: Partial<ShowMessageRequest> = {}): ShowMessageRequest {
        return { type: o.type ?? ShowMessageType.ERROR, message: o.message ?? "" }
    },
}

export interface SelectedResponse {
    selectedOption?: string
}
export const SelectedResponse = {
    create(o: Partial<SelectedResponse> = {}): SelectedResponse {
        return { ...o }
    },
}

// --- ShowSaveDialog ---

export interface ShowSaveDialogOptions {
    defaultPath?: string
    filters?: Record<string, { extensions: string[] }>
}
export const ShowSaveDialogOptions = {
    create(o: Partial<ShowSaveDialogOptions> = {}): ShowSaveDialogOptions {
        return { ...o }
    },
}

export interface ShowSaveDialogRequest {
    options?: ShowSaveDialogOptions
}
export const ShowSaveDialogRequest = {
    create(o: Partial<ShowSaveDialogRequest> = {}): ShowSaveDialogRequest {
        return { ...o }
    },
}

export interface ShowSaveDialogResponse {
    selectedPath?: string
}
export const ShowSaveDialogResponse = {
    create(o: Partial<ShowSaveDialogResponse> = {}): ShowSaveDialogResponse {
        return { ...o }
    },
}

// --- ShowInputBox ---

export interface ShowInputBoxRequest {
    title: string
    prompt?: string
    value?: string
}
export const ShowInputBoxRequest = {
    create(o: Partial<ShowInputBoxRequest> = {}): ShowInputBoxRequest {
        return { title: o.title ?? "" }
    },
}

export interface ShowInputBoxResponse {
    response?: string
}
export const ShowInputBoxResponse = {
    create(o: Partial<ShowInputBoxResponse> = {}): ShowInputBoxResponse {
        return { ...o }
    },
}

// --- OpenFile ---

export interface OpenFileRequest {
    filePath: string
}
export const OpenFileRequest = {
    create(o: Partial<OpenFileRequest> = {}): OpenFileRequest {
        return { filePath: o.filePath ?? "" }
    },
}

export interface OpenFileResponse {}
export const OpenFileResponse = {
    create(o: Partial<OpenFileResponse> = {}): OpenFileResponse {
        return { ...o }
    },
}

// --- OpenSettings ---

export interface OpenSettingsRequest {
    query?: string
}
export const OpenSettingsRequest = {
    create(o: Partial<OpenSettingsRequest> = {}): OpenSettingsRequest {
        return { ...o }
    },
}

export interface OpenSettingsResponse {}
export const OpenSettingsResponse = {
    create(o: Partial<OpenSettingsResponse> = {}): OpenSettingsResponse {
        return { ...o }
    },
}

// --- GetOpenTabs ---

export interface GetOpenTabsRequest {}
export const GetOpenTabsRequest = {
    create(o: Partial<GetOpenTabsRequest> = {}): GetOpenTabsRequest {
        return { ...o }
    },
}

export interface GetOpenTabsResponse {
    paths: string[]
}
export const GetOpenTabsResponse = {
    create(o: Partial<GetOpenTabsResponse> = {}): GetOpenTabsResponse {
        return { paths: o.paths ?? [] }
    },
}

// --- GetVisibleTabs ---

export interface GetVisibleTabsRequest {}
export const GetVisibleTabsRequest = {
    create(o: Partial<GetVisibleTabsRequest> = {}): GetVisibleTabsRequest {
        return { ...o }
    },
}

export interface GetVisibleTabsResponse {
    paths: string[]
}
export const GetVisibleTabsResponse = {
    create(o: Partial<GetVisibleTabsResponse> = {}): GetVisibleTabsResponse {
        return { paths: o.paths ?? [] }
    },
}

// --- GetActiveEditor ---

export interface GetActiveEditorRequest {}
export const GetActiveEditorRequest = {
    create(o: Partial<GetActiveEditorRequest> = {}): GetActiveEditorRequest {
        return { ...o }
    },
}

export interface GetActiveEditorResponse {
    filePath?: string
}
export const GetActiveEditorResponse = {
    create(o: Partial<GetActiveEditorResponse> = {}): GetActiveEditorResponse {
        return { ...o }
    },
}
