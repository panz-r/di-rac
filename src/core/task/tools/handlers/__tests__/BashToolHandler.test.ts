import { strict as assert } from "node:assert"
import fs from "node:fs/promises"
import os from "node:os"
import path from "node:path"
import proxyquire from "proxyquire"
import { DiracDefaultTool } from "@shared/tools"
import { afterEach, beforeEach, describe, it } from "mocha"
import sinon from "sinon"
import { TaskState } from "../../../TaskState"
import { ToolValidator } from "../../ToolValidator"
import type { TaskConfig } from "../../types/TaskConfig"
import { BashToolHandler } from "../BashToolHandler"

let tmpDir: string

function createConfig() {
	const taskState = new TaskState()

	const callbacks = {
		say: sinon.stub().resolves(undefined),
		ask: sinon.stub().resolves({ didApprove: true }),
		sayAndCreateMissingParamError: sinon.stub().resolves("missing"),
		removeLastPartialMessageIfExistsWithType: sinon.stub().resolves(),
		shouldAutoApproveToolWithPath: sinon.stub().resolves(false),
		postStateToWebview: sinon.stub().resolves(),
		cancelTask: sinon.stub().resolves(),
		updateTaskHistory: sinon.stub().resolves([]),
		switchToActMode: sinon.stub().resolves(false),
		getDiracMessages: sinon.stub().returns([]),
		updateDiracMessage: sinon.stub().resolves(),
		executeCommandTool: sinon.stub().resolves([false, "ok"]),
	}

	const config = {
		taskId: "task-1",
		ulid: "ulid-1",
		cwd: tmpDir,
		mode: "act",
		taskState,
		api: {
			getModel: () => ({ id: "test-model" }),
		},
		autoApprovalSettings: {
			enableNotifications: false,
		},
		services: {
			stateManager: {
				getGlobalSettingsKey: (key: string) => {
					if (key === "mode") return "act"
					return undefined
				},
				getApiConfiguration: () => ({
					planModeApiProvider: "openai",
					actModeApiProvider: "openai",
				}),
			},
			diracIgnoreController: { validateCommand: () => undefined },
			fileContextTracker: { trackFileContext: sinon.stub().resolves() },
			commandPermissionController: { validateCommand: () => ({ allowed: true }) },
		},
		callbacks,
	} as unknown as TaskConfig

	const validator = new ToolValidator({ validateAccess: () => true } as any)

	return { config, callbacks, taskState, validator }
}

describe("BashToolHandler", () => {
	beforeEach(async () => {
		tmpDir = await fs.mkdtemp(path.join(os.tmpdir(), "dirac-bash-test-"))
	})

	afterEach(async () => {
		await fs.rm(tmpDir, { recursive: true, force: true }).catch(() => {})
	})

	it("executes an allowed command after approval", async () => {
		const { config, validator } = createConfig()
		const handler = new BashToolHandler(validator)

		const result = await handler.execute(config, {
			type: "tool_use", id: "t1", name: DiracDefaultTool.BASH_RESTRICTED, partial: false,
			params: { command: "ls" }
		})

		assert.equal(typeof result, "string")
		const parsed = JSON.parse(result as string)
		assert.equal(parsed.ok, true)
		assert.equal(parsed.exitCode, 0)
	})

	it("blocks commands not in allowlist", async () => {
		const { config, validator } = createConfig()
		const handler = new BashToolHandler(validator)

		const result = await handler.execute(config, {
			type: "tool_use", id: "t1", name: DiracDefaultTool.BASH_RESTRICTED, partial: false,
			params: { command: "rm -rf /" }
		})
		const parsed = JSON.parse(result as string)
		assert.equal(parsed.ok, false)
		assert.equal(parsed.error, "BINARY_NOT_ALLOWED")
	})

	it("blocks path traversal", async () => {
		const { config, validator } = createConfig()
		const handler = new BashToolHandler(validator)

		const result = await handler.execute(config, {
			type: "tool_use", id: "t1", name: DiracDefaultTool.BASH_RESTRICTED, partial: false,
			params: { command: "cat ../passwd" }
		})
		const parsed = JSON.parse(result as string)
		assert.equal(parsed.ok, false)
		assert.equal(parsed.error, "PATH_ESCAPE")
	})

	it("blocks path arguments exceeding 255 bytes", async () => {
		const { config, validator } = createConfig()
		const handler = new BashToolHandler(validator)

		const longPath = "/tmp/" + "a".repeat(300)
		const result = await handler.execute(config, {
			type: "tool_use", id: "t1", name: DiracDefaultTool.BASH_RESTRICTED, partial: false,
			params: { command: `cat ${longPath}` }
		})
		const parsed = JSON.parse(result as string)
		assert.equal(parsed.ok, false)
		assert.equal(parsed.error, "PATH_TOO_LONG")
		assert.ok(parsed.message.includes("305 bytes"))
	})

	it("allows normal-length path arguments", async () => {
		const { config, validator } = createConfig()
		const handler = new BashToolHandler(validator)

		const normalPath = "/tmp/test.txt"
		const result = await handler.execute(config, {
			type: "tool_use", id: "t1", name: DiracDefaultTool.BASH_RESTRICTED, partial: false,
			params: { command: `cat ${normalPath}` }
		})
		const parsed = JSON.parse(result as string)
		assert.equal(parsed.ok, true)
	})

	it("respects timeout", async function() {
		this.timeout(40000)
		const { config, validator } = createConfig()
		const handler = new BashToolHandler(validator)

		// Using python to sleep since 'sleep' might not be in allowlist if I didn't add it
		const result = await handler.execute(config, {
			type: "tool_use", id: "t1", name: DiracDefaultTool.BASH_RESTRICTED, partial: false,
			params: { command: "python3 -c 'import time; time.sleep(35)'" }
		})

		const parsed = JSON.parse(result as string)
		assert.equal(parsed.ok, false)
		assert.equal(parsed.error, "TIMEOUT")
	})

	it("rewrites paths if enabled", async () => {
		const { config, validator } = createConfig()
		
		// Mock spawn to verify command
		const spawnStub = sinon.stub().returns({
			stdout: { on: sinon.stub() },
			stderr: { on: sinon.stub() },
			on: sinon.stub().callsFake((event, cb) => {
				if (event === "close") cb(0)
			}),
			kill: sinon.stub()
		} as any)

		const { BashToolHandler: MockedBashToolHandler } = proxyquire("../BashToolHandler", {
			"node:child_process": { spawn: spawnStub }
		})
		
		const handler = new MockedBashToolHandler(validator)

		// Mock rewritePaths to true
		const originalGet = config.services.stateManager.getGlobalSettingsKey
		config.services.stateManager.getGlobalSettingsKey = sinon.stub().callsFake((key: any) => {
			if (key === "rewritePaths") return true
			return originalGet.call(config.services.stateManager, key)
		})

		const absPath = path.join(tmpDir, "some/file.txt")
		
		await handler.execute(config, {
			type: "tool_use", id: "t1", name: DiracDefaultTool.BASH_RESTRICTED, partial: false,
			params: { command: `ls ${absPath}` }
		})

		assert.ok(spawnStub.calledOnce)
		const args = spawnStub.firstCall.args[1] as string[]
		assert.ok(args.some(a => a.includes("ls some/file.txt")))
	})
})
