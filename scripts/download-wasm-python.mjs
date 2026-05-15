#!/usr/bin/env node

/**
 * Download pre-compiled Python WASM binary for WasmEdge
 */

import { exec } from "child_process"
import fs from "fs"
import https from "https"
import path from "path"
import { pipeline } from "stream/promises"
import { promisify } from "util"

const execAsync = promisify(exec)

const PYTHON_WASM_URL = "https://github.com/vmware-labs/webassembly-language-runtimes/releases/download/python%2F3.11.1%2B20230127-c8036b4/python-aio-3.11.1.zip"
const OUTPUT_DIR = "standalone/runtime-files"
const ZIP_PATH = path.join(OUTPUT_DIR, "python.zip")

async function downloadFile(url, destPath) {
    return new Promise((resolve, reject) => {
        console.log(`  Downloading: ${url}`)
        const file = fs.createWriteStream(destPath)

        https.get(url, (response) => {
            if (response.statusCode === 302 || response.statusCode === 301) {
                return downloadFile(response.headers.location, destPath).then(resolve).catch(reject)
            }

            if (response.statusCode !== 200) {
                reject(new Error(`Failed to download: ${response.statusCode} ${response.statusMessage}`))
                return
            }

            response.pipe(file)
            file.on("finish", () => {
                file.close()
                resolve()
            })
        }).on("error", (err) => {
            fs.unlink(destPath, () => {})
            reject(err)
        })
    })
}

async function main() {
    console.log("🚀 Downloading Python WASM (AIO)...")
    if (!fs.existsSync(OUTPUT_DIR)) {
        fs.mkdirSync(OUTPUT_DIR, { recursive: true })
    }

    try {
        await downloadFile(PYTHON_WASM_URL, ZIP_PATH)
        console.log("  ✓ Downloaded")

        console.log("  Extracting...")
        await execAsync(`unzip -o -q "${ZIP_PATH}" -d "${OUTPUT_DIR}"`)
        console.log("  ✓ Extracted")

        // Clean up
        fs.unlinkSync(ZIP_PATH)
        
        // Find python-wasmedge.wasm
        const wasmPath = path.join(OUTPUT_DIR, "bin/python-3.11.1-wasmedge.wasm")
        if (fs.existsSync(wasmPath)) {
            fs.renameSync(wasmPath, path.join(OUTPUT_DIR, "python.wasm"))
            console.log("  ✓ python.wasm ready")
        } else {
            console.log("  ! python-3.11.1-wasmedge.wasm not found in bin/")
        }

        console.log("\n✅ Python WASM bundled successfully!")
    } catch (error) {
        console.error("  ✗ Failed:", error.message)
        process.exit(1)
    }
}

main()
