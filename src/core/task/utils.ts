import { ApiHandler } from "@core/api"
import { execSync } from "child_process"
import { showSystemNotification } from "@/integrations/notifications"
import { DiracApiReqCancelReason, DiracApiReqInfo, DiracMessage } from "@/shared/ExtensionMessage"
import { getApiMetrics } from "@/shared/getApiMetrics"
import { Logger } from "@/shared/services/Logger"
import { calculateApiCostAnthropic } from "@/utils/cost"
import { calculateApiCostOpenAI, calculateApiCostQwen } from "@/utils/cost"
import { MessageStateHandler } from "./message-state"

export const showNotificationForApproval = (message: string, notificationsEnabled: boolean) => {
	if (notificationsEnabled) {
		showSystemNotification({
			subtitle: "Approval Required",
			message,
		})
	}
}

type UpdateApiReqMsgParams = {
	messageStateHandler: MessageStateHandler
	lastApiReqIndex: number
	inputTokens: number
	reasoningTokens: number
	outputTokens: number
	cacheWriteTokens: number
	cacheReadTokens: number
	totalCost?: number
	api: ApiHandler
	cancelReason?: DiracApiReqCancelReason
	streamingFailedMessage?: string
	contextWindow?: number
	contextUsagePercentage?: number
}

// update api_req_started. we can't use api_req_finished anymore since it's a unique case where it could come after a streaming message (ie in the middle of being updated or executed)
// fortunately api_req_finished was always parsed out for the gui anyways, so it remains solely for legacy purposes to keep track of prices in tasks from history
// (it's worth removing a few months from now)
export const updateApiReqMsg = async (params: UpdateApiReqMsgParams) => {
	const diracMessages = params.messageStateHandler.getDiracMessages()
	const currentApiReqInfo: DiracApiReqInfo = JSON.parse(diracMessages[params.lastApiReqIndex].text || "{}")
	delete currentApiReqInfo.retryStatus // Clear retry status when request is finalized

	await params.messageStateHandler.updateDiracMessage(params.lastApiReqIndex, {
		text: JSON.stringify({
			...currentApiReqInfo, // Spread the modified info (with retryStatus removed)
			tokensIn: params.inputTokens,
			tokensOut: params.outputTokens,
			reasoningTokens: params.reasoningTokens,
			cacheWrites: params.cacheWriteTokens,
			cacheReads: params.cacheReadTokens,
			cost:
				params.totalCost ??
				(() => {
					const info = params.api.getModel().info
					const provider = params.api.constructor.name
					if (provider === "ApiGatewayHandler" || provider === "ZAiHandler" || provider === "OpenAiHandler" || provider === "DeepSeekHandler") {
						return calculateApiCostOpenAI(
							info,
							params.inputTokens,
							params.outputTokens,
							params.cacheWriteTokens,
							params.cacheReadTokens,
							undefined,
							params.reasoningTokens,
						)
					}
					if (provider === "QwenHandler") {
						return calculateApiCostQwen(
							info,
							params.inputTokens,
							params.outputTokens,
							params.cacheWriteTokens,
							params.cacheReadTokens,
							undefined,
							params.reasoningTokens,
						)
					}
					return calculateApiCostAnthropic(
						info,
						params.inputTokens,
						params.outputTokens,
						params.cacheWriteTokens,
						params.cacheReadTokens,
						undefined,
						params.reasoningTokens,
					)
				})(),
			cancelReason: params.cancelReason,
			streamingFailedMessage: params.streamingFailedMessage,
			contextWindow: params.contextWindow,
			contextUsagePercentage: params.contextUsagePercentage,
		} satisfies DiracApiReqInfo),
	})
}

/**
 * Common CLI tools that developers frequently use
 */
const CLI_TOOLS = [
	"gh",
	"git",
	"docker",
	"podman",
	"kubectl",
	"aws",
	"gcloud",
	"az",
	"terraform",
	"pulumi",
	"npm",
	"yarn",
	"pnpm",
	"pip",
	"cargo",
	"go",
	"curl",
	"jq",
	"make",
	"cmake",
	"python",
	"node",
	"psql",
	"mysql",
	"redis-cli",
	"sqlite3",
	"mongosh",
	"code",
	"grep",
	"sed",
	"awk",
	"brew",
	"apt",
	"yum",
	"gradle",
	"mvn",
	"bundle",
	"dotnet",
	"helm",
	"ansible",
	"wget",
]

