import { IDiagnosticsProvider } from "./IDiagnosticsProvider"
import { SyntaxFeedbackProvider } from "./SyntaxFeedbackProvider"
import type { AnalyzerClient } from "@/services/tree-sitter/AnalyzerClient"

export function getDiagnosticsProviders(
	_useLinterOnlyForSyntax = false,
	_timeoutMs?: number,
	_delayMs?: number,
	analyzer?: AnalyzerClient,
): IDiagnosticsProvider[] {
	return [new SyntaxFeedbackProvider(analyzer)]
}
