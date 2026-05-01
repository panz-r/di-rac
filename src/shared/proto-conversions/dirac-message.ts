import { DiracAsk as AppDiracAsk, DiracMessage as AppDiracMessage, DiracSay as AppDiracSay } from "@shared/ExtensionMessage"
import { DiracAsk, DiracMessageType, DiracSay, DiracMessage as ProtoDiracMessage } from "@shared/proto/dirac/ui"

// Helper function to convert DiracAsk string to enum
function convertDiracAskToProtoEnum(ask: AppDiracAsk | undefined): DiracAsk | undefined {
	if (!ask) {
		return undefined
	}

	const mapping: Record<AppDiracAsk, DiracAsk> = {
		followup: DiracAsk.FOLLOWUP,
		plan_mode_respond: DiracAsk.PLAN_MODE_RESPOND,
		act_mode_respond: DiracAsk.ACT_MODE_RESPOND,
		command: DiracAsk.COMMAND,
		command_output: DiracAsk.COMMAND_OUTPUT,
		completion_result: DiracAsk.COMPLETION_RESULT,
		tool: DiracAsk.TOOL,
		api_req_failed: DiracAsk.API_REQ_FAILED,
		resume_task: DiracAsk.RESUME_TASK,
		resume_completed_task: DiracAsk.RESUME_COMPLETED_TASK,
		storage: DiracAsk.STORAGE,
		mistake_limit_reached: DiracAsk.MISTAKE_LIMIT_REACHED,
		browser_action_launch: DiracAsk.BROWSER_ACTION_LAUNCH,
		new_task: DiracAsk.NEW_TASK,
		condense: DiracAsk.CONDENSE,
		summarize_task: DiracAsk.SUMMARIZE_TASK,
		report_bug: DiracAsk.REPORT_BUG,
		use_subagents: DiracAsk.USE_SUBAGENTS,
	}

	const result = mapping[ask]
	if (result === undefined) {
	}
	return result
}

// Helper function to convert DiracAsk enum to string
function convertProtoEnumToDiracAsk(ask: DiracAsk): AppDiracAsk | undefined {
	if (ask === DiracAsk.UNRECOGNIZED) {
		return undefined
	}

	const mapping: Record<Exclude<DiracAsk, DiracAsk.UNRECOGNIZED>, AppDiracAsk> = {
		[DiracAsk.FOLLOWUP]: "followup",
		[DiracAsk.PLAN_MODE_RESPOND]: "plan_mode_respond",
		[DiracAsk.ACT_MODE_RESPOND]: "act_mode_respond",
		[DiracAsk.COMMAND]: "command",
		[DiracAsk.COMMAND_OUTPUT]: "command_output",
		[DiracAsk.COMPLETION_RESULT]: "completion_result",
		[DiracAsk.TOOL]: "tool",
		[DiracAsk.API_REQ_FAILED]: "api_req_failed",
		[DiracAsk.RESUME_TASK]: "resume_task",
		[DiracAsk.RESUME_COMPLETED_TASK]: "resume_completed_task",
		[DiracAsk.MISTAKE_LIMIT_REACHED]: "mistake_limit_reached",
		[DiracAsk.BROWSER_ACTION_LAUNCH]: "browser_action_launch",
		[DiracAsk.NEW_TASK]: "new_task",
		[DiracAsk.CONDENSE]: "condense",
		[DiracAsk.SUMMARIZE_TASK]: "summarize_task",
		[DiracAsk.REPORT_BUG]: "report_bug",
		[DiracAsk.USE_SUBAGENTS]: "use_subagents",
		[DiracAsk.STORAGE]: "storage",
	}

	return mapping[ask]
}

