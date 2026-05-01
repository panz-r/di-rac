import { AnalyzerClient } from "@/services/tree-sitter/AnalyzerClient"
import path from "path"
import * as fs from "fs/promises"

const SUPPORTED_EXTENSIONS = new Set([
	"ts", "tsx", "js", "jsx", "py", "rs", "go", "c", "cpp", "h", "hpp",
	"java", "php", "rb", "swift", "kt",
])

const MAX_FILES = 20
const MAX_TOTAL_LINES = 500

export async function generateOutlinesForChangedFiles(
	changedFiles: string[],
	cwd: string,
	analyzer?: { outline: (filePath: string) => Promise<any[]> },
): Promise<string | null> {
	const parseable = changedFiles.filter((f) => {
		const ext = f.split(".").pop()?.toLowerCase()
		return ext && SUPPORTED_EXTENSIONS.has(ext)
	})

	if (parseable.length === 0) return null

	const filesToParse = parseable.slice(0, MAX_FILES)
	const absPaths = filesToParse.map((f) => path.resolve(cwd, f))

	// If no analyzer provided, skip outline generation
	if (!analyzer) return null

	const parts: string[] = ["Current outlines for modified files:"]
	let totalLines = 0

	for (let i = 0; i < absPaths.length; i++) {
		if (totalLines >= MAX_TOTAL_LINES) break

		const relPath = filesToParse[i]
		const absPath = absPaths[i]
		try {
			const daemonSymbols = await analyzer.outline(absPath)
			const fileContent = await fs.readFile(absPath, "utf8")
			const sourceLines = fileContent.split("\n")
			const definitions = AnalyzerClient.toParsedDefinitions(daemonSymbols, sourceLines)
			if (!definitions || definitions.length === 0) continue

			const fileParts: string[] = []
			for (const def of definitions) {
				if (totalLines + fileParts.length >= MAX_TOTAL_LINES) break
				const loc = def.fullBodyRange
					? ` (lines ${def.fullBodyRange.startLine + 1}-${def.fullBodyRange.endLine + 1})`
					: ` (line ${def.lineIndex + 1})`
				const sig = def.signature || def.text
				fileParts.push(`  - [${def.kind}:${def.name}] ${sig}${loc}`)
			}

			if (fileParts.length > 0) {
				parts.push(`${relPath} (${definitions.length} definitions):`)
				parts.push(...fileParts)
				totalLines += fileParts.length + 1
			}
		} catch {
			// Skip files that fail to parse
		}
	}

	return parts.length > 1 ? parts.join("\n") : null
}
