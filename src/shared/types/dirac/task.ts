// Replaces proto-generated dirac/task types with plain TypeScript.

import type { Metadata } from "./common"
import type { Empty } from "./common"
import type { StringRequest } from "./common"

export interface TaskResponse {
	id: string
	task: string
	ts: number
	isFavorited: boolean
	size: number
	totalCost: number
	tokensIn: number
	tokensOut: number
	cacheWrites: number
	cacheReads: number
	modelId: string
}
export const TaskResponse = {
	create(o: Partial<TaskResponse> = {}): TaskResponse {
		return {
			id: o.id ?? "",
			task: o.task ?? "",
			ts: o.ts ?? 0,
			isFavorited: o.isFavorited ?? false,
			size: o.size ?? 0,
			totalCost: o.totalCost ?? 0,
			tokensIn: o.tokensIn ?? 0,
			tokensOut: o.tokensOut ?? 0,
			cacheWrites: o.cacheWrites ?? 0,
			cacheReads: o.cacheReads ?? 0,
			modelId: o.modelId ?? "",
		}
	},
}

export interface GetTaskHistoryRequest {
	metadata: Metadata | undefined
	favoritesOnly: boolean
	searchQuery: string
	sortBy: string
	currentWorkspaceOnly: boolean
}
export const GetTaskHistoryRequest = {
	create(o: Partial<GetTaskHistoryRequest> = {}): GetTaskHistoryRequest {
		return {
			metadata: undefined,
			favoritesOnly: o.favoritesOnly ?? false,
			searchQuery: o.searchQuery ?? "",
			sortBy: o.sortBy ?? "",
			currentWorkspaceOnly: o.currentWorkspaceOnly ?? false,
		}
	},
}

export interface TaskHistoryArray {
	tasks: TaskItem[]
	totalCount: number
}
export const TaskHistoryArray = {
	create(o: Partial<TaskHistoryArray> = {}): TaskHistoryArray {
		return {
			tasks: o.tasks ?? [],
			totalCount: o.totalCount ?? 0,
		}
	},
}

export interface TaskItem {
	id: string
	task: string
	ts: number
	isFavorited: boolean
	size: number
	totalCost: number
	tokensIn: number
	tokensOut: number
	cacheWrites: number
	cacheReads: number
	modelId: string
}
export const TaskItem = {
	create(o: Partial<TaskItem> = {}): TaskItem {
		return {
			id: o.id ?? "",
			task: o.task ?? "",
			ts: o.ts ?? 0,
			isFavorited: o.isFavorited ?? false,
			size: o.size ?? 0,
			totalCost: o.totalCost ?? 0,
			tokensIn: o.tokensIn ?? 0,
			tokensOut: o.tokensOut ?? 0,
			cacheWrites: o.cacheWrites ?? 0,
			cacheReads: o.cacheReads ?? 0,
			modelId: o.modelId ?? "",
		}
	},
}

export interface NewTaskRequest {
	metadata: Metadata | undefined
	text: string
	images: string[]
	files: string[]
	taskSettings?: any
}
export const NewTaskRequest = {
	create(o: Partial<NewTaskRequest> = {}): NewTaskRequest {
		return {
			metadata: undefined,
			text: o.text ?? "",
			images: o.images ?? [],
			files: o.files ?? [],
			taskSettings: o.taskSettings,
		}
	},
}

export interface TaskFavoriteRequest {
	metadata: Metadata | undefined
	taskId: string
	isFavorited: boolean
}
export const TaskFavoriteRequest = {
	create(o: Partial<TaskFavoriteRequest> = {}): TaskFavoriteRequest {
		return {
			metadata: undefined,
			taskId: o.taskId ?? "",
			isFavorited: o.isFavorited ?? false,
		}
	},
}

export interface AskResponseRequest {
	metadata: Metadata | undefined
	responseType: string
	text: string
	images: string[]
	files: string[]
}
export const AskResponseRequest = {
	create(o: Partial<AskResponseRequest> = {}): AskResponseRequest {
		return {
			metadata: undefined,
			responseType: o.responseType ?? "",
			text: o.text ?? "",
			images: o.images ?? [],
			files: o.files ?? [],
		}
	},
}

export interface ExecuteQuickWinRequest {
	metadata: Metadata | undefined
	command: string
	title: string
}
export const ExecuteQuickWinRequest = {
	create(o: Partial<ExecuteQuickWinRequest> = {}): ExecuteQuickWinRequest {
		return {
			metadata: undefined,
			command: o.command ?? "",
			title: o.title ?? "",
		}
	},
}

export interface DeleteAllTaskHistoryCount {
	tasksDeleted: number
}
export const DeleteAllTaskHistoryCount = {
	create(o: Partial<DeleteAllTaskHistoryCount> = {}): DeleteAllTaskHistoryCount {
		return { tasksDeleted: o.tasksDeleted ?? 0 }
	},
}
