// Replaces proto-generated host/workspace types with plain TypeScript.

import type { Metadata } from "../dirac/common"

// --- GetWorkspacePaths ---

export interface GetWorkspacePathsRequest {
    id?: string
}
export const GetWorkspacePathsRequest = {
    create(o: Partial<GetWorkspacePathsRequest> = {}): GetWorkspacePathsRequest {
        return { ...o }
    },
}

export interface GetWorkspacePathsResponse {
    id?: string
    paths: string[]
}
export const GetWorkspacePathsResponse = {
    create(o: Partial<GetWorkspacePathsResponse> = {}): GetWorkspacePathsResponse {
        return { paths: o.paths ?? [] }
    },
}

// --- SaveOpenDocumentIfDirty ---

export interface SaveOpenDocumentIfDirtyRequest {
    filePath?: string
}
export const SaveOpenDocumentIfDirtyRequest = {
    create(o: Partial<SaveOpenDocumentIfDirtyRequest> = {}): SaveOpenDocumentIfDirtyRequest {
        return { ...o }
    },
}

export interface SaveOpenDocumentIfDirtyResponse {
    wasSaved?: boolean
}
export const SaveOpenDocumentIfDirtyResponse = {
    create(o: Partial<SaveOpenDocumentIfDirtyResponse> = {}): SaveOpenDocumentIfDirtyResponse {
        return { ...o }
    },
}

// --- GetDiagnostics ---

export interface GetDiagnosticsRequest {
    metadata?: Metadata
    filePaths: string[]
}
export const GetDiagnosticsRequest = {
    create(o: Partial<GetDiagnosticsRequest> = {}): GetDiagnosticsRequest {
        return { filePaths: o.filePaths ?? [] }
    },
}

export interface GetDiagnosticsResponse {
    fileDiagnostics: any[]
}
export const GetDiagnosticsResponse = {
    create(o: Partial<GetDiagnosticsResponse> = {}): GetDiagnosticsResponse {
        return { fileDiagnostics: o.fileDiagnostics ?? [] }
    },
}

// --- SearchWorkspaceItems ---

export enum SearchItemType {
    FILE = 0,
    FOLDER = 1,
}

export interface SearchWorkspaceItemsRequest {
    query: string
    limit?: number
    selectedType?: SearchItemType
}
export const SearchWorkspaceItemsRequest = {
    create(o: Partial<SearchWorkspaceItemsRequest> = {}): SearchWorkspaceItemsRequest {
        return { query: o.query ?? "" }
    },
}

export interface SearchItem {
    path: string
    type: SearchItemType
    label?: string
}
export const SearchItem = {
    create(o: Partial<SearchItem> = {}): SearchItem {
        return { path: o.path ?? "", type: o.type ?? SearchItemType.FILE }
    },
}

export interface SearchWorkspaceItemsResponse {
    items: SearchItem[]
}
export const SearchWorkspaceItemsResponse = {
    create(o: Partial<SearchWorkspaceItemsResponse> = {}): SearchWorkspaceItemsResponse {
        return { items: o.items ?? [] }
    },
}

// --- OpenProblemsPanel ---

export interface OpenProblemsPanelRequest {}
export const OpenProblemsPanelRequest = {
    create(o: Partial<OpenProblemsPanelRequest> = {}): OpenProblemsPanelRequest {
        return { ...o }
    },
}

export interface OpenProblemsPanelResponse {}
export const OpenProblemsPanelResponse = {
    create(o: Partial<OpenProblemsPanelResponse> = {}): OpenProblemsPanelResponse {
        return { ...o }
    },
}

// --- OpenInFileExplorerPanel ---

export interface OpenInFileExplorerPanelRequest {
    path: string
}
export const OpenInFileExplorerPanelRequest = {
    create(o: Partial<OpenInFileExplorerPanelRequest> = {}): OpenInFileExplorerPanelRequest {
        return { path: o.path ?? "" }
    },
}

export interface OpenInFileExplorerPanelResponse {}
export const OpenInFileExplorerPanelResponse = {
    create(o: Partial<OpenInFileExplorerPanelResponse> = {}): OpenInFileExplorerPanelResponse {
        return { ...o }
    },
}

// --- OpenDiracSidebarPanel ---

export interface OpenDiracSidebarPanelRequest {}
export const OpenDiracSidebarPanelRequest = {
    create(o: Partial<OpenDiracSidebarPanelRequest> = {}): OpenDiracSidebarPanelRequest {
        return { ...o }
    },
}

export interface OpenDiracSidebarPanelResponse {}
export const OpenDiracSidebarPanelResponse = {
    create(o: Partial<OpenDiracSidebarPanelResponse> = {}): OpenDiracSidebarPanelResponse {
        return { ...o }
    },
}

// --- OpenTerminal ---

export interface OpenTerminalRequest {}
export const OpenTerminalRequest = {
    create(o: Partial<OpenTerminalRequest> = {}): OpenTerminalRequest {
        return { ...o }
    },
}

export interface OpenTerminalResponse {}
export const OpenTerminalResponse = {
    create(o: Partial<OpenTerminalResponse> = {}): OpenTerminalResponse {
        return { ...o }
    },
}

// --- ExecuteCommandInTerminal ---

export interface ExecuteCommandInTerminalRequest {
    command: string
}
export const ExecuteCommandInTerminalRequest = {
    create(o: Partial<ExecuteCommandInTerminalRequest> = {}): ExecuteCommandInTerminalRequest {
        return { command: o.command ?? "" }
    },
}

export interface ExecuteCommandInTerminalResponse {
    success: boolean
}
export const ExecuteCommandInTerminalResponse = {
    create(o: Partial<ExecuteCommandInTerminalResponse> = {}): ExecuteCommandInTerminalResponse {
        return { success: o.success ?? false }
    },
}

// --- OpenFolder ---

export interface OpenFolderRequest {
    path: string
    newWindow: boolean
}
export const OpenFolderRequest = {
    create(o: Partial<OpenFolderRequest> = {}): OpenFolderRequest {
        return { path: o.path ?? "", newWindow: o.newWindow ?? false }
    },
}

export interface OpenFolderResponse {
    success: boolean
}
export const OpenFolderResponse = {
    create(o: Partial<OpenFolderResponse> = {}): OpenFolderResponse {
        return { success: o.success ?? false }
    },
}

// --- PrepareDiagnostics ---

export interface PrepareDiagnosticsRequest {
    filePaths: string[]
}
export const PrepareDiagnosticsRequest = {
    create(o: Partial<PrepareDiagnosticsRequest> = {}): PrepareDiagnosticsRequest {
        return { filePaths: o.filePaths ?? [] }
    },
}

export interface PrepareDiagnosticsResponse {
    success: boolean
}
export const PrepareDiagnosticsResponse = {
    create(o: Partial<PrepareDiagnosticsResponse> = {}): PrepareDiagnosticsResponse {
        return { success: o.success ?? false }
    },
}
