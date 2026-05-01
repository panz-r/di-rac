export interface ParsedDefinition {
	id: string // Structural handle, e.g., 'fn:ClassName.methodName'
	kind: string // 'function', 'class', 'method', 'interface', etc.
	name: string // Simple name of the symbol
	lineIndex: number // 0-based
	text: string // The line text where the definition starts
	indentation: string
	signature?: string // Full signature if available
	lineCount?: number
	calls?: string[]
	fullBodyRange?: {
		startLine: number
		endLine: number
		startIndex: number
		endIndex: number
	}
}

export interface ParseResult {
	definitions: ParsedDefinition[]
	imports: string[]
}

export { AnalyzerClient } from "./AnalyzerClient"