// Helper function to convert DiracSay string to enum
function convertDiracSayToProtoEnum(say: AppDiracSay | undefined): DiracSay | undefined {
	if (!say) {
		return undefined
	}

	const mapping: Record<AppDiracSay, DiracSay> = {
		task: DiracSay.TASK,
		error: DiracSay.ERROR,
		api_req_started: DiracSay.API_REQ_STARTED,
		api_req_finished: DiracSay.API_REQ_FINISHED,
		text: DiracSay.TEXT,
		reasoning: DiracSay.REASONING,
		completion_result: DiracSay.COMPLETION_RESULT_SAY,
		user_feedback: DiracSay.USER_FEEDBACK,
		user_feedback_diff: DiracSay.USER_FEEDBACK_DIFF,
		api_req_retried: DiracSay.API_REQ_RETRIED,
		command: DiracSay.COMMAND_SAY,
		command_output: DiracSay.COMMAND_OUTPUT_SAY,
		tool: DiracSay.TOOL_SAY,
		shell_integration_warning: DiracSay.SHELL_INTEGRATION_WARNING,
		shell_integration_warning_with_suggestion: DiracSay.SHELL_INTEGRATION_WARNING,
		browser_action_launch: DiracSay.BROWSER_ACTION_LAUNCH_SAY,
		browser_action: DiracSay.BROWSER_ACTION,
		browser_action_result: DiracSay.BROWSER_ACTION_RESULT,
		diff_error: DiracSay.DIFF_ERROR,
		deleted_api_reqs: DiracSay.DELETED_API_REQS,
		diracignore_error: DiracSay.DIRACIGNORE_ERROR,
		command_permission_denied: DiracSay.COMMAND_PERMISSION_DENIED,
		info: DiracSay.INFO,
		error_retry: DiracSay.ERROR_RETRY,
		hook_status: DiracSay.HOOK_STATUS,
		hook_output_stream: DiracSay.HOOK_OUTPUT_STREAM,
		conditional_rules_applied: DiracSay.CONDITIONAL_RULES_APPLIED,
		subagent: DiracSay.SUBAGENT_STATUS,
		use_subagents: DiracSay.USE_SUBAGENTS_SAY,
		subagent_usage: DiracSay.SUBAGENT_USAGE,
	}

	const result = mapping[say]

	return result
}

// Helper function to convert DiracSay enum to string
function convertProtoEnumToDiracSay(say: DiracSay): AppDiracSay | undefined {
	if (say === DiracSay.UNRECOGNIZED) {
		return undefined
	}

	const mapping: Record<Exclude<DiracSay, DiracSay.UNRECOGNIZED>, AppDiracSay> = {
		[DiracSay.TASK]: "task",
		[DiracSay.ERROR]: "error",
		[DiracSay.API_REQ_STARTED]: "api_req_started",
		[DiracSay.API_REQ_FINISHED]: "api_req_finished",
		[DiracSay.TEXT]: "text",
		[DiracSay.REASONING]: "reasoning",
		[DiracSay.COMPLETION_RESULT_SAY]: "completion_result",
		[DiracSay.USER_FEEDBACK]: "user_feedback",
		[DiracSay.USER_FEEDBACK_DIFF]: "user_feedback_diff",
		[DiracSay.API_REQ_RETRIED]: "api_req_retried",
		[DiracSay.COMMAND_SAY]: "command",
		[DiracSay.COMMAND_OUTPUT_SAY]: "command_output",
		[DiracSay.TOOL_SAY]: "tool",
		[DiracSay.SHELL_INTEGRATION_WARNING]: "shell_integration_warning",
		[DiracSay.BROWSER_ACTION_LAUNCH_SAY]: "browser_action_launch",
		[DiracSay.BROWSER_ACTION]: "browser_action",
		[DiracSay.BROWSER_ACTION_RESULT]: "browser_action_result",
		[DiracSay.DIFF_ERROR]: "diff_error",
		[DiracSay.DELETED_API_REQS]: "deleted_api_reqs",
		[DiracSay.DIRACIGNORE_ERROR]: "diracignore_error",
		[DiracSay.COMMAND_PERMISSION_DENIED]: "command_permission_denied",
		[DiracSay.INFO]: "info",
		[DiracSay.ERROR_RETRY]: "error_retry",
		[DiracSay.HOOK_STATUS]: "hook_status",
		[DiracSay.HOOK_OUTPUT_STREAM]: "hook_output_stream",
		[DiracSay.CONDITIONAL_RULES_APPLIED]: "conditional_rules_applied",
		[DiracSay.SUBAGENT_STATUS]: "subagent",
		[DiracSay.USE_SUBAGENTS_SAY]: "use_subagents",
		[DiracSay.SUBAGENT_USAGE]: "subagent_usage",
	}

	return mapping[say]
}

/**
 * Convert application DiracMessage to proto DiracMessage
 */
