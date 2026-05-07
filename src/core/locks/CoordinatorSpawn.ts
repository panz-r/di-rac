import { spawn } from "node:child_process"
import * as fs from "node:fs"
import * as path from "node:path"
import * as net from "node:net"
import os from "node:os"
import { Logger } from "@/shared/services/Logger"

/**
 * ensureCoordinatorRunning - Checks for the daemon socket and starts the daemon if missing or dead.
 */
export async function ensureCoordinatorRunning(): Promise<void> {
	const socketPath = "/tmp/di-vrr-coord.sock"
	
	if (fs.existsSync(socketPath)) {
		// Verify the daemon is actually listening
		const isAlive = await new Promise<boolean>((resolve) => {
			const socket = net.connect(socketPath)
			socket.on("connect", () => {
				socket.destroy()
				resolve(true)
			})
			socket.on("error", () => {
				resolve(false)
			})
		})

		if (isAlive) {
			return
		}
		
		Logger.info("[CoordinatorSpawn] Stale socket detected, removing...")
		try {
			fs.unlinkSync(socketPath)
		} catch {
			// Ignore
		}
	}

	Logger.info("[CoordinatorSpawn] Starting locking daemon...")

	// Persistence path
	const diracDir = path.join(os.homedir(), ".dirac")
	if (!fs.existsSync(diracDir)) fs.mkdirSync(diracDir, { recursive: true })
	const persistPath = path.join(diracDir, "daemon_state.kv")

	// Resolve binary path
	const binName = "di-vrr-central-deamon"
	const possiblePaths = [
		path.join(process.cwd(), "central-deamon", "build", binName),
		path.join(path.dirname(process.argv[1]), binName), // Next to the CLI binary
		path.join("/usr/local/bin", binName),
	]

	let binPath = ""
	for (const p of possiblePaths) {
		if (fs.existsSync(p)) {
			binPath = p
			break
		}
	}

	if (!binPath) {
		Logger.error(`[CoordinatorSpawn] Could not find ${binName} binary. Hierarchical locking will be disabled.`)
		return
	}

	Logger.info(`[CoordinatorSpawn] Spawning daemon: ${binPath} with persistence: ${persistPath}`)
	try {
		const child = spawn(binPath, ["--persist", persistPath], {
			detached: true,
			stdio: "ignore",
		})

		// Allow the child to continue running after this process exits
		child.unref()

		// Wait for the socket to appear (up to 2 seconds)
		let attempts = 0
		while (attempts < 20) {
			if (fs.existsSync(socketPath)) {
				Logger.info("[CoordinatorSpawn] Daemon started successfully")
				return
			}
			await new Promise((resolve) => setTimeout(resolve, 100))
			attempts++
		}

		Logger.error("[CoordinatorSpawn] Daemon failed to create socket within 2 seconds.")
	} catch (error) {
		Logger.error("[CoordinatorSpawn] Failed to spawn daemon:", error)
	}
}