/**
 * Detect which CLI tools are available in the system PATH
 * Uses 'which' command on Unix-like systems and 'where' on Windows
 */
export async function detectAvailableCliTools(): Promise<string[]> {
	const availableCommands: string[] = []
	const isWindows = process.platform === "win32"
	const checkCommand = isWindows ? "where" : "which"

	for (const command of CLI_TOOLS) {
		try {
			// Use execSync to check if the command exists
			execSync(`${checkCommand} ${command}`, {
				stdio: "ignore", // Don't output to console
				timeout: 1000, // 1 second timeout to avoid hanging
			})
			availableCommands.push(command)
		} catch (error) {
			// Command not found, skip it
		}
	}

	return availableCommands
}

/**
 * Extracts the domain from a provider URL string
 * @param url The URL to extract domain from
 * @returns The domain/hostname or undefined if invalid
 */
export function extractProviderDomainFromUrl(url: string | undefined): string | undefined {
	if (!url) {
		return undefined
	}
	try {
		const urlObj = new URL(url)
		return urlObj.hostname
	} catch {
		return undefined
	}
}

type ToolCallEntry = {
	tool: string
	status: "ok" | "error" | "truncated"
	tokens: number
	cached: boolean
	timestamp: number
	durationMs: number
	hint?: string
	retries?: number
}

type SessionSummaryDeps = {
	taskId: string
	messages: DiracMessage[]
	totalToolCallCount: number
	taskStartTimeMs: number
	recoveryEngine?: { getTelemetry(): Record<string, unknown> }
	toolCallLog?: ToolCallEntry[]
	modelId?: string
}

