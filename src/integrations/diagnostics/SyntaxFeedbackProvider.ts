import * as path from "path"
import { AnalyzerClient, CheckSyntaxResult } from "@/services/tree-sitter/AnalyzerClient"
import { Diagnostic, DiagnosticSeverity, FileDiagnostics } from "@/shared/proto/index.dirac"
import { Logger } from "@/shared/services/Logger"
import { DiagnosticsFeedbackResult, IDiagnosticsProvider } from "./IDiagnosticsProvider"
import { diagnosticsToProblemsString } from "./index"

const EXT_TO_LANG: Record<string, string> = {
	ts: "typescript", tsx: "typescript", js: "javascript", jsx: "javascript",
	py: "python", rs: "rust", go: "go", c: "c", cpp: "cpp",
	h: "c", hpp: "cpp", java: "java", php: "php", rb: "ruby",
	swift: "swift", kt: "java", bash: "bash", sh: "bash",
}

export class SyntaxFeedbackProvider implements IDiagnosticsProvider {
	constructor(private analyzer?: AnalyzerClient) {}

	async capturePreSaveState(): Promise<FileDiagnostics[]> {
		return []
	}

	async getDiagnosticsFeedback(
		filePath: string,
		content: string,
		_preSaveDiagnostics: FileDiagnostics[],
		hashes?: string[],
	): Promise<DiagnosticsFeedbackResult> {
		try {
			const ext = path.extname(filePath).toLowerCase().slice(1)
			const lang = EXT_TO_LANG[ext]

			if (!lang || !this.analyzer) {
				return { newProblemsMessage: "", fixedCount: 0 }
			}

			const result: CheckSyntaxResult = await this.analyzer.checkSyntaxContent(content, lang)

			if (!result.has_errors || result.errors.length === 0) {
				return { newProblemsMessage: "", fixedCount: 0 }
			}

			const errors: Diagnostic[] = result.errors.slice(0, 5).map((e) => ({
				range: {
					start: { line: e.start_line - 1, character: e.start_col - 1 },
					end: { line: e.end_line - 1, character: e.end_col - 1 },
				},
				message: e.message,
				severity: DiagnosticSeverity.DIAGNOSTIC_ERROR,
				source: "Syntax",
			}))

			const message = await diagnosticsToProblemsString(
				[{ filePath, diagnostics: errors }],
				[DiagnosticSeverity.DIAGNOSTIC_ERROR],
				new Map([[filePath, { lines: content.split("\n"), hashes }]]),
				5,
			)

			Logger.error(`[SyntaxFeedbackProvider] Returning syntax errors for ${filePath}: ${message}`)
			return {
				newProblemsMessage: message,
				fixedCount: 0,
			}
		} catch (error) {
			Logger.error(`Error in syntax check for ${filePath}:`, error)
			return { newProblemsMessage: "", fixedCount: 0 }
		}
	}

	async getDiagnosticsFeedbackForFiles(
		files: Array<{ filePath: string; content: string; hashes?: string[] }>,
		preSaveDiagnostics: FileDiagnostics[]
	): Promise<DiagnosticsFeedbackResult[]> {
		return Promise.all(
			files.map((f) => this.getDiagnosticsFeedback(f.filePath, f.content, preSaveDiagnostics, f.hashes))
		)
	}
}
