import { loadRequiredLanguageParsers } from "@/services/tree-sitter/languageParser"
import { parseFile } from "@/services/tree-sitter/index"
import path from "path"

const SUPPORTED_EXTENSIONS = new Set([
	"ts", "tsx", "js", "jsx", "py", "rs", "go", "c", "cpp", "h", "hpp",
	"java", "php", "rb", "swift", "kt",
])

const MAX_FILES = 20
const MAX_TOTAL_LINES = 500

export async function generateOutlinesForChangedFiles(
	changedFiles: string[],
	cwd: string,
): Promise<string | null> {
	const parseable = changedFiles.filter((f) => {
		const ext = f.split(".").pop()?.toLowerCase()
		return ext && SUPPORTED_EXTENSIONS.has(ext)
	})

	if (parseable.length === 0) return null

	const filesToParse = parseable.slice(0, MAX_FILES)
	const absPaths = filesToParse.map((f) => path.resolve(cwd, f))
	const languageParsers = await loadRequiredLanguageParsers(absPaths)
	const parts: string[] = ["Current outlines for modified files:"]
	let totalLines = 0

	for (let i = 0; i < absPaths.length; i++) {
		if (totalLines >= MAX_TOTAL_LINES) break

		const relPath = filesToParse[i]
		const absPath = absPaths[i]
		try {
			const definitions = await parseFile(absPath, languageParsers)
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
