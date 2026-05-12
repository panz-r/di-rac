// Replaces proto-generated dirac/common types with plain TypeScript.

export interface Metadata {}
export const Metadata = {
	create(_o: Partial<Metadata> = {}): Metadata {
		return {}
	},
}

export interface EmptyRequest {}
export const EmptyRequest = {
	create(_o: Partial<EmptyRequest> = {}): EmptyRequest {
		return {}
	},
}

export interface Empty {}
export const Empty = {
	create(_o: Partial<Empty> = {}): Empty {
		return {}
	},
}

export interface StringRequest {
	value: string
}
export const StringRequest = {
	create(o: Partial<StringRequest> = {}): StringRequest {
		return { value: o.value ?? "" }
	},
}

export interface StringArrayRequest {
	value: string[]
}
export const StringArrayRequest = {
	create(o: Partial<StringArrayRequest> = {}): StringArrayRequest {
		return { value: o.value ?? [] }
	},
}

export interface String {
	value: string
}
export const String = {
	create(o: Partial<String> = {}): String {
		return { value: o.value ?? "" }
	},
}

export interface Int64Request {
	value: number
}
export const Int64Request = {
	create(o: Partial<Int64Request> = {}): Int64Request {
		return { value: o.value ?? 0 }
	},
}

export interface Int64 {
	value: number
}
export const Int64 = {
	create(o: Partial<Int64> = {}): Int64 {
		return { value: o.value ?? 0 }
	},
}

export interface BytesRequest {
	value: Uint8Array
}
export const BytesRequest = {
	create(o: Partial<BytesRequest> = {}): BytesRequest {
		return { value: o.value ?? new Uint8Array(0) }
	},
}

export interface Bytes {
	value: Uint8Array
}
export const Bytes = {
	create(o: Partial<Bytes> = {}): Bytes {
		return { value: o.value ?? new Uint8Array(0) }
	},
}

export interface BooleanRequest {
	value: boolean
}
export const BooleanRequest = {
	create(o: Partial<BooleanRequest> = {}): BooleanRequest {
		return { value: o.value ?? false }
	},
}

export interface Boolean {
	value: boolean
}
export const Boolean = {
	create(o: Partial<Boolean> = {}): Boolean {
		return { value: o.value ?? false }
	},
}

export interface BooleanResponse {
	value: boolean
}
export const BooleanResponse = {
	create(o: Partial<BooleanResponse> = {}): BooleanResponse {
		return { value: o.value ?? false }
	},
}

export interface StringArray {
	values: string[]
}
export const StringArray = {
	create(o: Partial<StringArray> = {}): StringArray {
		return { values: o.values ?? [] }
	},
}

export interface StringArrays {
	values1: string[]
	values2: string[]
}
export const StringArrays = {
	create(o: Partial<StringArrays> = {}): StringArrays {
		return { values1: o.values1 ?? [], values2: o.values2 ?? [] }
	},
}

export interface KeyValuePair {
	key: string
	value: string
}
export const KeyValuePair = {
	create(o: Partial<KeyValuePair> = {}): KeyValuePair {
		return { key: o.key ?? "", value: o.value ?? "" }
	},
}

export enum DiagnosticSeverity {
	DIAGNOSTIC_ERROR = 0,
	DIAGNOSTIC_WARNING = 1,
	DIAGNOSTIC_INFORMATION = 2,
	DIAGNOSTIC_HINT = 3,
}

export interface DiagnosticPosition {
	line: number
	character: number
}
export const DiagnosticPosition = {
	create(o: Partial<DiagnosticPosition> = {}): DiagnosticPosition {
		return { line: o.line ?? 0, character: o.character ?? 0 }
	},
}

export interface DiagnosticRange {
	start: DiagnosticPosition
	end: DiagnosticPosition
}
export const DiagnosticRange = {
	create(o: Partial<DiagnosticRange> = {}): DiagnosticRange {
		return {
			start: o.start ?? DiagnosticPosition.create(),
			end: o.end ?? DiagnosticPosition.create(),
		}
	},
}

export interface Diagnostic {
	message: string
	range?: DiagnosticRange
	severity: DiagnosticSeverity
	source?: string
}
export const Diagnostic = {
	create(o: Partial<Diagnostic> = {}): Diagnostic {
		return { message: o.message ?? "", severity: o.severity ?? DiagnosticSeverity.DIAGNOSTIC_ERROR, range: o.range, source: o.source }
	},
}

export interface FileDiagnostics {
	filePath: string
	diagnostics: Diagnostic[]
}
export const FileDiagnostics = {
	create(o: Partial<FileDiagnostics> = {}): FileDiagnostics {
		return { filePath: o.filePath ?? "", diagnostics: o.diagnostics ?? [] }
	},
}