export function printSessionSummary(deps: SessionSummaryDeps): void {
	const metrics = getApiMetrics(deps.messages)
	const durationMs = Date.now() - deps.taskStartTimeMs
	const durationStr = formatDuration(durationMs)
	const taskPrefix = deps.taskId.slice(0, 8)
	const modelTag = deps.modelId ? ' | model=' + deps.modelId : ''

	const tokensIn = metrics.totalTokensIn
	const tokensOut = metrics.totalTokensOut
	const cost = metrics.totalCost
	const hasMetrics = tokensIn > 0 || tokensOut > 0

	const recoveryTelemetry = deps.recoveryEngine?.getTelemetry()
	const hasRecovery = recoveryTelemetry && (recoveryTelemetry.interceptedCount as number) > 0

	Logger.info(
		'[Session Summary] task=' + taskPrefix + modelTag + ' | duration=' + durationStr + ' | tools=' + deps.totalToolCallCount +
		(hasMetrics ? ' | tokens=' + tokensIn + ' in / ' + tokensOut + ' out' : ' | tokens=n/a') +
		(hasMetrics && cost > 0 ? ' | cost=$' + cost.toFixed(4) : '') +
		(hasRecovery
			? ' | recovery: ' + (recoveryTelemetry!.interceptedCount as number) + ' intercepted (saved ~' + (recoveryTelemetry!.totalTurnSavings as number).toFixed(1) + ' turns), ' + (recoveryTelemetry!.escalatedCount as number) + ' escalated, rate=' + recoveryTelemetry!.recoveryRate
			: '') +
		(deps.toolCallLog && deps.toolCallLog.length > 0
			? (() => {
				const log = deps.toolCallLog!
				const errors = log.filter(e => e.status === 'error').length
				const okRate = ((log.length - errors) / log.length * 100).toFixed(0)
				const cacheRate = (log.filter(e => e.cached).length / log.length * 100).toFixed(0)
				const totalRetries = log.reduce((s, e) => s + (e.retries ?? 0), 0)
				const hintCount = log.filter(e => e.hint).length
				// Write\u2192read verification
				let verified = 0, writes = 0
				for (let i = 0; i < log.length; i++) {
					if (log[i].tool === 'write' || log[i].tool === 'edit') {
						writes++
						for (let j = i + 1; j < Math.min(i + 4, log.length); j++) {
							if (log[j].tool === 'read') { verified++; break }
						}
					}
				}
				const verifyRate = writes > 0 ? (verified / writes * 100).toFixed(0) : 'n/a'
				// Per-tool breakdown (top 5) with avg duration
				const byTool = new Map<string, { ok: number; err: number; retried: number; totalMs: number }>()
				for (const e of log) {
					const t = byTool.get(e.tool) ?? { ok: 0, err: 0, retried: 0, totalMs: 0 }
					if (e.status === 'ok') t.ok++; else t.err++
					if (e.retries) t.retried++
					t.totalMs += e.durationMs || 0
					byTool.set(e.tool, t)
				}
				const topTools = [...byTool.entries()]
					.sort((a, b) => (b[1].ok + b[1].err) - (a[1].ok + a[1].err))
					.slice(0, 5)
					.map(([name, t]) => name + ':' + (t.ok + t.err) + (t.err ? '(' + t.err + 'err)' : '') + '[' + Math.round(t.totalMs / (t.ok + t.err)) + 'ms]')
					.join(' ')
				const outputTokens = log.reduce((s, e) => s + e.tokens, 0)
				const avgDuration = Math.round(log.reduce((s, e) => s + (e.durationMs || 0), 0) / log.length)
				let m = ' | tools: ' + log.length + ' calls, success=' + okRate + '%, cache=' + cacheRate + '%, verify=' + verifyRate + '%, avg=' + avgDuration + 'ms'
				if (errors) m += ', errors=' + errors
				if (totalRetries) m += ', retries=' + totalRetries
				if (hintCount) m += ', hints=' + hintCount
				m += ', output_tokens=' + outputTokens
				m += ' | top: ' + topTools
				return m
			})()
			: ''),
	)

		// Structured metrics for cross-session analysis (machine-parseable JSON in logs)
		if (deps.toolCallLog && deps.toolCallLog.length > 0) {
			const log = deps.toolCallLog
			const byTool = new Map<string, { calls: number; ok: number; err: number; truncated: number; cached: number; retried: number; totalMs: number }>()
			for (const e of log) {
				const t = byTool.get(e.tool) ?? { calls: 0, ok: 0, err: 0, truncated: 0, cached: 0, retried: 0, totalMs: 0 }
				t.calls++
				if (e.status === 'ok') t.ok++; else if (e.status === 'error') t.err++; else t.truncated++
				if (e.cached) t.cached++
				if (e.retries) t.retried++
				t.totalMs += e.durationMs || 0
				byTool.set(e.tool, t)
			}
			const perTool: Record<string, { calls: number; ok: number; err: number; truncated: number; cached: number; retried: number; avgMs: number }> = {}
			for (const [name, t] of byTool) {
				perTool[name] = { calls: t.calls, ok: t.ok, err: t.err, truncated: t.truncated, cached: t.cached, retried: t.retried, avgMs: Math.round(t.totalMs / t.calls) }
			}
			Logger.info('[Tool Metrics]', {
				taskId: taskPrefix,
				model: deps.modelId,
				durationMs,
				totalCalls: log.length,
				successRate: parseFloat(((log.filter(e => e.status === 'ok').length / log.length) * 100).toFixed(1)),
				cacheHitRate: parseFloat(((log.filter(e => e.cached).length / log.length) * 100).toFixed(1)),
				avgDurationMs: Math.round(log.reduce((s, e) => s + (e.durationMs || 0), 0) / log.length),
				totalErrors: log.filter(e => e.status === 'error').length,
				totalRetries: log.reduce((s, e) => s + (e.retries ?? 0), 0),
				totalHints: log.filter(e => e.hint).length,
				perTool,
			})
		}
}

function formatDuration(ms: number): string {
	const seconds = Math.floor(ms / 1000)
	if (seconds < 60) return seconds + 's'
	const minutes = Math.floor(seconds / 60)
	const remainingSeconds = seconds % 60
	if (minutes < 60) return minutes + 'm ' + remainingSeconds + 's'
	const hours = Math.floor(minutes / 60)
	const remainingMinutes = minutes % 60
	return hours + 'h ' + remainingMinutes + 'm'
}
