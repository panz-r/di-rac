import { strict as assert } from "node:assert"
import fs from "node:fs/promises"
import os from "node:os"
import path from "node:path"
import { DiracDefaultTool } from "@shared/tools"
import { FileAnchorIndex } from "@shared/utils/file-anchor-index"
import * as pathUtils from "@utils/path"
import { afterEach, beforeEach, describe, it } from "mocha"
import sinon from "sinon"
import { TaskState } from "../../../TaskState"
import { ToolValidator } from "../../ToolValidator"
import type { TaskConfig } from "../../types/TaskConfig"
import { ListFilesToolHandler } from "../ListFilesToolHandler"
import { SearchFilesToolHandler } from "../SearchFilesToolHandler"

/**
 * End-to-end tests for path-based tool handlers' error recovery.
 *
 * These exercise the actual handlers with a mock TaskConfig (following the
 * SubagentToolHandler.test.ts pattern), verifying that:
 *
 *   1. Non-existent paths return a tool error (not a thrown exception)
 *   2. consecutiveMistakeCount increments on failure
 *   3. Repeated failures accumulate across calls
 *   4. A successful operation resets consecutiveMistakeCount to 0
 *   5. Missing parameters increment the counter
 *   6. Forced exceptions from core operations are caught gracefully
 */

let tmpDir: string

function createConfig() {
	const taskState = new TaskState()

	const callbacks = {
		say: sinon.stub().resolves(undefined),
		ask: sinon.stub().resolves({ response: "yesButtonClicked" }),
		saveCheckpoint: sinon.stub().resolves(),
		sayAndCreateMissingParamError: sinon.stub().resolves("missing"),
		removeLastPartialMessageIfExistsWithType: sinon.stub().resolves(),
		shouldAutoApproveToolWithPath: sinon.stub().resolves(true),
		postStateToWebview: sinon.stub().resolves(),
		cancelTask: sinon.stub().resolves(),
		updateTaskHistory: sinon.stub().resolves([]),
		switchToActMode: sinon.stub().resolves(false),
		setActiveHookExecution: sinon.stub().resolves(),
		clearActiveHookExecution: sinon.stub().resolves(),
		getActiveHookExecution: sinon.stub().resolves(undefined),
		runUserPromptSubmitHook: sinon.stub().resolves({}),
		executeCommandTool: sinon.stub().resolves([false, "ok"]),
		cancelRunningCommandTool: sinon.stub().resolves(false),
		doesLatestTaskCompletionHaveNewChanges: sinon.stub().resolves(false),
		updateFCListFromToolResponse: sinon.stub().resolves(),
		shouldAutoApproveTool: sinon.stub().returns([true, true]),
		reinitExistingTaskFromId: sinon.stub().resolves(),
		applyLatestBrowserSettings: sinon.stub().resolves(undefined),
	}

	const config = {
		taskId: "task-1",
		ulid: "ulid-1",
		cwd: tmpDir,
		mode: "act",
		strictPlanModeEnabled: false,
		yoloModeToggled: true,
		doubleCheckCompletionEnabled: false,
		vscodeTerminalExecutionMode: "backgroundExec",
		enableParallelToolCalling: true,
		isSubagentExecution: true, // skip UI calls and approval flow
		taskState,
		messageState: {
			getApiConversationHistory: sinon.stub().returns([]),
		},
		api: {
			getModel: () => ({ id: "test-model", info: { supportsImages: false } }),
		},
		autoApprovalSettings: {
			enableNotifications: false,
			actions: { executeCommands: false },
		},
		autoApprover: {
			shouldAutoApproveTool: sinon.stub().returns([true, true]),
		},
		browserSettings: {},
		focusChainSettings: {},
		services: {
			stateManager: {
				getGlobalStateKey: () => undefined,
				getGlobalSettingsKey: (key: string) => {
					if (key === "mode") return "act"
					if (key === "hooksEnabled") return false
					return undefined
				},
				getApiConfiguration: () => ({
					planModeApiProvider: "openai",
					actModeApiProvider: "openai",
				}),
			},
			fileContextTracker: {
				trackFileContext: sinon.stub().resolves(),
			},
			browserSession: {},
			urlContentFetcher: {},
			diffViewProvider: {},
			diracIgnoreController: { validateAccess: () => true, filterPaths: (paths: string[]) => paths },
			commandPermissionController: {},
			contextManager: {},
		},
		callbacks,
		coordinator: { getHandler: sinon.stub() },
	} as unknown as TaskConfig

	const validator = new ToolValidator({ validateAccess: () => true } as any)

	return { config, callbacks, taskState, validator }
}

