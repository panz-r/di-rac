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

type SessionSummaryDeps = {
	taskId: string
	messages: DiracMessage[]
	totalToolCallCount: number
	taskStartTimeMs: number
	recoveryEngine?: { getTelemetry(): Record<string, unknown> }
}

export function printSessionSummary(deps: SessionSummaryDeps): void {
	const metrics = getApiMetrics(deps.messages)
	const durationMs = Date.now() - deps.taskStartTimeMs
	const durationStr = formatDuration(durationMs)
	const taskPrefix = deps.taskId.slice(0, 8)

	const tokensIn = metrics.totalTokensIn
	const tokensOut = metrics.totalTokensOut
	const cost = metrics.totalCost
	const hasMetrics = tokensIn > 0 || tokensOut > 0

	const recoveryTelemetry = deps.recoveryEngine?.getTelemetry()
	const hasRecovery = recoveryTelemetry && (recoveryTelemetry.interceptedCount as number) > 0

	Logger.info(
		'[Session Summary] task=' + taskPrefix + ' | duration=' + durationStr + ' | tools=' + deps.totalToolCallCount +
		(hasMetrics ? ' | tokens=' + tokensIn + ' in / ' + tokensOut + ' out' : ' | tokens=n/a') +
		(hasMetrics && cost > 0 ? ' | cost=$' + cost.toFixed(4) : '') +
		(hasRecovery
			? ' | recovery: ' + (recoveryTelemetry!.interceptedCount as number) + ' intercepted (saved ~' + (recoveryTelemetry!.totalTurnSavings as number).toFixed(1) + ' turns), ' + (recoveryTelemetry!.escalatedCount as number) + ' escalated, rate=' + recoveryTelemetry!.recoveryRate
			: ''),
	)
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
