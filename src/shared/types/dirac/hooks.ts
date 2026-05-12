// Replaces proto-generated dirac/hooks types with plain TypeScript.

export interface HookModelContext {
	provider: string
	slug: string
}
export const HookModelContext = {
	create(o: Partial<HookModelContext> = {}): HookModelContext {
		return {
			provider: o.provider ?? "",
			slug: o.slug ?? "",
		}
	},
}

export interface PreToolUseData {
	toolName: string
	parameters: Record<string, string>
}
export const PreToolUseData = {
	create(o: Partial<PreToolUseData> = {}): PreToolUseData {
		return {
			toolName: o.toolName ?? "",
			parameters: o.parameters ?? {},
		}
	},
}

export interface PostToolUseData {
	toolName: string
	parameters: Record<string, string>
	result: string
	success: boolean
	executionTimeMs: number
}
export const PostToolUseData = {
	create(o: Partial<PostToolUseData> = {}): PostToolUseData {
		return {
			toolName: o.toolName ?? "",
			parameters: o.parameters ?? {},
			result: o.result ?? "",
			success: o.success ?? false,
			executionTimeMs: o.executionTimeMs ?? 0,
		}
	},
}

export interface UserPromptSubmitData {
	prompt: string
	attachments: string[]
}
export const UserPromptSubmitData = {
	create(o: Partial<UserPromptSubmitData> = {}): UserPromptSubmitData {
		return {
			prompt: o.prompt ?? "",
			attachments: o.attachments ?? [],
		}
	},
}

export interface NotificationData {
	event: string
	source: string
	message: string
	waitingForUserInput: boolean
}
export const NotificationData = {
	create(o: Partial<NotificationData> = {}): NotificationData {
		return {
			event: o.event ?? "",
			source: o.source ?? "",
			message: o.message ?? "",
			waitingForUserInput: o.waitingForUserInput ?? false,
		}
	},
}

export interface TaskStartData {
	taskMetadata: Record<string, string>
}
export const TaskStartData = {
	create(o: Partial<TaskStartData> = {}): TaskStartData {
		return { taskMetadata: o.taskMetadata ?? {} }
	},
}

export interface TaskResumeData {
	taskMetadata: Record<string, string>
	previousState: Record<string, string>
}
export const TaskResumeData = {
	create(o: Partial<TaskResumeData> = {}): TaskResumeData {
		return {
			taskMetadata: o.taskMetadata ?? {},
			previousState: o.previousState ?? {},
		}
	},
}

export interface TaskCancelData {
	taskMetadata: Record<string, string>
}
export const TaskCancelData = {
	create(o: Partial<TaskCancelData> = {}): TaskCancelData {
		return { taskMetadata: o.taskMetadata ?? {} }
	},
}

export interface TaskCompleteData {
	taskMetadata: Record<string, string>
}
export const TaskCompleteData = {
	create(o: Partial<TaskCompleteData> = {}): TaskCompleteData {
		return { taskMetadata: o.taskMetadata ?? {} }
	},
}

export interface PreCompactData {
	taskId: string
	ulid: string
	contextSize: number
	compactionStrategy: string
	previousApiReqIndex: number
	tokensIn: number
	tokensOut: number
	tokensInCache: number
	tokensOutCache: number
	deletedRangeStart: number
	deletedRangeEnd: number
	contextJsonPath: string
	contextRawPath: string
}
export const PreCompactData = {
	create(o: Partial<PreCompactData> = {}): PreCompactData {
		return {
			taskId: o.taskId ?? "",
			ulid: o.ulid ?? "",
			contextSize: o.contextSize ?? 0,
			compactionStrategy: o.compactionStrategy ?? "",
			previousApiReqIndex: o.previousApiReqIndex ?? 0,
			tokensIn: o.tokensIn ?? 0,
			tokensOut: o.tokensOut ?? 0,
			tokensInCache: o.tokensInCache ?? 0,
			tokensOutCache: o.tokensOutCache ?? 0,
			deletedRangeStart: o.deletedRangeStart ?? 0,
			deletedRangeEnd: o.deletedRangeEnd ?? 0,
			contextJsonPath: o.contextJsonPath ?? "",
			contextRawPath: o.contextRawPath ?? "",
		}
	},
}

export interface HookInput {
	diracVersion: string
	hookName: string
	timestamp: string
	taskId: string
	workspaceRoots: string[]
	userId: string
	model: HookModelContext
	preToolUse?: PreToolUseData
	postToolUse?: PostToolUseData
	userPromptSubmit?: UserPromptSubmitData
	taskStart?: TaskStartData
	taskResume?: TaskResumeData
	taskCancel?: TaskCancelData
	taskComplete?: TaskCompleteData
	preCompact?: PreCompactData
	notification?: NotificationData
}
export const HookInput = {
	create(o: Partial<HookInput> = {}): HookInput {
		return {
			diracVersion: o.diracVersion ?? "",
			hookName: o.hookName ?? "",
			timestamp: o.timestamp ?? "",
			taskId: o.taskId ?? "",
			workspaceRoots: o.workspaceRoots ?? [],
			userId: o.userId ?? "",
			model: o.model ?? HookModelContext.create(),
			preToolUse: o.preToolUse,
			postToolUse: o.postToolUse,
			userPromptSubmit: o.userPromptSubmit,
			taskStart: o.taskStart,
			taskResume: o.taskResume,
			taskCancel: o.taskCancel,
			taskComplete: o.taskComplete,
			preCompact: o.preCompact,
			notification: o.notification,
		}
	},
}

export interface HookOutput {
	contextModification: string
	cancel: boolean
	errorMessage: string
}
export const HookOutput = {
	create(o: Partial<HookOutput> = {}): HookOutput {
		return {
			contextModification: o.contextModification ?? "",
			cancel: o.cancel ?? false,
			errorMessage: o.errorMessage ?? "",
		}
	},
}
