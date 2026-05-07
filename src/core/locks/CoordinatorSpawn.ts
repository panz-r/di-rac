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
			socket.setTimeout(1000)
			socket.on("connect", () => {
				socket.destroy()
				resolve(true)
			})
			socket.on("error", () => {
				resolve(false)
			})
			socket.on("timeout", () => {
				socket.destroy()
				resolve(false)
			})
		})

		if (isAlive) {
			return
		}
		
		Logger.info("CoordinatorSpawn", "Stale socket detected, removing...")
		try {
			fs.unlinkSync(socketPath)
		} catch {
			// Ignore
		}
	}

	Logger.info("CoordinatorSpawn", "Starting locking daemon...")

	// Persistence path
	const diracDir = path.join(os.homedir(), ".dirac")
	if (!fs.existsSync(diracDir)) fs.mkdirSync(diracDir, { recursive: true })
	const persistPath = path.join(diracDir, "daemon_state.kv")

	// Resolve binary path
	const binName = "di-vrr-central-deamon"
	const possiblePaths = [
		// 1. Next to the CLI binary (standard for bundled installs)
		path.join(path.dirname(process.argv[1]), binName),
		// 2. In the dist folder relative to CWD
		path.join(process.cwd(), "dist", binName),
		// 3. In the build folder (development)
		path.join(process.cwd(), "central-deamon", "build", binName),
		// 4. Global path
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
		Logger.error("CoordinatorSpawn", `Could not find ${binName} binary. searched in: ${possiblePaths.join(", ")}`)
		return
	}

	Logger.info("CoordinatorSpawn", `Spawning daemon: ${binPath} with persistence: ${persistPath}`)
	try {
		// We use a log file for the daemon if it fails to start
		const logPath = path.join(diracDir, "daemon.log")
		const logStream = fs.openSync(logPath, "a")

		const child = spawn(binPath, ["--persist", persistPath], {
			detached: true,
			stdio: ["ignore", logStream, logStream],
		})

		// Allow the child to continue running after this process exits
		child.unref()
		fs.closeSync(logStream)

		// Wait for the socket to appear (up to 5 seconds)
		let attempts = 0
		while (attempts < 50) {
			if (fs.existsSync(socketPath)) {
				// Final check: can we connect?
				const canConnect = await new Promise<boolean>((resolve) => {
					const s = net.connect(socketPath)
					s.on("connect", () => { s.destroy(); resolve(true) })
					s.on("error", () => resolve(false))
					setTimeout(() => { s.destroy(); resolve(false) }, 200)
				})
				if (canConnect) {
					Logger.info("CoordinatorSpawn", "Daemon started and responsive")
					return
				}
			}
			await new Promise((resolve) => setTimeout(resolve, 100))
			attempts++
		}

		Logger.error("CoordinatorSpawn", `Daemon failed to become responsive within 5 seconds. Check ${logPath} for details.`)
	} catch (error) {
		Logger.error("CoordinatorSpawn", `Failed to spawn daemon: ${error}`)
	}
}