// ─── ListFilesToolHandler ───────────────────────────────────────────────────

describe("ListFilesToolHandler.execute – error recovery", () => {
	let sandbox: sinon.SinonSandbox

	beforeEach(async () => {
		sandbox = sinon.createSandbox()
		tmpDir = await fs.mkdtemp(path.join(os.tmpdir(), "dirac-listfiles-test-"))
		sandbox.stub(pathUtils, "isLocatedInWorkspace").resolves(true)
		
	})

	afterEach(async () => {
		sandbox.restore()
		await fs.rm(tmpDir, { recursive: true, force: true }).catch(() => {})
	})

	function makeBlock(paths?: string | string[], recursive?: string) {
		const params: any = {}
		if (paths !== undefined) {
			if (Array.isArray(paths)) {
				params.paths = paths
			} else {
				params.paths = [paths]
			}
		}
		if (recursive !== undefined) params.recursive = recursive
		return {
			type: "tool_use" as const,
			name: DiracDefaultTool.LIST_FILES,
			params,
			partial: false,
		}
	}

	it("returns a tool result (not a thrown exception) for a non-existent directory", async () => {
		const { config, taskState, validator } = createConfig()
		const handler = new ListFilesToolHandler(validator)

		const result = await handler.execute(config, makeBlock("no-such-dir"))

		// listFiles returns empty for non-existent directories, so the handler
		// should succeed rather than throw.
		assert.equal(typeof result, "string")
		assert.equal(taskState.consecutiveMistakeCount, 0)
	})

	it("increments consecutiveMistakeCount when path parameter is missing", async () => {
		const { config, taskState, validator } = createConfig()
		const handler = new ListFilesToolHandler(validator)

		const result = await handler.execute(config, makeBlock())

		assert.equal(result, "missing")
		assert.equal(taskState.consecutiveMistakeCount, 1)
	})

	it("repeated missing-param failures accumulate", async () => {
		const { config, taskState, validator } = createConfig()
		const handler = new ListFilesToolHandler(validator)

		await handler.execute(config, makeBlock())
		await handler.execute(config, makeBlock())
		await handler.execute(config, makeBlock())
		assert.equal(taskState.consecutiveMistakeCount, 3)
	})

	it("resets consecutiveMistakeCount to 0 after a successful list", async () => {
		const { config, taskState, validator } = createConfig()
		const handler = new ListFilesToolHandler(validator)

		// Accumulate failures
		await handler.execute(config, makeBlock())
		await handler.execute(config, makeBlock())
		assert.equal(taskState.consecutiveMistakeCount, 2)

		// Create a real directory with a file
		const dirName = "real-dir"
		await fs.mkdir(path.join(tmpDir, dirName))
		await fs.writeFile(path.join(tmpDir, dirName, "file.txt"), "content")

		const result = await handler.execute(config, makeBlock(dirName))
		assert.equal(typeof result, "string")
		assert.ok((result as string).includes("file.txt"))
		assert.equal(taskState.consecutiveMistakeCount, 0)
	})

	it("catches a thrown exception from listFiles and returns a tool error", async () => {
		const { config, taskState, validator } = createConfig()
		const handler = new ListFilesToolHandler(validator)

		// Stub listFiles to throw
		const listFilesModule = await import("@services/glob/list-files")
		sandbox.stub(listFilesModule, "listFiles").rejects(new Error("cwd must be a directory"))

		const result = await handler.execute(config, makeBlock("some-dir"))

		assert.equal(typeof result, "string")
		assert.ok((result as string).includes("Error"))
		assert.ok((result as string).includes("cwd must be a directory"))
		assert.equal(taskState.consecutiveMistakeCount, 1)
	})

	it("accumulates failures when listFiles throws repeatedly", async () => {
		const { config, taskState, validator } = createConfig()
		const handler = new ListFilesToolHandler(validator)

		const listFilesModule = await import("@services/glob/list-files")
		sandbox.stub(listFilesModule, "listFiles").rejects(new Error("boom"))

		await handler.execute(config, makeBlock("dir-1"))
		assert.equal(taskState.consecutiveMistakeCount, 1)

		await handler.execute(config, makeBlock("dir-2"))
		assert.equal(taskState.consecutiveMistakeCount, 2)

		await handler.execute(config, makeBlock("dir-3"))
		assert.equal(taskState.consecutiveMistakeCount, 3)
	})

	it("increments consecutiveMistakeCount on diracignore denial", async () => {
		const { config, taskState } = createConfig()
		// Create a validator whose diracIgnoreController blocks all paths
		const blockingValidator = new ToolValidator({ validateAccess: () => false } as any)
		const handler = new ListFilesToolHandler(blockingValidator)

		const result = await handler.execute(config, makeBlock("blocked-dir"))

		assert.equal(typeof result, "string")
		assert.ok((result as string).includes("diracignore"))
		assert.equal(taskState.consecutiveMistakeCount, 1)
	})

	it("accumulates diracignore denials across repeated calls", async () => {
		const { config, taskState } = createConfig()
		const blockingValidator = new ToolValidator({ validateAccess: () => false } as any)
		const handler = new ListFilesToolHandler(blockingValidator)

		await handler.execute(config, makeBlock("blocked-1"))
		await handler.execute(config, makeBlock("blocked-2"))
		await handler.execute(config, makeBlock("blocked-3"))
		assert.equal(taskState.consecutiveMistakeCount, 3)
	})

	it("increments consecutiveMistakeCount only once when multiple paths are blocked", async () => {
		const { config, taskState } = createConfig()
		const blockingValidator = new ToolValidator({ validateAccess: () => false } as any)
		const handler = new ListFilesToolHandler(blockingValidator)

		const result = await handler.execute(config, makeBlock(["blocked-1", "blocked-2"]))

		assert.equal(typeof result, "string")
		assert.ok((result as string).includes("blocked-1"))
		assert.ok((result as string).includes("blocked-2"))
		assert.equal(taskState.consecutiveMistakeCount, 1)
	})

	it("resets consecutiveMistakeCount if at least one path succeeds in a multi-path call", async () => {
		const { config, taskState, validator } = createConfig()
		const handler = new ListFilesToolHandler(validator)

		// Mock diracIgnoreController to block one path but allow another
		const diracIgnoreController = {
			validateAccess: (p: string) => !p.includes("blocked"),
			filterPaths: (paths: string[]) => paths.filter((p) => !p.includes("blocked")),
		}
		config.services.diracIgnoreController = diracIgnoreController as any
		// We also need to mock the validator's checkDiracIgnorePath if it uses it
		sandbox.stub(validator, "checkDiracIgnorePath").callsFake((p: string) => {
			if (!p.includes("blocked")) {
				return { ok: true }
			} else {
				return { ok: false, error: "diracignore" }
			}
		})

		// Accumulate failures first
		taskState.consecutiveMistakeCount = 2

		// Create a real directory for the successful path
		const dirName = "real-dir"
		await fs.mkdir(path.join(tmpDir, dirName))
		await fs.writeFile(path.join(tmpDir, dirName, "file.txt"), "content")

		const result = await handler.execute(config, makeBlock(["blocked-dir", dirName]))

		assert.equal(typeof result, "string")
		assert.ok((result as string).includes("blocked-dir"))
		assert.ok((result as string).includes("file.txt"))
		assert.equal(taskState.consecutiveMistakeCount, 0)
	})

})

