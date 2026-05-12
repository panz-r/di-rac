// Replaces proto-generated dirac/file types with plain TypeScript.

import type { Metadata } from "./common"

// --- Enums ---

export enum FileSearchType {
	FILE = 0,
	FOLDER = 1,
}

export enum RuleScope {
	LOCAL = 0,
	GLOBAL = 1,
	REMOTE = 2,
}

// --- Types ---

export interface DiracRulesToggles {
	toggles: Record<string, boolean>
}
export const DiracRulesToggles = {
	create(o: Partial<DiracRulesToggles> = {}): DiracRulesToggles {
		return { toggles: o.toggles ?? {} }
	},
}

export interface RefreshedRules {
	globalDiracRulesToggles: DiracRulesToggles
	localDiracRulesToggles: DiracRulesToggles
	localCursorRulesToggles: DiracRulesToggles
	localWindsurfRulesToggles: DiracRulesToggles
	localAgentsRulesToggles: DiracRulesToggles
	localWorkflowToggles: DiracRulesToggles
	globalWorkflowToggles: DiracRulesToggles
}
export const RefreshedRules = {
	create(o: Partial<RefreshedRules> = {}): RefreshedRules {
		return {
			globalDiracRulesToggles: o.globalDiracRulesToggles ?? DiracRulesToggles.create(),
			localDiracRulesToggles: o.localDiracRulesToggles ?? DiracRulesToggles.create(),
			localCursorRulesToggles: o.localCursorRulesToggles ?? DiracRulesToggles.create(),
			localWindsurfRulesToggles: o.localWindsurfRulesToggles ?? DiracRulesToggles.create(),
			localAgentsRulesToggles: o.localAgentsRulesToggles ?? DiracRulesToggles.create(),
			localWorkflowToggles: o.localWorkflowToggles ?? DiracRulesToggles.create(),
			globalWorkflowToggles: o.globalWorkflowToggles ?? DiracRulesToggles.create(),
		}
	},
}

export interface ToggleWindsurfRuleRequest {
	metadata: Metadata
	rulePath: string
	enabled: boolean
}
export const ToggleWindsurfRuleRequest = {
	create(o: Partial<ToggleWindsurfRuleRequest> = {}): ToggleWindsurfRuleRequest {
		return { metadata: o.metadata ?? {}, rulePath: o.rulePath ?? "", enabled: o.enabled ?? false }
	},
}

export interface ToggleAgentsRuleRequest {
	metadata: Metadata
	rulePath: string
	enabled: boolean
}
export const ToggleAgentsRuleRequest = {
	create(o: Partial<ToggleAgentsRuleRequest> = {}): ToggleAgentsRuleRequest {
		return { metadata: o.metadata ?? {}, rulePath: o.rulePath ?? "", enabled: o.enabled ?? false }
	},
}

export interface RelativePathsRequest {
	metadata: Metadata
	uris: string[]
}
export const RelativePathsRequest = {
	create(o: Partial<RelativePathsRequest> = {}): RelativePathsRequest {
		return { metadata: o.metadata ?? {}, uris: o.uris ?? [] }
	},
}

export interface RelativePaths {
	paths: string[]
}
export const RelativePaths = {
	create(o: Partial<RelativePaths> = {}): RelativePaths {
		return { paths: o.paths ?? [] }
	},
}

export interface FileSearchRequest {
	metadata: Metadata
	query: string
	mentionsRequestId?: string
	limit?: number
	selectedType?: FileSearchType
	workspaceHint?: string
}
export const FileSearchRequest = {
	create(o: Partial<FileSearchRequest> = {}): FileSearchRequest {
		return { metadata: o.metadata ?? {}, query: o.query ?? "", ...o }
	},
}

export interface FileInfo {
	path: string
	type: string
	label?: string
	workspaceName?: string
}
export const FileInfo = {
	create(o: Partial<FileInfo> = {}): FileInfo {
		return { path: o.path ?? "", type: o.type ?? "", ...o }
	},
}

export interface FileSearchResults {
	results: FileInfo[]
	mentionsRequestId?: string
}
export const FileSearchResults = {
	create(o: Partial<FileSearchResults> = {}): FileSearchResults {
		return { results: o.results ?? [], ...o }
	},
}

