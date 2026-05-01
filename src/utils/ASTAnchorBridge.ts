import fs from "fs/promises"
import { DiracIgnoreController } from "../core/ignore/DiracIgnoreController"
import { AnalyzerClient } from "../services/tree-sitter/AnalyzerClient"
import { FileAnchorIndex } from "../shared/utils/file-anchor-index"
import { contentHash, formatLineWithHash } from "./line-hashing"

export interface SymbolRange {
	startIndex: number
	endIndex: number
	startLine: number
	nameText: string
}

export interface GetFunctionsResult {
	formattedContent: string
	foundNames: string[]
}

export class ASTAnchorBridge {
	public static async getFileSkeleton(
		absolutePath: string,
		diracIgnoreController?: DiracIgnoreController,
		taskId?: string,
		_options?: { showCallGraph?: boolean },
		analyzer?: AnalyzerClient,
	): Promise<string | null> {
		if (!analyzer) return null

		const symbols = await analyzer.outline(absolutePath)
		if (!symbols || symbols.length === 0) return null

		const fileContent = await fs.readFile(absolutePath, "utf8")
		const lines = fileContent.split("\n")
		const anchors = new FileAnchorIndex(lines).getAllHashes()
		const definitions = AnalyzerClient.toParsedDefinitions(symbols, lines)

		let formattedOutput = ""
		let lastLineAdded = -1

		for (const def of definitions) {
			const startLine = def.lineIndex
			if (lastLineAdded !== -1 && startLine > lastLineAdded + 1) {
				formattedOutput += "|----\n"
			}
			if (startLine > lastLineAdded) {
				formattedOutput += `│${formatLineWithHash(def.text, anchors[startLine])}\n`
				lastLineAdded = startLine
			}
		}

		if (formattedOutput.length > 0) {
			return `|----\n${formattedOutput}|----\n`
		}
		return null
	}

	public static async getFunctions(
		absolutePath: string,
		relPath: string,
		functionNames: string[],
		diracIgnoreController?: DiracIgnoreController,
		_taskId?: string,
		analyzer?: AnalyzerClient,
	): Promise<GetFunctionsResult | null> {
		if (diracIgnoreController && !diracIgnoreController.validateAccess(absolutePath)) {
			return null
		}
		if (!analyzer) {
			return {
				formattedContent: `Analyzer not available for ${relPath}`,
				foundNames: [],
			}
		}

		const fileContent = await fs.readFile(absolutePath, "utf8")
		const allLines = fileContent.split(/\r?\n/)
		const allAnchors = new FileAnchorIndex(allLines).getAllHashes()

		const symbols = await analyzer.outline(absolutePath)
		const fileResults: string[] = []
		const foundNamesInFile = new Set<string>()
		const seenRanges = new Set<string>()

		for (const sym of symbols) {
			const normalizedHandle = sym.handle.replace(/::/g, ".")
			const normalizedName = sym.name.replace(/::/g, ".")
			const matchedReqNames = functionNames.filter((reqName) => {
				const normalizedReqName = reqName.replace(/::/g, ".")
				if (normalizedHandle === normalizedReqName || normalizedName === normalizedReqName) return true
				if (normalizedHandle.endsWith("." + normalizedReqName) || normalizedName.endsWith("." + normalizedReqName)) return true
				return false
			})

			if (matchedReqNames.length === 0) continue
			matchedReqNames.forEach((reqName) => foundNamesInFile.add(reqName))

			const rangeResult = await analyzer.symbolRange(absolutePath, sym.handle)
			if (!rangeResult) continue

			const rangeKey = `${rangeResult.start_byte}-${rangeResult.end_byte}`
			if (seenRanges.has(rangeKey)) continue
			seenRanges.add(rangeKey)

			const startLine = rangeResult.start_line - 1
			const defText = fileContent.slice(rangeResult.start_byte, rangeResult.end_byte)
			const defLines = defText.split(/\r?\n/)
			const defAnchors = allAnchors.slice(startLine, startLine + defLines.length)

			const ctx = await analyzer.symbolContext(absolutePath, sym.handle)
			let contextStr = ""
			if (ctx) {
				const ctxLines: string[] = []
				for (const imp of ctx.imports) ctxLines.push(imp)
				if (ctx.class_head) ctxLines.push(ctx.class_head)
				for (const prop of ctx.properties) ctxLines.push(prop)
				const sorted = ctxLines
					.map((text) => {
						const idx = allLines.indexOf(text)
						return { text, idx: idx >= 0 ? idx : Number.MAX_SAFE_INTEGER }
					})
					.sort((a, b) => a.idx - b.idx)
				let lastIdx = -1
				for (const item of sorted) {
					if (lastIdx !== -1 && item.idx > lastIdx + 1) contextStr += "...\n"
					contextStr += formatLineWithHash(item.text, allAnchors[item.idx]) + "\n"
					lastIdx = item.idx
				}
				if (contextStr) contextStr += "...\n"
			}

			const formatted = defLines.map((line, i) => formatLineWithHash(line, defAnchors[i])).join("\n")
			const funcHash = contentHash(defText)
			const fullName = sym.parent ? sym.parent.replace("class:", "") + "." + sym.name : sym.name
			fileResults.push(`${relPath}::${fullName}\n[Function Hash: ${funcHash}]\nAll Hash Anchors provided below are stable and can be used with edit_file directly.\n${contextStr}${formatted}`)
		}

		if (fileResults.length > 0) {
			return {
				formattedContent: fileResults.join("\n\n---\n\n"),
				foundNames: Array.from(foundNamesInFile),
			}
		}
		return {
			formattedContent: `None of the requested functions (${functionNames.join(", ")}) were found in ${relPath}`,
			foundNames: [],
		}
	}

	public static async getSymbolRange(
		absolutePath: string,
		symbol: string,
		type?: string,
		diracIgnoreController?: DiracIgnoreController,
		_taskId?: string,
		analyzer?: AnalyzerClient,
	): Promise<SymbolRange | null> {
		if (diracIgnoreController && !diracIgnoreController.validateAccess(absolutePath)) {
			return null
		}
		if (!analyzer) return null

		const normalizedSymbol = symbol.replace(/::/g, ".")

		const handles: string[] = []
		if (type === "class") {
			handles.push("class:" + normalizedSymbol)
		} else if (type === "function" || type === "method") {
			handles.push("fn:" + normalizedSymbol)
		}
		handles.push(normalizedSymbol)

		for (const handle of handles) {
			const result = await analyzer.symbolRange(absolutePath, handle)
			if (result) {
				return {
					startIndex: result.start_byte,
					endIndex: result.end_byte,
					startLine: result.start_line - 1,
					nameText: result.name_text,
				}
			}
		}

		return null
	}
}
