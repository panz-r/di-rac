// Replaces proto-generated host/diff types with plain TypeScript.

import type { Metadata } from "../dirac/common"

// Re-export Metadata for the barrel.
export type { Metadata } from "../dirac/common"

// --- OpenDiff ---

export interface OpenDiffRequest {
    metadata?: Metadata
    path?: string
    content?: string
}
export const OpenDiffRequest = {
    create(o: Partial<OpenDiffRequest> = {}): OpenDiffRequest {
        return { ...o }
    },
}

export interface OpenDiffResponse {
    diffId?: string
}
export const OpenDiffResponse = {
    create(o: Partial<OpenDiffResponse> = {}): OpenDiffResponse {
        return { ...o }
    },
}

// --- GetDocumentText ---

export interface GetDocumentTextRequest {
    metadata?: Metadata
    diffId?: string
}
export const GetDocumentTextRequest = {
    create(o: Partial<GetDocumentTextRequest> = {}): GetDocumentTextRequest {
        return { ...o }
    },
}

export interface GetDocumentTextResponse {
    content?: string
}
export const GetDocumentTextResponse = {
    create(o: Partial<GetDocumentTextResponse> = {}): GetDocumentTextResponse {
        return { ...o }
    },
}

// --- ReplaceText ---

export interface ReplaceTextRequest {
    metadata?: Metadata
    diffId?: string
    content?: string
    startLine?: number
    endLine?: number
}
export const ReplaceTextRequest = {
    create(o: Partial<ReplaceTextRequest> = {}): ReplaceTextRequest {
        return { ...o }
    },
}

export interface ReplaceTextResponse {}
export const ReplaceTextResponse = {
    create(o: Partial<ReplaceTextResponse> = {}): ReplaceTextResponse {
        return { ...o }
    },
}

// --- ScrollDiff ---

export interface ScrollDiffRequest {
    diffId?: string
    line?: number
}
export const ScrollDiffRequest = {
    create(o: Partial<ScrollDiffRequest> = {}): ScrollDiffRequest {
        return { ...o }
    },
}

export interface ScrollDiffResponse {}
export const ScrollDiffResponse = {
    create(o: Partial<ScrollDiffResponse> = {}): ScrollDiffResponse {
        return { ...o }
    },
}

// --- TruncateDocument ---

export interface TruncateDocumentRequest {
    metadata?: Metadata
    diffId?: string
    endLine?: number
}
export const TruncateDocumentRequest = {
    create(o: Partial<TruncateDocumentRequest> = {}): TruncateDocumentRequest {
        return { ...o }
    },
}

export interface TruncateDocumentResponse {}
export const TruncateDocumentResponse = {
    create(o: Partial<TruncateDocumentResponse> = {}): TruncateDocumentResponse {
        return { ...o }
    },
}

// --- CloseAllDiffs ---

export interface CloseAllDiffsRequest {}
export const CloseAllDiffsRequest = {
    create(o: Partial<CloseAllDiffsRequest> = {}): CloseAllDiffsRequest {
        return { ...o }
    },
}

export interface CloseAllDiffsResponse {}
export const CloseAllDiffsResponse = {
    create(o: Partial<CloseAllDiffsResponse> = {}): CloseAllDiffsResponse {
        return { ...o }
    },
}

// --- SaveDocument ---

export interface SaveDocumentRequest {
    metadata?: Metadata
    diffId?: string
}
export const SaveDocumentRequest = {
    create(o: Partial<SaveDocumentRequest> = {}): SaveDocumentRequest {
        return { ...o }
    },
}

export interface SaveDocumentResponse {}
export const SaveDocumentResponse = {
    create(o: Partial<SaveDocumentResponse> = {}): SaveDocumentResponse {
        return { ...o }
    },
}

// --- ContentDiff ---

export interface ContentDiff {
    filePath?: string
    leftContent?: string
    rightContent?: string
}
export const ContentDiff = {
    create(o: Partial<ContentDiff> = {}): ContentDiff {
        return { ...o }
    },
}

// --- OpenMultiFileDiff ---

export interface OpenMultiFileDiffRequest {
    title?: string
    diffs: ContentDiff[]
}
export const OpenMultiFileDiffRequest = {
    create(o: Partial<OpenMultiFileDiffRequest> = {}): OpenMultiFileDiffRequest {
        return { diffs: o.diffs ?? [] }
    },
}

export interface OpenMultiFileDiffResponse {}
export const OpenMultiFileDiffResponse = {
    create(o: Partial<OpenMultiFileDiffResponse> = {}): OpenMultiFileDiffResponse {
        return { ...o }
    },
}