export interface GitCommit {
	hash: string
	shortHash: string
	subject: string
	author: string
	date: string
}
export const GitCommit = {
	create(o: Partial<GitCommit> = {}): GitCommit {
		return {
			hash: o.hash ?? "",
			shortHash: o.shortHash ?? "",
			subject: o.subject ?? "",
			author: o.author ?? "",
			date: o.date ?? "",
		}
	},
}

export interface GitCommits {
	commits: GitCommit[]
}
export const GitCommits = {
	create(o: Partial<GitCommits> = {}): GitCommits {
		return { commits: o.commits ?? [] }
	},
}

export interface RuleFileRequest {
	metadata: Metadata
	isGlobal: boolean
	rulePath?: string
	filename?: string
	type?: string
}
export const RuleFileRequest = {
	create(o: Partial<RuleFileRequest> = {}): RuleFileRequest {
		return { metadata: o.metadata ?? {}, isGlobal: o.isGlobal ?? false, ...o }
	},
}

export interface RuleFile {
	filePath: string
	displayName: string
	alreadyExists: boolean
}
export const RuleFile = {
	create(o: Partial<RuleFile> = {}): RuleFile {
		return { filePath: o.filePath ?? "", displayName: o.displayName ?? "", alreadyExists: o.alreadyExists ?? false }
	},
}

export interface ToggleDiracRuleRequest {
	metadata: Metadata
	scope: RuleScope
	rulePath: string
	enabled: boolean
}
export const ToggleDiracRuleRequest = {
	create(o: Partial<ToggleDiracRuleRequest> = {}): ToggleDiracRuleRequest {
		return { metadata: o.metadata ?? {}, scope: o.scope ?? RuleScope.LOCAL, rulePath: o.rulePath ?? "", enabled: o.enabled ?? false }
	},
}

export interface ToggleDiracRules {
	globalDiracRulesToggles: DiracRulesToggles
	localDiracRulesToggles: DiracRulesToggles
	remoteRulesToggles: DiracRulesToggles
}
export const ToggleDiracRules = {
	create(o: Partial<ToggleDiracRules> = {}): ToggleDiracRules {
		return {
			globalDiracRulesToggles: o.globalDiracRulesToggles ?? DiracRulesToggles.create(),
			localDiracRulesToggles: o.localDiracRulesToggles ?? DiracRulesToggles.create(),
			remoteRulesToggles: o.remoteRulesToggles ?? DiracRulesToggles.create(),
		}
	},
}

export interface ToggleCursorRuleRequest {
	metadata: Metadata
	rulePath: string
	enabled: boolean
}
export const ToggleCursorRuleRequest = {
	create(o: Partial<ToggleCursorRuleRequest> = {}): ToggleCursorRuleRequest {
		return { metadata: o.metadata ?? {}, rulePath: o.rulePath ?? "", enabled: o.enabled ?? false }
	},
}

export interface ToggleWorkflowRequest {
	metadata: Metadata
	workflowPath: string
	enabled: boolean
	scope: RuleScope
}
export const ToggleWorkflowRequest = {
	create(o: Partial<ToggleWorkflowRequest> = {}): ToggleWorkflowRequest {
		return { metadata: o.metadata ?? {}, workflowPath: o.workflowPath ?? "", enabled: o.enabled ?? false, scope: o.scope ?? RuleScope.LOCAL }
	},
}

export interface HookInfo {
	name: string
	enabled: boolean
	absolutePath: string
}
export const HookInfo = {
	create(o: Partial<HookInfo> = {}): HookInfo {
		return { name: o.name ?? "", enabled: o.enabled ?? false, absolutePath: o.absolutePath ?? "" }
	},
}

export interface WorkspaceHooks {
	workspaceName: string
	hooks: HookInfo[]
}
export const WorkspaceHooks = {
	create(o: Partial<WorkspaceHooks> = {}): WorkspaceHooks {
		return { workspaceName: o.workspaceName ?? "", hooks: o.hooks ?? [] }
	},
}

export interface HooksToggles {
	globalHooks: HookInfo[]
	workspaceHooks: WorkspaceHooks[]
	isWindows: boolean
}
export const HooksToggles = {
	create(o: Partial<HooksToggles> = {}): HooksToggles {
		return { globalHooks: o.globalHooks ?? [], workspaceHooks: o.workspaceHooks ?? [], isWindows: o.isWindows ?? false }
	},
}