export function convertDiracMessageToProto(message: AppDiracMessage): ProtoDiracMessage {
	// For sending messages, we need to provide values for required proto fields
	const askEnum = message.ask ? convertDiracAskToProtoEnum(message.ask) : undefined
	const sayEnum = message.say ? convertDiracSayToProtoEnum(message.say) : undefined

	// Determine appropriate enum values based on message type
	let finalAskEnum: DiracAsk = DiracAsk.FOLLOWUP // Proto default
	let finalSayEnum: DiracSay = DiracSay.TEXT // Proto default

	if (message.type === "ask") {
		finalAskEnum = askEnum ?? DiracAsk.FOLLOWUP // Use FOLLOWUP as default for ask messages
	} else if (message.type === "say") {
		finalSayEnum = sayEnum ?? DiracSay.TEXT // Use TEXT as default for say messages
	}

	const protoMessage: ProtoDiracMessage = {
		ts: message.ts,
		type: message.type === "ask" ? DiracMessageType.ASK : DiracMessageType.SAY,
		ask: finalAskEnum,
		say: finalSayEnum,
		text: message.text ?? "",
		reasoning: message.reasoning ?? "",
		images: message.images ?? [],
		files: message.files ?? [],
		partial: message.partial ?? false,
		isOperationOutsideWorkspace: message.isOperationOutsideWorkspace ?? false,
		conversationHistoryIndex: message.conversationHistoryIndex ?? 0,
		conversationHistoryDeletedRange: message.conversationHistoryDeletedRange
			? {
					startIndex: message.conversationHistoryDeletedRange[0],
					endIndex: message.conversationHistoryDeletedRange[1],
				}
			: undefined,
		// Additional optional fields for specific ask/say types
		sayTool: undefined,
		sayBrowserAction: undefined,
		browserActionResult: undefined,
		planModeResponse: undefined,
		askQuestion: undefined,
		askNewTask: undefined,
		apiReqInfo: undefined,
		modelInfo: message.modelInfo ?? undefined,
		multiCommandState: message.multiCommandState
			? {
					commands: message.multiCommandState.commands.map((cmd) => ({
						command: cmd.command,
						status: cmd.status,
						output: cmd.output ?? undefined,
						exitCode: cmd.exitCode ?? undefined,
						signal: cmd.signal ?? undefined,
						requiresApproval: cmd.requiresApproval ?? undefined,
						wasAutoApproved: cmd.wasAutoApproved ?? undefined,
					})),
				}
			: undefined,

	}

	return protoMessage
}

/**
 * Convert proto DiracMessage to application DiracMessage
 */
export function convertProtoToDiracMessage(protoMessage: ProtoDiracMessage): AppDiracMessage {
	const message: AppDiracMessage = {
		ts: protoMessage.ts,
		type: protoMessage.type === DiracMessageType.ASK ? "ask" : "say",
	}

	// Convert ask enum to string
	if (protoMessage.type === DiracMessageType.ASK) {
		const ask = convertProtoEnumToDiracAsk(protoMessage.ask)
		if (ask !== undefined) {
			message.ask = ask
		}
	}

	// Convert say enum to string
	if (protoMessage.type === DiracMessageType.SAY) {
		const say = convertProtoEnumToDiracSay(protoMessage.say)
		if (say !== undefined) {
			message.say = say
		}
	}

	// Convert other fields - preserve empty strings as they may be intentional
	if (protoMessage.text !== "") {
		message.text = protoMessage.text
	}
	if (protoMessage.reasoning !== "") {
		message.reasoning = protoMessage.reasoning
	}
	if (protoMessage.images.length > 0) {
		message.images = protoMessage.images
	}
	if (protoMessage.files.length > 0) {
		message.files = protoMessage.files
	}
	if (protoMessage.partial) {
		message.partial = protoMessage.partial
	}
	if (protoMessage.isOperationOutsideWorkspace) {
		message.isOperationOutsideWorkspace = protoMessage.isOperationOutsideWorkspace
	}
	if (protoMessage.conversationHistoryIndex !== 0) {
		message.conversationHistoryIndex = protoMessage.conversationHistoryIndex
	}

	// Convert conversationHistoryDeletedRange from object to tuple
	if (protoMessage.conversationHistoryDeletedRange) {
		message.conversationHistoryDeletedRange = [
			protoMessage.conversationHistoryDeletedRange.startIndex,
			protoMessage.conversationHistoryDeletedRange.endIndex,
		]
	}

	if (protoMessage.multiCommandState) {
		message.multiCommandState = {
			commands: protoMessage.multiCommandState.commands.map((cmd: any) => ({
				command: cmd.command,
				status: cmd.status as any,
				output: cmd.output ?? undefined,
				exitCode: cmd.exitCode ?? undefined,
				signal: cmd.signal ?? undefined,
				requiresApproval: cmd.requiresApproval ?? undefined,
				wasAutoApproved: cmd.wasAutoApproved ?? undefined,
			})),
		}
	}


	return message
}
