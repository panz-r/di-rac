import { resolveWorkspacePath } from "@core/workspace"
import { isMultiRootEnabled } from "@core/workspace/multi-root-utils"
import { DiracDefaultTool } from "@shared/tools"
import { StateManager } from "@/core/storage/StateManager"
import { HostProvider } from "@/hosts/host-provider"
import { getCwd, getDesktopDir, isLocatedInPath, isLocatedInWorkspace } from "@/utils/path"


const WRITE_TOOLS: DiracDefaultTool[] = [
	DiracDefaultTool.FILE_NEW,
	DiracDefaultTool.EDIT_FILE,
	DiracDefaultTool.REPLACE_SYMBOL,
	DiracDefaultTool.RENAME_SYMBOL,
	DiracDefaultTool.NEW_RULE,
]

export class AutoApprove {
	private stateManager: StateManager
	// Cache for workspace paths - populated on first access and reused for the task lifetime
	// NOTE: This assumes that the task has a fixed set of workspace roots(which is currently true).
	private workspacePathsCache: { paths: string[] } | null = null
	private isMultiRootScenarioCache: boolean | null = null

	constructor(stateManager: StateManager) {
		this.stateManager = stateManager
	}

	/**
	 * Get workspace information with caching to avoid repeated API calls
	 * Cache is task-scoped since each task gets a new AutoApprove instance
	 */
	private async getWorkspaceInfo(): Promise<{
		workspacePaths: { paths: string[] }
		isMultiRootScenario: boolean
	}> {
		// Check if we already have cached values
		if (this.workspacePathsCache === null || this.isMultiRootScenarioCache === null) {
			// First time - fetch and cache for the lifetime of this task
			const result = await HostProvider.workspace.getWorkspacePaths({})
			this.workspacePathsCache = result
			this.isMultiRootScenarioCache = isMultiRootEnabled(this.stateManager) && result.paths.length > 1
		}

		return {
			workspacePaths: this.workspacePathsCache!,
			isMultiRootScenario: this.isMultiRootScenarioCache!,
		}
	}

		// Check if the tool should be auto-approved based on the settings
	// Returns bool for most tools, and tuple for tools with nested settings
	shouldAutoApproveTool(toolName: DiracDefaultTool): boolean | [boolean, boolean] {
		if (this.stateManager.getGlobalSettingsKey("yoloModeToggled")) {
			switch (toolName) {
				case DiracDefaultTool.FILE_READ:
				case DiracDefaultTool.LIST_FILES:
				case DiracDefaultTool.SEARCH:
				case DiracDefaultTool.NEW_RULE:
				case DiracDefaultTool.FILE_NEW:
				case DiracDefaultTool.EDIT_FILE:
				case DiracDefaultTool.USE_SUBAGENTS:
				case DiracDefaultTool.USE_SKILL:
					return [true, true]

				case DiracDefaultTool.BASH:
				case DiracDefaultTool.BROWSER:
				case DiracDefaultTool.WEB_FETCH:
				case DiracDefaultTool.WEB_SEARCH:
					return true
			}
		}

		if (this.stateManager.getGlobalSettingsKey("autoApproveAllToggled")) {
			switch (toolName) {
				case DiracDefaultTool.FILE_READ:
				case DiracDefaultTool.LIST_FILES:
				case DiracDefaultTool.SEARCH:
				case DiracDefaultTool.NEW_RULE:
				case DiracDefaultTool.FILE_NEW:
				case DiracDefaultTool.EDIT_FILE:
				case DiracDefaultTool.USE_SUBAGENTS:
				case DiracDefaultTool.USE_SKILL:
					return [true, true]

				case DiracDefaultTool.BASH:
				case DiracDefaultTool.BROWSER:
				case DiracDefaultTool.WEB_FETCH:
				case DiracDefaultTool.WEB_SEARCH:
					return true
			}
		}

		const autoApprovalSettings = this.stateManager.getGlobalSettingsKey("autoApprovalSettings")

		switch (toolName) {
			case DiracDefaultTool.FILE_READ:
			case DiracDefaultTool.LIST_FILES:
			case DiracDefaultTool.SEARCH:
			case DiracDefaultTool.USE_SUBAGENTS:
			case DiracDefaultTool.USE_SKILL:
				return [autoApprovalSettings.actions.readFiles, autoApprovalSettings.actions.readFilesExternally ?? false]

			case DiracDefaultTool.NEW_RULE:
			case DiracDefaultTool.FILE_NEW:
			case DiracDefaultTool.EDIT_FILE:
				return [autoApprovalSettings.actions.editFiles, autoApprovalSettings.actions.editFilesExternally ?? false]

			case DiracDefaultTool.BASH:
				return autoApprovalSettings.actions.executeCommands ?? false
			case DiracDefaultTool.BROWSER:
				return autoApprovalSettings.actions.useBrowser
			case DiracDefaultTool.WEB_FETCH:
			case DiracDefaultTool.WEB_SEARCH:
				return autoApprovalSettings.actions.useBrowser
		}
		return false
	}

	// Check if the tool should be auto-approved based on the settings
	// and the path of the action. Returns true if the tool should be auto-approved
	// based on the user's settings and the path of the action.
	async shouldAutoApproveToolWithPath(
		blockname: DiracDefaultTool,
		autoApproveActionpath: string | undefined,
	): Promise<boolean> {
		// 1. Determine if the action is local or external FIRST
		let isLocalRead = false
		if (autoApproveActionpath) {
			// Use cached workspace info instead of fetching every time
			const { isMultiRootScenario } = await this.getWorkspaceInfo()

			if (isMultiRootScenario) {
				// Multi-root: check if file is in ANY workspace
				isLocalRead = await isLocatedInWorkspace(autoApproveActionpath)
			} else {
				// Single-root: use existing logic
				const cwd = await getCwd(getDesktopDir())
				try {
					// When called with a string cwd, resolveWorkspacePath returns a string
					const absolutePath = resolveWorkspacePath(
						cwd,
						autoApproveActionpath,
						"AutoApprove.shouldAutoApproveToolWithPath",
					) as string
					isLocalRead = isLocatedInPath(cwd, absolutePath)
				} catch (e) {
					// Catch PathEscapeError and return false. 
					// This prevents partial absolute path fragments during streaming 
					// from aborting the entire tool call.
					isLocalRead = false
				}
			}
		} else {
			// If we do not get a path for some reason, default to a (safer) false return
			isLocalRead = false
		}

		// 2. SAFETY POLICY: Always require manual approval for writes outside the workspace, even in YOLO mode
		const isWriteOperation = WRITE_TOOLS.includes(blockname)
		if (!isLocalRead && isWriteOperation) {
			return false
		}

		if (this.stateManager.getGlobalSettingsKey("yoloModeToggled")) {
			return true
		}
		if (this.stateManager.getGlobalSettingsKey("autoApproveAllToggled")) {
			return true
		}
		// Get auto-approve settings for local and external edits
		const autoApproveResult = this.shouldAutoApproveTool(blockname)
		const [autoApproveLocal, autoApproveExternal] = Array.isArray(autoApproveResult)
			? autoApproveResult
			: [autoApproveResult, false]

		if ((isLocalRead && autoApproveLocal) || (!isLocalRead && autoApproveLocal && autoApproveExternal)) {
			return true
		}
		return false
	}
}
