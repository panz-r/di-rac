import { expect } from "chai"
import * as path from "path"
import * as fs from "fs"
import { CoordinatorClient } from "../core/locks/CoordinatorClient"
import { CommandClient } from "../services/command/CommandClient"
import { AnalyzerClient } from "../services/tree-sitter/AnalyzerClient"
import { spawn } from "child_process"

describe("Full Daemon IPC Integration", function() {
    this.timeout(30000)

    const workspaceRoot = process.cwd()
    const distDir = path.join(workspaceRoot, "dist")
    const coordBinary = path.join(distDir, "di-vrr-central-deamon")
    const cmdBinary = path.join(distDir, "di-rvv-cmd")
    const analyzerBinary = path.join(distDir, "di-rvv-analyzer")

    let coordClient: CoordinatorClient
    let cmdClient: CommandClient
    let analyzerClient: AnalyzerClient

    before(async () => {
        // Ensure binaries exist
        if (!fs.existsSync(coordBinary) || !fs.existsSync(cmdBinary) || !fs.existsSync(analyzerBinary)) {
            throw new Error("Binaries not found in dist/. Run build.sh first.")
        }

        // 1. Start Coordinator (Central Daemon)
        // We'll let the client handle spawning or assume it's running.
        // For a clean test, we'll spawn it manually if needed, but CoordinatorClient.ts has spawn logic.
        coordClient = new CoordinatorClient()
        await coordClient.initialize()

        // 2. Start Command Daemon
        cmdClient = new CommandClient(cmdBinary, workspaceRoot)
        await cmdClient.start()

        // 3. Start Analyzer Daemon
        analyzerClient = new AnalyzerClient(analyzerBinary, workspaceRoot)
        await analyzerClient.start()
    })

    after(async () => {
        await analyzerClient.shutdown()
        await cmdClient.shutdown()
        await coordClient.dispose()
    })

    it("should coordinate locks via central-deamon", async () => {
        const lockPath = "/test/path/a"
        const lock = await coordClient.acquireLock(lockPath, 5000)
        expect(lock).to.be.true

        // Try to acquire same lock from another "client" (mocked via same connection for now)
        // Note: C daemon allows multiple same-client locks if we don't distinguish by ID, 
        // but here we check if it responds.
        const lock2 = await coordClient.acquireLock(lockPath, 100)
        expect(lock2).to.be.true // Re-entrant for same client connection

        await coordClient.releaseLock(lockPath)
    })

    it("should persist and sync settings via central-deamon", async () => {
        const key = "test_key_" + Date.now()
        await coordClient.setConfig(key, "hello_world", "global")
        
        const val = await coordClient.getConfig(key)
        expect(val).to.equal("hello_world")
    })

    it("should perform threaded directory walks via command-daemon", async () => {
        const files = await cmdClient.walk(".", 10, false)
        expect(files).to.be.an("array")
        expect(files.length).to.be.greaterThan(0)
        expect(files[0]).to.have.property("path")
    })

    it("should execute commands safely via command-daemon", async () => {
        const result = await cmdClient.execute("echo 'ipc_test'")
        expect(result.stdout).to.contain("ipc_test")
    })

    it("should extract symbols via analyzer-daemon (C version)", async () => {
        const content = "def test_func():\n  pass\nclass TestClass:\n  pass"
        const symbols = await analyzerClient.outlineContent(content, "python")
        
        const names = symbols.map(s => s.name)
        expect(names).to.include("test_func")
        expect(names).to.include("TestClass")
        
        const kinds = symbols.map(s => s.kind)
        expect(kinds).to.include("function")
        expect(kinds).to.include("class")
    })

    it("should index and search symbols using SQLite in analyzer-daemon", async () => {
        // Create a temp file to index
        const tempFile = path.join(workspaceRoot, "dist", "integration_test.py")
        fs.writeFileSync(tempFile, "def unique_integration_symbol():\n  pass")
        
        try {
            // Re-start analyzer with a DB path for this test
            await analyzerClient.shutdown()
            const dbPath = path.join(workspaceRoot, "dist", "test_integration.db")
            if (fs.existsSync(dbPath)) fs.unlinkSync(dbPath)
            
            // Override spawn to include --db
            const originalSpawn = (analyzerClient as any).spawnDaemon.bind(analyzerClient)
            ;(analyzerClient as any).spawnDaemon = () => {
                const proc = spawn(analyzerBinary, ["--workspace-root", workspaceRoot, "--db", dbPath], {
                    stdio: ["pipe", "pipe", "pipe"],
                })
                ;(analyzerClient as any).process = proc
                return new Promise<void>((resolve) => {
                    proc.stderr!.on("data", (c) => { if (c.toString().includes("ready")) resolve() })
                })
            }
            await analyzerClient.start()

            // Index the file
            await analyzerClient.indexFile(tempFile)

            // Search for the symbol
            const results = await analyzerClient.searchIndex("unique_integration_symbol")
            expect(results.length).to.be.greaterThan(0)
            expect(results[0].name).to.equal("unique_integration_symbol")
            expect(results[0].file).to.equal(tempFile)

        } finally {
            if (fs.existsSync(tempFile)) fs.unlinkSync(tempFile)
        }
    })
})
