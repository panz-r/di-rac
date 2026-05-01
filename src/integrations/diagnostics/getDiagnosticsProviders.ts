import { IDiagnosticsProvider } from "./IDiagnosticsProvider"
import { LinterFeedbackProvider } from "./LinterFeedbackProvider"
import { SyntaxFeedbackProvider } from "./SyntaxFeedbackProvider"
import type { AnalyzerClient } from "@/services/tree-sitter/AnalyzerClient"

export function getDiagnosticsProviders(
	useLinterOnlyForSyntax = false,
	timeoutMs?: number,
	delayMs?: number,
	analyzer?: AnalyzerClient,
): IDiagnosticsProvider[] {
	if (useLinterOnlyForSyntax) {
		return [new SyntaxFeedbackProvider(analyzer)]
	}
	return [new SyntaxFeedbackProvider(analyzer), new LinterFeedbackProvider(timeoutMs, delayMs)]
}
