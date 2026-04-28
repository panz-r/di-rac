import { getShell } from "@utils/shell"
import { expect } from "chai"
import { afterEach, beforeEach, describe, it } from "mocha"
import { userInfo } from "os"

describe("Shell Detection Tests", () => {
	let originalPlatform: string
	let originalEnv: NodeJS.ProcessEnv
	let originalUserInfo: typeof userInfo

	beforeEach(() => {
		originalPlatform = process.platform
		originalEnv = { ...process.env }
		originalUserInfo = userInfo

		delete process.env.SHELL
		delete process.env.COMSPEC

		;(userInfo as any) = () => ({ shell: null })
	})

	afterEach(() => {
		Object.defineProperty(process, "platform", { value: originalPlatform })
		process.env = originalEnv
		;(userInfo as any) = originalUserInfo
	})

	describe("Windows Shell Detection", () => {
		beforeEach(() => {
			Object.defineProperty(process, "platform", { value: "win32" })
		})

		it("respects userInfo() shell", () => {
			;(userInfo as any) = () => ({ shell: "C:\\Custom\\PowerShell.exe" })
			expect(getShell()).to.equal("C:\\Custom\\PowerShell.exe")
		})

		it("respects COMSPEC env var", () => {
			process.env.COMSPEC = "D:\\CustomCmd\\cmd.exe"
			expect(getShell()).to.equal("D:\\CustomCmd\\cmd.exe")
		})

		it("defaults to cmd.exe if nothing is set", () => {
			expect(getShell()).to.equal("C:\\Windows\\System32\\cmd.exe")
		})
	})

	describe("macOS Shell Detection", () => {
		beforeEach(() => {
			Object.defineProperty(process, "platform", { value: "darwin" })
		})

		it("falls back to userInfo().shell", () => {
			;(userInfo as any) = () => ({ shell: "/opt/homebrew/bin/zsh" })
			expect(getShell()).to.equal("/opt/homebrew/bin/zsh")
		})

		it("falls back to SHELL env var", () => {
			process.env.SHELL = "/usr/local/bin/zsh"
			expect(getShell()).to.equal("/usr/local/bin/zsh")
		})

		it("falls back to /bin/zsh if nothing is set", () => {
			expect(getShell()).to.equal("/bin/zsh")
		})
	})

	describe("Linux Shell Detection", () => {
		beforeEach(() => {
			Object.defineProperty(process, "platform", { value: "linux" })
		})

		it("falls back to userInfo().shell", () => {
			;(userInfo as any) = () => ({ shell: "/usr/bin/zsh" })
			expect(getShell()).to.equal("/usr/bin/zsh")
		})

		it("falls back to SHELL env var", () => {
			process.env.SHELL = "/usr/bin/fish"
			expect(getShell()).to.equal("/usr/bin/fish")
		})

		it("falls back to /bin/bash if nothing is set", () => {
			expect(getShell()).to.equal("/bin/bash")
		})
	})

	describe("Unknown Platform / Error Handling", () => {
		it("falls back to /bin/sh for unknown platforms", () => {
			Object.defineProperty(process, "platform", { value: "sunos" })
			expect(getShell()).to.equal("/bin/sh")
		})

		it("handles userInfo errors gracefully, falling back to environment variable", () => {
			Object.defineProperty(process, "platform", { value: "darwin" })
			;(userInfo as any) = () => {
				throw new Error("userInfo error")
			}
			process.env.SHELL = "/bin/zsh"
			expect(getShell()).to.equal("/bin/zsh")
		})

		it("falls back fully to default shell paths if everything fails", () => {
			Object.defineProperty(process, "platform", { value: "linux" })
			;(userInfo as any) = () => {
				throw new Error("userInfo error")
			}
			delete process.env.SHELL
			expect(getShell()).to.equal("/bin/bash")
		})
	})
})
