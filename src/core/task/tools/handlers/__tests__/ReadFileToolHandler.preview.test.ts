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

function makeBlock(relPath: string, startLine?: number, endLine?: number) {
	return {
		type: "tool_use" as const,
		id: "tool-1",
		name: DiracDefaultTool.FILE_READ,
		params: {
			paths: [relPath],
			...(startLine !== undefined ? { start_line: startLine } : {}),
			...(endLine !== undefined ? { end_line: endLine } : {}),
		},
		partial: false,
	}
}

describe("ReadFileToolHandler.execute – large file preview", () => {
	let sandbox: sinon.SinonSandbox

	beforeEach(async () => {
		sandbox = sinon.createSandbox()
		tmpDir = await fs.mkdtemp(path.join(os.tmpdir(), "dirac-read-preview-test-"))
		sandbox.stub(pathUtils, "isLocatedInWorkspace").resolves(true)
	})

	afterEach(async () => {
		sandbox.restore()
		await fs.rm(tmpDir, { recursive: true, force: true }).catch(() => {})
	})

	it("returns a preview for a large file without range", async () => {
		const { config, taskState, validator } = createConfig()
		const handler = new ReadFileToolHandler(validator)

		// Create a file > 50KB (approx 180KB)
		const largeContent = Array.from({ length: 2000 }, (_, i) => `Line ${i + 1}: Some repetitive text to fill up space. Extra content to ensure it is large enough.`).join("\n")
		const largeFile = "large-file.txt"
		await fs.writeFile(path.join(tmpDir, largeFile), largeContent)

		const result = await handler.execute(config, makeBlock(largeFile))
		assert.equal(typeof result, "string")
		assert.ok((result as string).includes("[File Hash:"))
		assert.ok((result as string).includes("NOTE: File 'large-file.txt' is"))
		assert.ok((result as string).includes("(~2,000 lines). Showing first 200 lines."))
		assert.ok((result as string).includes("To view other sections, add 'start_line' and 'end_line' parameters (e.g. start_line=201, end_line=400)."))
		
		// Verify content: should have 200 lines
		const lines = (result as string).split("\n").filter(l => l.includes("|Line "))
		assert.equal(lines.length, 200)
		assert.equal(taskState.consecutiveMistakeCount, 0)
	})

	it("returns full content for a large file if range is specified", async () => {
		const { config, taskState, validator } = createConfig()
		const handler = new ReadFileToolHandler(validator)

		const largeContent = Array.from({ length: 1000 }, (_, i) => `Line ${i + 1}: Some repetitive text.`).join("\n")
		const largeFile = "large-file.txt"
		await fs.writeFile(path.join(tmpDir, largeFile), largeContent)

		// Request a range that is still "large" but has explicit range
		const result = await handler.execute(config, makeBlock(largeFile, 1, 300))

		assert.equal(typeof result, "string")
		assert.ok(!(result as string).includes("preview"))
		const lines = (result as string).split("\n").filter(l => l.includes("|Line "))
		assert.equal(lines.length, 300)
		assert.equal(taskState.consecutiveMistakeCount, 0)
	})

	it("includes symbols if file is a supported code file", async () => {
		const { config, validator } = createConfig()
		const handler = new ReadFileToolHandler(validator)

		// Create a large TS file
		const codeLines = [
			"import fs from 'fs';",
			"export class MyClass {",
			"  constructor() {}",
			"  myMethod() {",
			"    console.log('hello');",
			"  }",
			"}",
			"export function myFunction() { return 1; }",
		]
		const padding = Array.from({ length: 5000 }, () => "// padding line to make it large").join("\n")
		const largeCode = codeLines.join("\n") + "\n" + padding
		const largeFile = "large-file.ts"
		await fs.writeFile(path.join(tmpDir, largeFile), largeCode)

		const result = await handler.execute(config, makeBlock(largeFile))
		assert.equal(typeof result, "string")
		assert.ok((result as string).includes("Chunk map (full file):"))
		assert.ok((result as string).includes("export class MyClass"))
		assert.ok((result as string).includes("myMethod()"))
		assert.ok((result as string).includes("export function myFunction()"))
	})
})
