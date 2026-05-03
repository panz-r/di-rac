import { spawn, type ChildProcess } from "node:child_process"
import fs from "node:fs"
import net from "node:net"
import os from "node:os"
import path from "node:path"
import { fileURLToPath } from "node:url"
import { Logger } from "@/shared/services/Logger"

/**
 * Per-process socket path: ~/.dirac/api-gateway-{pid}.sock
 * Each CLI agent gets its own gateway process and socket.
 * Set DIRAC_API_GATEWAY_SOCKET in env so api-gateway.ts and gateway-client.ts pick it up.
 */
const INSTANCE_SOCKET_PATH = `${os.homedir()}/.dirac/api-gateway-${process.pid}.sock`
process.env.DIRAC_API_GATEWAY_SOCKET = INSTANCE_SOCKET_PATH

let gatewayProcess: ChildProcess | null = null
let gatewayBinPath: string | null = null
let cleanupRegistered = false

function findGatewayBinary(): string | null {
	// 1. Explicit env override
	if (process.env.DIRAC_API_GATEWAY_BIN) {
		if (fs.existsSync(process.env.DIRAC_API_GATEWAY_BIN)) {
			return process.env.DIRAC_API_GATEWAY_BIN
		}
	}

	// 2. Same directory as cli.mjs (dist/api-gateway)
	try {
		const cliPath = fileURLToPath(import.meta.url)
		const cliDir = path.dirname(cliPath)
		const localBin = path.join(cliDir, "api-gateway")
		if (fs.existsSync(localBin)) return localBin
	} catch {}

	return null
}

function waitForSocket(timeoutMs = 15000): Promise<void> {
	return new Promise((resolve, reject) => {
		const start = Date.now()
		const check = () => {
			if (fs.existsSync(INSTANCE_SOCKET_PATH)) {
				resolve()
				return
			}
			if (Date.now() - start > timeoutMs) {
				reject(new Error(`Gateway socket not found after ${timeoutMs}ms at ${INSTANCE_SOCKET_PATH}`))
				return
			}
			setTimeout(check, 200)
		}
		check()
	})
}

/**
 * Kill this instance's gateway process and remove its socket.
 * Safe to call multiple times.
 */
function killGateway() {
	if (!gatewayProcess) return
	try {
		gatewayProcess.kill("SIGTERM")
	} catch {}
	gatewayProcess = null
	gatewayBinPath = null
	try {
		if (fs.existsSync(INSTANCE_SOCKET_PATH)) fs.unlinkSync(INSTANCE_SOCKET_PATH)
	} catch {}
}

/**
 * Register process-level cleanup so the gateway never outlives the CLI.
 */
function registerCleanup() {
	if (cleanupRegistered) return
	cleanupRegistered = true

	process.on("beforeExit", killGateway)
	process.on("exit", killGateway)
}

export async function startApiGateway(): Promise<void> {
	const binPath = findGatewayBinary()
	if (!binPath) {
		Logger.warn("[Gateway]", "Binary not found — skipping gateway launch")
		return
	}

	Logger.info("[Gateway]", `Starting from ${binPath}`)

	gatewayBinPath = binPath
	gatewayProcess = spawn(binPath, [], {
		stdio: ["ignore", "pipe", "pipe"],
		detached: false,
		env: {
			...process.env,
			DIRAC_API_GATEWAY_SOCKET: INSTANCE_SOCKET_PATH,
		},
	})

	gatewayProcess.stdout?.on("data", (data: Buffer) => {
		Logger.info("[Gateway:stdout]", data.toString().trim())
	})

	gatewayProcess.stderr?.on("data", (data: Buffer) => {
		Logger.info("[Gateway:stderr]", data.toString().trim())
	})

	gatewayProcess.on("exit", (code, signal) => {
		Logger.warn("[Gateway]", `Exited with code=${code} signal=${signal}`)
		gatewayProcess = null
		gatewayBinPath = null
	})

	gatewayProcess.on("error", (err) => {
		Logger.error("[Gateway]", `Spawn error: ${err.message}`)
		gatewayProcess = null
		gatewayBinPath = null
	})

	await waitForSocket()
	Logger.info("[Gateway]", `Ready on ${INSTANCE_SOCKET_PATH}`)

	// Ensure gateway is killed when the CLI exits, regardless of exit path
	registerCleanup()
}

export async function stopApiGateway(): Promise<void> {
	killGateway()
}
