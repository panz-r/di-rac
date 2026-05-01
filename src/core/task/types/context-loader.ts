import { ApiProviderInfo } from "../../../core/api"
import { WorkspaceRootManager } from "../../../core/workspace/WorkspaceRootManager"
import { UrlContentFetcher } from "../../../services/browser/UrlContentFetcher"
import { FileContextTracker } from "../../context/context-tracking/FileContextTracker"
import { Controller } from "../../controller"
import { DiracIgnoreController } from "../../ignore/DiracIgnoreController"
import { StateManager } from "../../storage/StateManager"
import { TaskState } from "../TaskState"
import type { AnalyzerClient } from "@/services/tree-sitter/AnalyzerClient"

export interface ContextLoaderDependencies {
	ulid: string
	stateManager: StateManager
	controller: Controller
	cwd: string
	urlContentFetcher: UrlContentFetcher
	fileContextTracker: FileContextTracker
	workspaceManager?: WorkspaceRootManager
	diracIgnoreController: DiracIgnoreController
	analyzer?: AnalyzerClient
	taskState: TaskState
	getCurrentProviderInfo: () => ApiProviderInfo
	getEnvironmentDetails: (includeFileDetails?: boolean) => Promise<string>
}
