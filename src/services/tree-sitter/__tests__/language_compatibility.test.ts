import { strict as assert } from "node:assert"
import * as fs from "node:fs/promises"
import * as path from "node:path"
import { TaskState } from "@core/task/TaskState"
import { FindSymbolReferencesToolHandler } from "@core/task/tools/handlers/FindSymbolReferencesToolHandler"
import { GetFileSkeletonToolHandler } from "@core/task/tools/handlers/GetFileSkeletonToolHandler"
import { GetFunctionToolHandler } from "@core/task/tools/handlers/GetFunctionToolHandler"
import { ReplaceSymbolToolHandler } from "@core/task/tools/handlers/ReplaceSymbolToolHandler"
import { ToolValidator } from "@core/task/tools/ToolValidator"
import { DiracDefaultTool } from "@shared/tools"
import { stripHashes } from "@shared/utils/line-hashing"
import { FileAnchorIndex } from "@shared/utils/file-anchor-index"
import { before, beforeEach, describe, it } from "mocha"
import sinon from "sinon"
import { HostProvider } from "@/hosts/host-provider"
import * as diagnosticsProvidersModule from "@/integrations/diagnostics/getDiagnosticsProviders"
import { SymbolIndexService } from "@/services/symbol-index/SymbolIndexService"

const UPDATE_SNAPSHOTS = process.env.UPDATE_SNAPSHOTS === "true" || process.argv.includes("--update-snapshots")
const FIXTURES_DIR = path.join(__dirname, "fixtures")

function createMockConfig(cwd: string) {
	const taskState = new TaskState()
	const callbacks = {
		say: sinon.stub().resolves(undefined),
		ask: sinon.stub().resolves({ response: "yesButtonClicked" }),
		shouldAutoApproveToolWithPath: sinon.stub().resolves(true),
		removeLastPartialMessageIfExistsWithType: sinon.stub().resolves(),
		sayAndCreateMissingParamError: sinon.stub().resolves("missing_param_error"),
		cancelTask: sinon.stub().resolves(),
		setActiveHookExecution: sinon.stub().resolves(),
		clearActiveHookExecution: sinon.stub().resolves(),
	}

	return {
		taskId: "test-task",
		ulid: "test-ulid",
		cwd,
		taskState,
		callbacks,
		messageState: {
			getApiConversationHistory: sinon.stub().returns([]),
		},

		api: {
			getModel: () => ({ id: "test-model", info: { supportsImages: false } }),
		},
		services: {
			stateManager: {
				getApiConfiguration: () => ({
					planModeApiProvider: "openai",
					actModeApiProvider: "openai",
				}),
				getGlobalSettingsKey: (key: string) => {
					if (key === "mode") return "act"
					if (key === "hooksEnabled") return false
					return undefined
				},
			},
			fileContextTracker: {
				markFileAsEditedByDirac: sinon.stub(),
				trackFileContext: sinon.stub().resolves(),
			},
			diracIgnoreController: {
				validateAccess: () => true,
				filterPaths: (paths: string[]) => paths,
			},
			diffViewProvider: {
				editType: undefined,
				open: sinon.stub().resolves(),
				update: sinon.stub().resolves(),
				saveChanges: sinon.stub().resolves({ finalContent: "", newProblemsMessage: "" }),
				reset: sinon.stub().resolves(),
				revertChanges: sinon.stub().resolves(),
			},

		},
	} as any
}

async function assertSnapshot(filePath: string, actual: string) {
	const strippedActual = stripHashes(actual)
	if (UPDATE_SNAPSHOTS) {
		await fs.mkdir(path.dirname(filePath), { recursive: true })
		await fs.writeFile(filePath, strippedActual, "utf-8")
		return
	}

	try {
		const expected = await fs.readFile(filePath, "utf-8")
		assert.strictEqual(strippedActual, expected, `Snapshot mismatch for ${filePath}`)
	} catch (error: any) {
		if (error.code === "ENOENT") {
			throw new Error(`Snapshot not found: ${filePath}. Run with UPDATE_SNAPSHOTS=true to create it.`)
		}
		throw error
	}
}

