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
import { ExpandSymbolToolHandler } from "../ExpandSymbolToolHandler"
import { SearchSymbolsToolHandler } from "../SearchSymbolsToolHandler"
import { RepoMapToolHandler } from "../RepoMapToolHandler"

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

describe("Progressive Exploration Tools", () => {
	let sandbox: sinon.SinonSandbox

	beforeEach(async () => {
		sandbox = sinon.createSandbox()
		tmpDir = await fs.mkdtemp(path.join(os.tmpdir(), "dirac-expl-test-"))
		sandbox.stub(pathUtils, "isLocatedInWorkspace").resolves(true)
	})

	afterEach(async () => {
		sandbox.restore()
		await fs.rm(tmpDir, { recursive: true, force: true }).catch(() => {})
	})

	describe("read_file detail modes", () => {
		it("returns outline detail", async () => {
			const { config, validator } = createConfig()
			const handler = new ReadFileToolHandler(validator)
			const content = "export class MyClass {\n  myMethod() {}\n}\nexport function myFunc() {}"
			await fs.writeFile(path.join(tmpDir, "test.ts"), content)

			const result = await handler.execute(config, {
				type: "tool_use", id: "t1", name: DiracDefaultTool.FILE_READ, partial: false,
				params: { paths: ["test.ts"], detail: "outline" }
			})

			assert.ok((result as string).includes("[class:MyClass]"))
			assert.ok((result as string).includes("[fn:MyClass.myMethod]"))
			assert.ok((result as string).includes("[fn:myFunc]"))
		})

		it("returns skeleton detail", async () => {
			const { config, validator } = createConfig()
			const handler = new ReadFileToolHandler(validator)
			const content = "export function myFunc() {\n  console.log('line 1');\n  console.log('line 2');\n}"
			await fs.writeFile(path.join(tmpDir, "test.ts"), content)

			const result = await handler.execute(config, {
				type: "tool_use", id: "t1", name: DiracDefaultTool.FILE_READ, partial: false,
				params: { paths: ["test.ts"], detail: "skeleton" }
			})

			assert.ok((result as string).includes("export function myFunc()"))
			assert.ok((result as string).includes("{ ... }"))
			assert.ok(!(result as string).includes("line 1"))
		})

		it("auto-degrades based on max_tokens", async () => {
			const { config, validator } = createConfig()
			const handler = new ReadFileToolHandler(validator)
			const content = "export function f1() {}\n".repeat(50)
			await fs.writeFile(path.join(tmpDir, "test.ts"), content)

			const result = await handler.execute(config, {
				type: "tool_use", id: "t1", name: DiracDefaultTool.FILE_READ, partial: false,
				params: { paths: ["test.ts"], detail: "full", max_tokens: 50 }
			})

			assert.ok((result as string).includes("[DEGRADED TO STAY IN BUDGET]"))
		})
	})

	describe("pagination", () => {
		it("supports page: 'next'", async () => {
			const { config, validator } = createConfig()
			const handler = new ReadFileToolHandler(validator)
			const content = Array.from({ length: 500 }, (_, i) => `Line ${i + 1}`).join("\n")
			await fs.writeFile(path.join(tmpDir, "large.txt"), content)

			// First read (preview)
			await handler.execute(config, {
				type: "tool_use", id: "t1", name: DiracDefaultTool.FILE_READ, partial: false,
				params: { paths: ["large.txt"] }
			})

			// Next page
			const result = await handler.execute(config, {
				type: "tool_use", id: "t2", name: DiracDefaultTool.FILE_READ, partial: false,
				params: { paths: ["large.txt"], page: "next" }
			})

			assert.ok((result as string).includes("|Line 201"))
			assert.ok((result as string).includes("|Line 400"))
		})

		it("supports page: 'section'", async () => {
			const { config, validator } = createConfig()
			const handler = new ReadFileToolHandler(validator)
			const code = "function f1() {}\n".repeat(250) + "export function targetFunc() {\n  // body\n}"
			await fs.writeFile(path.join(tmpDir, "code.ts"), code)

			const result = await handler.execute(config, {
				type: "tool_use", id: "t1", name: DiracDefaultTool.FILE_READ, partial: false,
				params: { paths: ["code.ts"], page: "section", section: "fn:targetFunc" }
			})

			assert.ok((result as string).includes("export function targetFunc()"))
		})
	})

	describe("expand_symbol", () => {
		it("expands a symbol body", async () => {
			const { config, validator } = createConfig()
			const handler = new ExpandSymbolToolHandler(validator)
			const content = "function other() {}\nexport function myFunc() {\n  console.log('secret');\n}"
			await fs.writeFile(path.join(tmpDir, "test.ts"), content)

			const result = await handler.execute(config, {
				type: "tool_use", id: "t1", name: DiracDefaultTool.EXPAND_SYMBOL, partial: false,
				params: { path: "test.ts", symbol: "fn:myFunc" }
			})

			assert.ok((result as string).includes("secret"))
			assert.ok(!(result as string).includes("function other()"))
		})
	})

	describe("search_symbols", () => {
		it("finds symbols by query", async () => {
			const { config, validator } = createConfig()
			const handler = new SearchSymbolsToolHandler(validator)
			await fs.writeFile(path.join(tmpDir, "a.ts"), "export function findMe() {}")
			await fs.writeFile(path.join(tmpDir, "b.ts"), "export class LostAndFound {}")

			const result = await handler.execute(config, {
				type: "tool_use", id: "t1", name: DiracDefaultTool.SEARCH_SYMBOLS, partial: false,
				params: { query: "Me" }
			})
			assert.ok((result as string).includes("fn:findMe"))
			
			const result2 = await handler.execute(config, {
				type: "tool_use", id: "t2", name: DiracDefaultTool.SEARCH_SYMBOLS, partial: false,
				params: { query: "Lost" }
			})
			assert.ok((result2 as string).includes("class:LostAndFound"))
		})
	})

	describe("repo_map", () => {
		it("returns a summary of the repository", async () => {
			const { config, validator } = createConfig()
			const handler = new RepoMapToolHandler(validator)
			// Add some content to ensure it's not empty and has a symbol
			await fs.writeFile(path.join(tmpDir, "main.ts"), "import { x } from './other';\nexport function start() {\n  console.log('hi');\n}")
			
			const result = await handler.execute(config, {
				type: "tool_use", id: "t1", name: DiracDefaultTool.REPO_MAP, partial: false,
				params: {}
			})

			const resStr = result as string
			assert.ok(resStr.includes("Repository Structure Summary:"), "Should have summary header")
			assert.ok(resStr.includes("main.ts"), "Should list main.ts")
			assert.ok(resStr.includes("start"), "Should find 'start' symbol")
		})
	})
})