// ─── SearchFilesToolHandler ─────────────────────────────────────────────────

describe("SearchFilesToolHandler.execute – error recovery", () => {
	let sandbox: sinon.SinonSandbox

	beforeEach(async () => {
		sandbox = sinon.createSandbox()
		tmpDir = await fs.mkdtemp(path.join(os.tmpdir(), "dirac-search-test-"))
		sandbox.stub(pathUtils, "isLocatedInWorkspace").resolves(true)
		
	})

	afterEach(async () => {
		sandbox.restore()
		await fs.rm(tmpDir, { recursive: true, force: true }).catch(() => {})
	})

	function makeBlock(relPaths?: string | string[], regex?: string, filePattern?: string) {
		const params: Record<string, any> = {}
		if (relPaths !== undefined) {
			if (Array.isArray(relPaths)) {
				params.paths = relPaths
			} else {
				params.paths = [relPaths]
			}
		}
		if (regex !== undefined) params.regex = regex
		if (filePattern !== undefined) params.file_pattern = filePattern
		return {
			type: "tool_use" as const,
			name: DiracDefaultTool.SEARCH,
			params,
			partial: false,
		}
	}

	it("returns a tool result (not a thrown exception) for a non-existent directory", async () => {
		const { config, taskState, validator } = createConfig()
		const handler = new SearchFilesToolHandler(validator)

		const result = await handler.execute(config, makeBlock("no-such-dir", "pattern"))

		// regexSearchFiles throws for non-existent directories, executeSearch catches it,
		// returns success=false, and the handler increments consecutiveMistakeCount.
		assert.equal(typeof result, "string")
		assert.equal(taskState.consecutiveMistakeCount, 1)
	})

	it("increments consecutiveMistakeCount when path parameter is missing", async () => {
		const { config, taskState, validator } = createConfig()
		const handler = new SearchFilesToolHandler(validator)

		const result = await handler.execute(config, makeBlock(undefined, "pattern"))

		assert.equal(result, "missing")
		assert.equal(taskState.consecutiveMistakeCount, 1)
	})

	it("increments consecutiveMistakeCount when regex parameter is missing", async () => {
		const { config, taskState, validator } = createConfig()
		const handler = new SearchFilesToolHandler(validator)

		const result = await handler.execute(config, makeBlock("some-dir"))

		assert.equal(result, "missing")
		assert.equal(taskState.consecutiveMistakeCount, 1)
	})

	it("repeated missing-param failures accumulate", async () => {
		const { config, taskState, validator } = createConfig()
		const handler = new SearchFilesToolHandler(validator)

		await handler.execute(config, makeBlock()) // missing path
		await handler.execute(config, makeBlock("dir")) // missing regex
		await handler.execute(config, makeBlock()) // missing path again
		assert.equal(taskState.consecutiveMistakeCount, 3)
	})

	it("resets consecutiveMistakeCount to 0 after a successful search", async () => {
		const { config, taskState, validator } = createConfig()
		const handler = new SearchFilesToolHandler(validator)

		// Accumulate failures
		await handler.execute(config, makeBlock())
		await handler.execute(config, makeBlock("dir"))
		assert.equal(taskState.consecutiveMistakeCount, 2)

		// Stub regexSearchFiles to return a successful result
		const ripgrepModule = await import("@services/ripgrep")
		sandbox.stub(ripgrepModule, "regexSearchFiles").resolves("Found 0 results.\n\n")

		// Search in tmpDir (exists, will find 0 results but should succeed)
		const result = await handler.execute(config, makeBlock(".", "nonexistent-pattern-xyz"))
		assert.equal(typeof result, "string")
		assert.equal(taskState.consecutiveMistakeCount, 0)
	})

	it("catches a thrown exception from path resolution and returns a tool error", async () => {
		const { config, taskState, validator } = createConfig()
		const handler = new SearchFilesToolHandler(validator)

		// Stub determineSearchPaths to throw (simulating a bad workspace config)
		sandbox.stub(handler as any, "determineSearchPaths").throws(new Error("invalid workspace hint"))

		const result = await handler.execute(config, makeBlock("some-dir", "pattern"))

		assert.equal(typeof result, "string")
		assert.ok((result as string).includes("Error"))
		assert.ok((result as string).includes("invalid workspace hint"))
		assert.equal(taskState.consecutiveMistakeCount, 1)
	})

	it("accumulates failures when path resolution throws repeatedly", async () => {
		const { config, taskState, validator } = createConfig()
		const handler = new SearchFilesToolHandler(validator)

		sandbox.stub(handler as any, "determineSearchPaths").throws(new Error("boom"))

		await handler.execute(config, makeBlock("dir-1", "pat"))
		assert.equal(taskState.consecutiveMistakeCount, 1)

		await handler.execute(config, makeBlock("dir-2", "pat"))
		assert.equal(taskState.consecutiveMistakeCount, 2)

		await handler.execute(config, makeBlock("dir-3", "pat"))
		assert.equal(taskState.consecutiveMistakeCount, 3)
	})

	it("increments consecutiveMistakeCount when regexSearchFiles throws", async () => {
		const { config, taskState, validator } = createConfig()
		const handler = new SearchFilesToolHandler(validator)

		// Stub regexSearchFiles to throw
		const ripgrepModule = await import("@services/ripgrep")
		sandbox.stub(ripgrepModule, "regexSearchFiles").rejects(new Error("ripgrep crashed"))

		const result = await handler.execute(config, makeBlock(".", "pattern"))

		// The search failed, so consecutiveMistakeCount should increment
		assert.equal(typeof result, "string")
		assert.equal(taskState.consecutiveMistakeCount, 1)
	})

	it("accumulates failures when regexSearchFiles throws repeatedly", async () => {
		const { config, taskState, validator } = createConfig()
		const handler = new SearchFilesToolHandler(validator)

		const ripgrepModule = await import("@services/ripgrep")
		sandbox.stub(ripgrepModule, "regexSearchFiles").rejects(new Error("boom"))

		await handler.execute(config, makeBlock("dir-1", "pat"))
		assert.equal(taskState.consecutiveMistakeCount, 1)

		await handler.execute(config, makeBlock("dir-2", "pat"))
		assert.equal(taskState.consecutiveMistakeCount, 2)

		await handler.execute(config, makeBlock("dir-3", "pat"))
		assert.equal(taskState.consecutiveMistakeCount, 3)
	})

	it("resets consecutiveMistakeCount after regexSearchFiles failure is followed by success", async () => {
		const { config, taskState, validator } = createConfig()
		const handler = new SearchFilesToolHandler(validator)

		const ripgrepModule = await import("@services/ripgrep")
		const stub = sandbox.stub(ripgrepModule, "regexSearchFiles")

		// First two calls fail
		stub.onFirstCall().rejects(new Error("boom"))
		stub.onSecondCall().rejects(new Error("boom"))
		// Third call succeeds
		stub.onThirdCall().resolves("Found 0 results.\n\n")

		await handler.execute(config, makeBlock("dir-1", "pat"))
		assert.equal(taskState.consecutiveMistakeCount, 1)

		await handler.execute(config, makeBlock("dir-2", "pat"))
		assert.equal(taskState.consecutiveMistakeCount, 2)

		await handler.execute(config, makeBlock("dir-3", "pat"))
		assert.equal(taskState.consecutiveMistakeCount, 0)
	})


	it("resets consecutiveMistakeCount if at least one path succeeds in a multi-path call", async () => {
		const { config, taskState, validator } = createConfig()
		const handler = new SearchFilesToolHandler(validator)

		const ripgrepModule = await import("@services/ripgrep")
		const stub = sandbox.stub(ripgrepModule, "regexSearchFiles")

		// Mock one failure and one success
		stub.withArgs(sinon.match.any, sinon.match(/blocked/), sinon.match.any).rejects(new Error("boom"))
		stub.withArgs(sinon.match.any, sinon.match(/success/), sinon.match.any).resolves("Found 1 result.\n\nfile.txt\nmatch")

		// Accumulate failures first
		taskState.consecutiveMistakeCount = 2

		const result = await handler.execute(config, makeBlock(["blocked-dir", "success-dir"], "pattern"))

		assert.equal(typeof result, "string")
		assert.ok((result as string).includes("Found 1 result"))
		assert.equal(taskState.consecutiveMistakeCount, 0)
	})

	it("correctly handles an array passed to the 'path' parameter (bug fix)", async () => {
		const { config, taskState, validator } = createConfig()
		const handler = new SearchFilesToolHandler(validator)

		const ripgrepModule = await import("@services/ripgrep")
		sandbox.stub(ripgrepModule, "regexSearchFiles").resolves("Found 0 results.\n\n")

		// Pass array to 'path' instead of 'paths'
		const block = {
			type: "tool_use" as const,
			name: DiracDefaultTool.SEARCH,
			params: {
				path: ["src/transformers/pipelines"],
				regex: "class.*Pipeline",
			},
			partial: false,
		}

		const result = await handler.execute(config, block as any)

		assert.equal(typeof result, "string")
		assert.equal(taskState.consecutiveMistakeCount, 0)
	})
})