export interface ToggleHookRequest {
	metadata: Metadata
	hookName: string
	isGlobal: boolean
	enabled: boolean
	workspaceName?: string
}
export const ToggleHookRequest = {
	create(o: Partial<ToggleHookRequest> = {}): ToggleHookRequest {
		return { metadata: o.metadata ?? {}, hookName: o.hookName ?? "", isGlobal: o.isGlobal ?? false, enabled: o.enabled ?? false, ...o }
	},
}

export interface ToggleHookResponse {
	hooksToggles: HooksToggles
}
export const ToggleHookResponse = {
	create(o: Partial<ToggleHookResponse> = {}): ToggleHookResponse {
		return { hooksToggles: o.hooksToggles ?? HooksToggles.create() }
	},
}

export interface CreateHookRequest {
	metadata: Metadata
	hookName: string
	isGlobal: boolean
	workspaceName?: string
}
export const CreateHookRequest = {
	create(o: Partial<CreateHookRequest> = {}): CreateHookRequest {
		return { metadata: o.metadata ?? {}, hookName: o.hookName ?? "", isGlobal: o.isGlobal ?? false, ...o }
	},
}

export interface CreateHookResponse {
	hooksToggles: HooksToggles
}
export const CreateHookResponse = {
	create(o: Partial<CreateHookResponse> = {}): CreateHookResponse {
		return { hooksToggles: o.hooksToggles ?? HooksToggles.create() }
	},
}

export interface DeleteHookRequest {
	metadata: Metadata
	hookName: string
	isGlobal: boolean
	workspaceName?: string
}
export const DeleteHookRequest = {
	create(o: Partial<DeleteHookRequest> = {}): DeleteHookRequest {
		return { metadata: o.metadata ?? {}, hookName: o.hookName ?? "", isGlobal: o.isGlobal ?? false, ...o }
	},
}

export interface DeleteHookResponse {
	hooksToggles: HooksToggles
}
export const DeleteHookResponse = {
	create(o: Partial<DeleteHookResponse> = {}): DeleteHookResponse {
		return { hooksToggles: o.hooksToggles ?? HooksToggles.create() }
	},
}

export interface SkillInfo {
	name: string
	description: string
	path: string
	enabled: boolean
}
export const SkillInfo = {
	create(o: Partial<SkillInfo> = {}): SkillInfo {
		return { name: o.name ?? "", description: o.description ?? "", path: o.path ?? "", enabled: o.enabled ?? false }
	},
}

export interface RefreshedSkills {
	globalSkills: SkillInfo[]
	localSkills: SkillInfo[]
}
export const RefreshedSkills = {
	create(o: Partial<RefreshedSkills> = {}): RefreshedSkills {
		return { globalSkills: o.globalSkills ?? [], localSkills: o.localSkills ?? [] }
	},
}

export interface SkillsToggles {
	globalSkillsToggles: Record<string, boolean>
	localSkillsToggles: Record<string, boolean>
}
export const SkillsToggles = {
	create(o: Partial<SkillsToggles> = {}): SkillsToggles {
		return { globalSkillsToggles: o.globalSkillsToggles ?? {}, localSkillsToggles: o.localSkillsToggles ?? {} }
	},
}

export interface ToggleSkillRequest {
	metadata: Metadata
	skillPath: string
	isGlobal: boolean
	enabled: boolean
}
export const ToggleSkillRequest = {
	create(o: Partial<ToggleSkillRequest> = {}): ToggleSkillRequest {
		return { metadata: o.metadata ?? {}, skillPath: o.skillPath ?? "", isGlobal: o.isGlobal ?? false, enabled: o.enabled ?? false }
	},
}

export interface CreateSkillRequest {
	metadata: Metadata
	skillName: string
	isGlobal: boolean
}
export const CreateSkillRequest = {
	create(o: Partial<CreateSkillRequest> = {}): CreateSkillRequest {
		return { metadata: o.metadata ?? {}, skillName: o.skillName ?? "", isGlobal: o.isGlobal ?? false }
	},
}

export interface DeleteSkillRequest {
	metadata: Metadata
	skillPath: string
	isGlobal: boolean
}
export const DeleteSkillRequest = {
	create(o: Partial<DeleteSkillRequest> = {}): DeleteSkillRequest {
		return { metadata: o.metadata ?? {}, skillPath: o.skillPath ?? "", isGlobal: o.isGlobal ?? false }
	},
}
