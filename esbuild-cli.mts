import fs from "node:fs"
import { execSync } from "node:child_process"
import path from "node:path"
import { fileURLToPath } from "node:url"
import dotenv from "dotenv"
import * as esbuild from "esbuild"

const __filename = fileURLToPath(import.meta.url)
const __dirname = path.dirname(__filename)

// Load .env from repo root
dotenv.config({ path: path.join(__dirname, ".env") })

const production = process.argv.includes("--production")
const watch = process.argv.includes("--watch")

const cliDir = path.join(__dirname, "cli")
const distDir = path.join(__dirname, "dist")

/**
 * Plugin to resolve path aliases from the project
 */
const aliasResolverPlugin: esbuild.Plugin = {
	name: "alias-resolver",
	setup(build) {
		const aliases = {
			"@": path.resolve(__dirname, "src"),
			"@core": path.resolve(__dirname, "src/core"),
			"@integrations": path.resolve(__dirname, "src/integrations"),
			"@services": path.resolve(__dirname, "src/services"),
			"@shared": path.resolve(__dirname, "src/shared"),
			"@utils": path.resolve(__dirname, "src/utils"),
			"@packages": path.resolve(__dirname, "src/packages"),
			"@hosts": path.resolve(__dirname, "src/hosts"),
			"@generated": path.resolve(__dirname, "src/generated"),
			"@api": path.resolve(__dirname, "src/core/api"),
		}

		// For each alias entry, create a resolver
		Object.entries(aliases).forEach(([alias, aliasPath]) => {
			const aliasRegex = new RegExp(`^${alias}($|/.*)`)
			build.onResolve({ filter: aliasRegex }, (args) => {
				const importPath = args.path.replace(alias, aliasPath)

				// First, check if the path exists as is
				if (fs.existsSync(importPath)) {
					const stats = fs.statSync(importPath)
					if (stats.isDirectory()) {
						// If it's a directory, try to find index files
						const extensions = [".ts", ".tsx", ".js", ".jsx"]
						for (const ext of extensions) {
							const indexFile = path.join(importPath, `index${ext}`)
							if (fs.existsSync(indexFile)) {
								return { path: indexFile }
							}
						}
					} else {
						// It's a file that exists, so return it
						return { path: importPath }
					}
				}

				// If the path doesn't exist, try appending extensions
				const extensions = [".ts", ".tsx", ".js", ".jsx"]
				for (const ext of extensions) {
					const pathWithExtension = `${importPath}${ext}`
					if (fs.existsSync(pathWithExtension)) {
						return { path: pathWithExtension }
					}
				}

				// Handle .js -> .ts extension mapping (common in ESM TypeScript projects)
				if (importPath.endsWith(".js")) {
					const tsPath = importPath.replace(/\.js$/, ".ts")
					if (fs.existsSync(tsPath)) {
						return { path: tsPath }
					}
					const tsxPath = importPath.replace(/\.js$/, ".tsx")
					if (fs.existsSync(tsxPath)) {
						return { path: tsxPath }
					}
				}

				// If nothing worked, return the original path and let esbuild handle the error
				return { path: importPath }
			})
		})
	},
}

/**
 * Plugin to redirect vscode imports to our shim
 */
const vscodeStubPlugin: esbuild.Plugin = {
	name: "vscode-stub",
	setup(build) {
		// Redirect 'vscode' imports to our shim
		build.onResolve({ filter: /^vscode$/ }, () => {
			return { path: path.join(cliDir, "src", "vscode-shim.ts") }
		})
	},
}

const esbuildProblemMatcherPlugin: esbuild.Plugin = {
	name: "esbuild-problem-matcher",
	setup(build) {
		build.onStart(() => {
			console.log("[cli esbuild] Build started...")
		})
		build.onEnd((result) => {
			result.errors.forEach(({ text, location }) => {
				console.error(`✘ [ERROR] ${text}`)
				if (location) {
					console.error(`    ${location.file}:${location.line}:${location.column}:`)
				}
			})
			console.log("[cli esbuild] Build finished")
		})
	},
}

// Plugin to stub out optional devtools module
const stubOptionalModulesPlugin: esbuild.Plugin = {
	name: "stub-optional-modules",
	setup(build) {
		build.onResolve({ filter: /^react-devtools-core$/ }, () => {
			return { path: path.join(cliDir, "src", "stub-devtools.js"), external: false }
		})
	},
}

const copyWasmFiles: esbuild.Plugin = {
	name: "copy-wasm-files",
	setup(build) {
		build.onEnd(() => {
			// Ensure dist directory exists
			if (!fs.existsSync(distDir)) {
				fs.mkdirSync(distDir, { recursive: true })
			}

			// Copy sql-wasm.wasm
			const sqlJsSource = path.join(__dirname, "node_modules", "sql.js", "dist", "sql-wasm.wasm")
			if (fs.existsSync(sqlJsSource)) {
				fs.copyFileSync(sqlJsSource, path.join(distDir, "sql-wasm.wasm"))
			}

			// Copy .hash_anchors
			const dictionarySource = path.join(__dirname, "src", "utils", ".hash_anchors")
			const dictionaryTarget = path.join(distDir, ".hash_anchors")
			if (fs.existsSync(dictionarySource)) {
				fs.copyFileSync(dictionarySource, dictionaryTarget)
			}
		})
	},
}

const buildEnvVars: Record<string, string> = {
	"process.env.IS_STANDALONE": JSON.stringify("true"),
	"process.env.IS_CLI": JSON.stringify("true"),
}

