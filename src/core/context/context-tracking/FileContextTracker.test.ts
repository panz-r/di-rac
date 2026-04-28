import * as diskModule from "@core/storage/disk"
import { expect } from "chai"
import chokidar from "chokidar"
import { afterEach, beforeEach, describe, it } from "mocha"
import * as path from "path"
import * as sinon from "sinon"
import { Controller } from "@/core/controller"
import { setVscodeHostProviderMock } from "@/test/host-provider-test-utils"
import type { FileMetadataEntry, TaskMetadata } from "./ContextTrackerTypes"
import { FileContextTracker } from "./FileContextTracker"

describe("FileContextTracker", () => {
	const filePath = "src/test-file.ts"
	const taskId = "test-task-id"

	let sandbox: sinon.SinonSandbox
	let mockFileSystemWatcher: any
	let chokidarWatchStub: sinon.SinonStub
	let tracker: FileContextTracker
	let mockTaskMetadata: TaskMetadata
	let getTaskMetadataStub: sinon.SinonStub
	let saveTaskMetadataStub: sinon.SinonStub

	beforeEach(() => {
		sandbox = sinon.createSandbox()

		// Mock chokidar file watcher
		mockFileSystemWatcher = {
			close: sandbox.stub().resolves(),
			on: sandbox.stub(),
		}
		mockFileSystemWatcher.on.returns(mockFileSystemWatcher)

		chokidarWatchStub = sandbox.stub(chokidar, "watch").returns(mockFileSystemWatcher as any)

		// Mock disk module functions
		mockTaskMetadata = { files_in_context: [], model_usage: [], environment_history: [] }
		getTaskMetadataStub = sandbox.stub(diskModule, "getTaskMetadata").resolves(mockTaskMetadata)
		saveTaskMetadataStub = sandbox.stub(diskModule, "saveTaskMetadata").resolves()

		setVscodeHostProviderMock()

		// Create tracker instance
		tracker = new FileContextTracker({} as Controller, taskId)
	})

	afterEach(() => {
		sandbox.restore()
	})

	it("should add a record when a file is read by a tool", async () => {
		await tracker.trackFileContext(filePath, "read_tool")

		expect(getTaskMetadataStub.calledOnce).to.be.true
		expect(getTaskMetadataStub.firstCall.args[0]).to.equal(taskId)

		expect(saveTaskMetadataStub.calledOnce).to.be.true

		const savedMetadata = saveTaskMetadataStub.firstCall.args[1]
		expect(savedMetadata.files_in_context.length).to.equal(1)

		const fileEntry = savedMetadata.files_in_context[0]
		expect(fileEntry.path).to.equal(filePath)
		expect(fileEntry.record_state).to.equal("active")
		expect(fileEntry.record_source).to.equal("read_tool")
		expect(fileEntry.dirac_read_date).to.be.a("number")
		expect(fileEntry.dirac_edit_date).to.be.null
	})

	it("should add a record when a file is edited by Dirac", async () => {
		await tracker.trackFileContext(filePath, "dirac_edited")

		expect(saveTaskMetadataStub.calledOnce).to.be.true
		const savedMetadata = saveTaskMetadataStub.firstCall.args[1]

		expect(savedMetadata.files_in_context).to.be.an("array").that.is.not.empty

		const activeEntry = savedMetadata.files_in_context.find(
			(entry: FileMetadataEntry) => entry.path === filePath && entry.record_state === "active",
		)

		expect(activeEntry).to.exist

		expect(activeEntry.path).to.equal(filePath)
		expect(activeEntry.record_state).to.equal("active")
		expect(activeEntry.record_source).to.equal("dirac_edited")
		expect(activeEntry.dirac_read_date).to.be.a("number")
		expect(activeEntry.dirac_edit_date).to.be.a("number")
	})

	it("should add a record when a file is mentioned", async () => {
		await tracker.trackFileContext(filePath, "file_mentioned")

		const savedMetadata = saveTaskMetadataStub.firstCall.args[1]
		const fileEntry = savedMetadata.files_in_context[0]

		expect(fileEntry.path).to.equal(filePath)
		expect(fileEntry.record_state).to.equal("active")
		expect(fileEntry.record_source).to.equal("file_mentioned")
		expect(fileEntry.dirac_read_date).to.be.a("number")
		expect(fileEntry.dirac_edit_date).to.be.null
	})

	it("should add a record when a file is edited by the user", async () => {
		await tracker.trackFileContext(filePath, "user_edited")

		const savedMetadata = saveTaskMetadataStub.firstCall.args[1]
		const fileEntry = savedMetadata.files_in_context[0]

		expect(fileEntry.path).to.equal(filePath)
		expect(fileEntry.record_state).to.equal("active")
		expect(fileEntry.record_source).to.equal("user_edited")
		expect(fileEntry.user_edit_date).to.be.a("number")

		const modifiedFiles = tracker.getAndClearRecentlyModifiedFiles()
		expect(modifiedFiles).to.include(filePath)
	})

	it("should mark existing entries as stale when adding a new entry for the same file", async () => {
		mockTaskMetadata.files_in_context = [
			{
				path: filePath,
				record_state: "active",
				record_source: "read_tool",
				dirac_read_date: Date.now() - 1000,
				dirac_edit_date: null,
				user_edit_date: null,
			},
		]

		await tracker.trackFileContext(filePath, "dirac_edited")

		const savedMetadata = saveTaskMetadataStub.firstCall.args[1]
		expect(savedMetadata.files_in_context.length).to.equal(2)

		expect(savedMetadata.files_in_context[0].record_state).to.equal("stale")

		const newEntry = savedMetadata.files_in_context[1]
		expect(newEntry.record_state).to.equal("active")
		expect(newEntry.record_source).to.equal("dirac_edited")
	})

	it("should setup a file watcher for tracked files", async () => {
		await tracker.trackFileContext(filePath, "read_tool")

		expect(chokidarWatchStub.called).to.be.true
		expect(mockFileSystemWatcher.on.called).to.be.true
	})

	it("should track user edits when file watcher detects changes", async () => {
		await tracker.trackFileContext(filePath, "read_tool")

		getTaskMetadataStub.resetHistory()
		saveTaskMetadataStub.resetHistory()

		const trackFileContextSpy = sandbox.spy(tracker, "trackFileContext")

		const callback = mockFileSystemWatcher.on.firstCall.args[1]

		callback(path.resolve("/mock/workspace", filePath))

		expect(trackFileContextSpy.calledWith(filePath, "user_edited")).to.be.true

		const modifiedFiles = tracker.getAndClearRecentlyModifiedFiles()
		expect(modifiedFiles).to.include(filePath)
	})

	it("should not track Dirac edits as user edits", async () => {
		await tracker.trackFileContext(filePath, "read_tool")

		tracker.markFileAsEditedByDirac(filePath)

		getTaskMetadataStub.resetHistory()
		saveTaskMetadataStub.resetHistory()

		const trackFileContextSpy = sandbox.spy(tracker, "trackFileContext")

		const callback = mockFileSystemWatcher.on.firstCall.args[1]

		callback(path.resolve("/mock/workspace", filePath))

		expect(trackFileContextSpy.calledWith(filePath, "user_edited")).to.be.false

		const modifiedFiles = tracker.getAndClearRecentlyModifiedFiles()
		expect(modifiedFiles).to.not.include(filePath)
	})

	it("should dispose file watchers when dispose is called", async () => {
		await tracker.trackFileContext(filePath, "read_tool")

		await tracker.dispose()

		expect(mockFileSystemWatcher.close.called).to.be.true
	})
})