describe("Language Compatibility Tests (Big Four)", () => {
	const languages = [
		{ name: "typescript", ext: "ts" },
		{ name: "python", ext: "py" },
		{ name: "rust", ext: "rs" },
		{ name: "cpp", ext: "cpp" },
		{ name: "go", ext: "go" },
		{ name: "c", ext: "c" },
		{ name: "csharp", ext: "cs" },
		{ name: "ruby", ext: "rb" },
		{ name: "java", ext: "java" },
		{ name: "php", ext: "php" },
		{ name: "swift", ext: "swift" },
		{ name: "kotlin", ext: "kt" },
	]

	const validator = new ToolValidator({ validateAccess: () => true } as any)
	const handlers = {
		skeleton: new GetFileSkeletonToolHandler(validator),
		getFunction: new GetFunctionToolHandler(validator),
		references: new FindSymbolReferencesToolHandler(validator),
		replace: new ReplaceSymbolToolHandler(validator),
	}

	before(async () => {
		SymbolIndexService.getInstance().setPersistenceEnabled(false)
		if (!HostProvider.isInitialized()) {
			HostProvider.initialize(
				"extension",
				null as any,
				null as any,
				null as any,
				null as any,
				{
					workspaceClient: {
						saveOpenDocumentIfDirty: sinon.stub().resolves(),
						getWorkspacePaths: sinon.stub().resolves({ paths: [FIXTURES_DIR] }),
					},
				} as any,
				null as any,
				null as any,
				null as any,
				"/tmp",
				"/tmp",
				async (_cwd: string) => undefined
			)
		}

		// Mock diagnostics provider to prevent timeouts during linter polling
		sinon.stub(diagnosticsProvidersModule, "getDiagnosticsProviders").returns([
			{
				capturePreSaveState: sinon.stub().resolves([]),
				getDiagnosticsFeedback: sinon.stub().resolves({
					fixedCount: 0,
					newProblemsMessage: "",
				}),
				getDiagnosticsFeedbackForFiles: sinon.stub().callsFake(async (data) => data.map(() => ({ newProblemsMessage: "", fixedCount: 0 }))),
			} as any,
		])
	})

	after(() => {
		sinon.restore()
	})

	for (const lang of languages) {
		describe(`Language: ${lang.name}`, () => {
			const langDir = path.join(FIXTURES_DIR, lang.name)
			const samplePath = path.join(langDir, `sample.${lang.ext}`)
			let config: any

			beforeEach(async () => {
				config = createMockConfig(langDir)
				
			})

			it("get_file_skeleton", async () => {
				const result = await handlers.skeleton.execute(config, {
					name: DiracDefaultTool.GET_FILE_SKELETON,
					params: { paths: [`sample.${lang.ext}`] },
				} as any)
				await assertSnapshot(path.join(langDir, "get_file_skeleton.txt"), result as string)
			})

			describe("Complex Tool Tests", () => {
				let testCases: any
				before(async () => {
					const testsJson = await fs.readFile(path.join(langDir, "tests.json"), "utf-8")
					testCases = JSON.parse(testsJson)
				})

				it("get_function", async () => {
					for (const test of testCases.get_function) {
						const result = await handlers.getFunction.execute(config, {
							name: DiracDefaultTool.GET_FUNCTION,
							params: { paths: [`sample.${lang.ext}`], function_names: test.symbols },
						} as any)
						await assertSnapshot(path.join(langDir, `get_function_${test.name}.txt`), result as string)
					}
				})

				it("find_symbol_references", async () => {
					for (const test of testCases.find_symbol_references) {
						const result = await handlers.references.execute(config, {
							name: DiracDefaultTool.FIND_SYMBOL_REFERENCES,
							params: {
								paths: [`sample.${lang.ext}`],
								symbols: test.symbols,
								find_type: test.find_type || "both",
							},
						} as any)
						await assertSnapshot(path.join(langDir, `find_symbol_references_${test.name}.txt`), result as string)
					}
				})

				it("replace_symbol", async () => {
					// Backup sample file content
					const originalContent = await fs.readFile(samplePath, "utf-8")
					try {
						for (const test of testCases.replace_symbol) {
							const testConfig = createMockConfig(langDir)
							const result = await handlers.replace.execute(testConfig, {
								name: DiracDefaultTool.REPLACE_SYMBOL,
								params: {
									path: `sample.${lang.ext}`,
									symbol: test.symbol,
									text: test.text,
								},
							} as any)
							await assertSnapshot(path.join(langDir, `replace_symbol_${test.name}.txt`), result as string)
							// Restore original content after each replace test
							await fs.writeFile(samplePath, originalContent, "utf-8")
						}
					} finally {
						await fs.writeFile(samplePath, originalContent, "utf-8")
					}
				})
			})
		})
	}
})