const buildTimeEnvs = [
	"TELEMETRY_SERVICE_API_KEY",
	"ERROR_SERVICE_API_KEY",
	"ENABLE_ERROR_AUTOCAPTURE",
	"POSTHOG_TELEMETRY_ENABLED",
	"OTEL_TELEMETRY_ENABLED",
	"OTEL_LOGS_EXPORTER",
	"OTEL_METRICS_EXPORTER",
	"OTEL_EXPORTER_OTLP_PROTOCOL",
	"OTEL_EXPORTER_OTLP_ENDPOINT",
	"OTEL_EXPORTER_OTLP_HEADERS",
	"OTEL_METRIC_EXPORT_INTERVAL",
	"DIRAC_ENVIRONMENT",
]

buildTimeEnvs.forEach((envVar) => {
	if (process.env[envVar]) {
		console.log(`[cli esbuild] ${envVar} env var is set`)
		buildEnvVars[`process.env.${envVar}`] = JSON.stringify(process.env[envVar])
	}
})

if (production) {
	buildEnvVars["process.env.IS_DEV"] = "false"
}

// Shared build options
const sharedOptions: Partial<esbuild.BuildOptions> = {
	bundle: true,
	minify: production,
	sourcemap: !production,
	logLevel: "silent",
	define: buildEnvVars,
	tsconfig: "./tsconfig.json",
	plugins: [copyWasmFiles, aliasResolverPlugin, vscodeStubPlugin, stubOptionalModulesPlugin, esbuildProblemMatcherPlugin],
	format: "esm",
	sourcesContent: false,
	platform: "node",
	target: "node20",
	// These modules need to load files from the module directory at runtime
	external: [
		"web-tree-sitter",
		"@grpc/reflection",
		"grpc-health-check",
		"better-sqlite3",
		"ink",
		"ink-spinner",
		"ink-picture",
		"react",
		"aws4fetch",
		"pino",
		"pino-roll",
		"@vscode/ripgrep", // Uses __dirname to locate the binary
	],
	supported: { "top-level-await": true },
}

// CLI executable configuration
const cliConfig: esbuild.BuildOptions = {
	...sharedOptions,
	entryPoints: [path.join(cliDir, "src", "index.ts")],
	outfile: path.join(distDir, "cli.mjs"),
	banner: {
		js: `#!/usr/bin/env node
// Suppress all Node.js warnings (deprecation, experimental, etc.)
process.emitWarning = () => {};
import { createRequire as _createRequire } from 'module';
import { fileURLToPath as _fileURLToPath } from 'url';
import { dirname as _dirname } from 'path';
const require = _createRequire(import.meta.url);
const __filename = _fileURLToPath(import.meta.url);
const __dirname = _dirname(__filename);`,
	},
}

async function main() {
	if (watch) {
		// In watch mode, only watch the CLI (primary use case for development)
		const ctx = await esbuild.context(cliConfig)
		await ctx.watch()
		console.log("[cli] Watching for changes...")
	} else {
		// Build CLI executable
		console.log("[cli esbuild] Building CLI executable...")
		const cliCtx = await esbuild.context(cliConfig)
		await cliCtx.rebuild()
		await cliCtx.dispose()

		// Make the CLI output executable
		const cliOutfile = path.join(distDir, "cli.mjs")
		if (fs.existsSync(cliOutfile)) {
			fs.chmodSync(cliOutfile, "755")
		}

		// Build and copy the Go API gateway binary
		const gatewayDir = path.join(__dirname, "api-gateway")
		const gatewaySource = path.join(gatewayDir, "api-gateway")
		const gatewayDest = path.join(distDir, "api-gateway")
		try {
			execSync("go build -o api-gateway .", { cwd: gatewayDir, stdio: "pipe" })
			console.log("[cli esbuild] Built api-gateway binary")
			fs.copyFileSync(gatewaySource, gatewayDest)
			fs.chmodSync(gatewayDest, "755")
			console.log("[cli esbuild] Copied api-gateway binary to dist/")
		} catch (e: any) {
			console.error("[cli esbuild] WARNING: Failed to build api-gateway:", e.stderr?.toString() || e.message)
		}

		// Build command daemon (C)
		const cmdDaemonDir = path.join(__dirname, "command-daemon")
		const cmdDaemonBuild = path.join(cmdDaemonDir, "build", "dirac-cmd")
		const cmdDaemonDest = path.join(distDir, "dirac-cmd")
		try {
			execSync("cmake -B build -DCMAKE_BUILD_TYPE=Release", { cwd: cmdDaemonDir, stdio: "pipe" })
			execSync("cmake --build build", { cwd: cmdDaemonDir, stdio: "pipe" })
			fs.copyFileSync(cmdDaemonBuild, cmdDaemonDest)
			fs.chmodSync(cmdDaemonDest, "755")
			console.log("[cli esbuild] Built and copied dirac-cmd binary to dist/")
		} catch (e: any) {
			console.error("[cli esbuild] WARNING: Failed to build command daemon:", e.stderr?.toString() || e.message)
		}
		
		// Build tree-sitter analyzer daemon (Rust)
		const analyzerDir = path.join(__dirname, "treesitter-daemon")
		const analyzerSource = path.join(analyzerDir, "target", "release", "di-rvv-analyzer")
		const analyzerDest = path.join(distDir, "di-rvv-analyzer")
		try {
			execSync("cargo build --release", { cwd: analyzerDir, stdio: "pipe" })
			fs.copyFileSync(analyzerSource, analyzerDest)
			fs.chmodSync(analyzerDest, "755")
			console.log("[cli esbuild] Built and copied di-rvv-analyzer binary to dist/")
		} catch (e: any) {
			console.error("[cli esbuild] WARNING: Failed to build analyzer daemon:", e.stderr?.toString() || e.message)
		}
	}
}

main().catch((e) => {
	console.error(e)
	process.exit(1)
})