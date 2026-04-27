import { strict as assert } from "node:assert"
import fs from "node:fs/promises"
import os from "node:os"
import path from "node:path"
import { DiracDefaultTool } from "@shared/tools"
import * as pathUtils from "@utils/path"
import { afterEach, beforeEach, describe, it } from "mocha"
import sinon from "sinon"
import { TaskState } from "../../../TaskState"
import { ToolValidator } from "../../ToolValidator"
import type { TaskConfig } from "../../types/TaskConfig"
import { ReadFileToolHandler } from "../ReadFileToolHandler"

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
		isSubagentExecution: true,
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
			diracIgnoreController: { validateAccess: () => true },
			commandPermissionController: {},
			contextManager: {},
		},
		callbacks,
		coordinator: { getHandler: sinon.stub() },
	} as unknown as TaskConfig

	const validator = new ToolValidator({ validateAccess: () => true } as any)

	return { config, callbacks, taskState, validator }
}

describe("ReadFileToolHandler - Enhanced Exploration", () => {
	let sandbox: sinon.SinonSandbox

	beforeEach(async () => {
		sandbox = sinon.createSandbox()
		tmpDir = await fs.mkdtemp(path.join(os.tmpdir(), "dirac-read-enhanced-test-"))
		sandbox.stub(pathUtils, "isLocatedInWorkspace").resolves(true)
	})

	afterEach(async () => {
		sandbox.restore()
		await fs.rm(tmpDir, { recursive: true, force: true }).catch(() => {})
	})

	it("supports reading multiple ranges in one call", async () => {
		const { config, validator } = createConfig()
		const handler = new ReadFileToolHandler(validator)
		const content = Array.from({ length: 100 }, (_, i) => `Line ${i + 1}`).join("\n")
		await fs.writeFile(path.join(tmpDir, "test.txt"), content)

		const result = await handler.execute(config, {
			type: "tool_use", id: "t1", name: DiracDefaultTool.FILE_READ, partial: false,
			params: { paths: ["test.txt"], ranges: [{ start: 10, end: 20 }, { start: 50, end: 60 }] }
		})

		const resStr = result as string
		assert.ok(resStr.includes("Lines 10-20"))
		assert.ok(resStr.includes("Lines 50-60"))
		assert.ok(resStr.includes("|Line 15"))
		assert.ok(resStr.includes("|Line 55"))
		assert.ok(!resStr.includes("|Line 30"))
	})

	it("merges overlapping ranges", async () => {
		const { config, validator } = createConfig()
		const handler = new ReadFileToolHandler(validator)
		const content = Array.from({ length: 100 }, (_, i) => `Line ${i + 1}`).join("\n")
		await fs.writeFile(path.join(tmpDir, "test.txt"), content)

		const result = await handler.execute(config, {
			type: "tool_use", id: "t1", name: DiracDefaultTool.FILE_READ, partial: false,
			params: { paths: ["test.txt"], ranges: [{ start: 10, end: 25 }, { start: 20, end: 40 }] }
		})

		const resStr = result as string
		assert.ok(resStr.includes("Lines 10-40"))
		assert.ok(!resStr.includes("Lines 10-25"))
		assert.ok(!resStr.includes("Lines 20-40"))
	})

	it("returns compact 'unchanged' signal for repeat reads", async () => {
		const { config, validator } = createConfig()
		const handler = new ReadFileToolHandler(validator)
		const content = "Pure and simple content."
		await fs.writeFile(path.join(tmpDir, "simple.txt"), content)

		// First read
		await handler.execute(config, {
			type: "tool_use", id: "t1", name: DiracDefaultTool.FILE_READ, partial: false,
			params: { paths: ["simple.txt"] }
		})

		// Second read of same file
		const result = await handler.execute(config, {
			type: "tool_use", id: "t2", name: DiracDefaultTool.FILE_READ, partial: false,
			params: { paths: ["simple.txt"] }
		})

		assert.ok((result as string).includes("unchanged since your last read"))
		assert.ok(!(result as string).includes("Pure and simple content"))
	})

	it("auto-expands preview size after 3 reads", async () => {
		const { config, validator } = createConfig()
		const handler = new ReadFileToolHandler(validator)
		const content = Array.from({ length: 1000 }, (_, i) => `Line ${i + 1}: Some repetitive text to make the file large enough to trigger preview logic.`).join("\n")
		await fs.writeFile(path.join(tmpDir, "large.ts"), content)

		// Read 1
		await handler.execute(config, {
			type: "tool_use", id: "t1", name: DiracDefaultTool.FILE_READ, partial: false,
			params: { paths: ["large.ts"] }
		})
		// Read 2
		await handler.execute(config, {
			type: "tool_use", id: "t2", name: DiracDefaultTool.FILE_READ, partial: false,
			params: { paths: ["large.ts"] }
		})
		
		// Modify file to avoid "unchanged" hit
		await fs.writeFile(path.join(tmpDir, "large.ts"), content + "\n// modified")

		// Read 3 - should expand to 500 lines
		const result = await handler.execute(config, {
			type: "tool_use", id: "t3", name: DiracDefaultTool.FILE_READ, partial: false,
			params: { paths: ["large.ts"] }
		})

		const resStr = result as string
		assert.ok(resStr.includes("Extended preview shown due to multiple reads"))
		assert.ok(resStr.includes("|Line 450"))
		assert.ok(!resStr.includes("|Line 550"))
	})

	it("supports reading multiple files in one call", async () => {
		const { config, validator } = createConfig()
		const handler = new ReadFileToolHandler(validator)
		await fs.writeFile(path.join(tmpDir, "a.txt"), "Content A")
		await fs.writeFile(path.join(tmpDir, "b.txt"), "Content B")

		const result = await handler.execute(config, {
			type: "tool_use", id: "t1", name: DiracDefaultTool.FILE_READ, partial: false,
			params: { paths: ["a.txt", "b.txt"] }
		})

		const resStr = result as string
		// Header for multiple files should be present
		assert.ok(resStr.includes("--- a.txt ---"))
		assert.ok(resStr.includes("--- b.txt ---"))
		assert.ok(resStr.includes("Content A"))
		assert.ok(resStr.includes("Content B"))
	})
})
