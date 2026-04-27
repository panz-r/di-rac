import { DiracIgnoreController } from "@core/ignore/DiracIgnoreController"
import * as fs from "fs/promises"
import * as path from "path"
import Parser from "web-tree-sitter"
import { Logger } from "@/shared/services/Logger"
import { LanguageParser } from "./languageParser"

export interface ParsedDefinition {
	id: string // Structural handle, e.g., 'fn:ClassName.methodName'
	kind: string // 'function', 'class', 'method', 'interface', etc.
	name: string // Simple name of the symbol
	lineIndex: number // 0-based
	text: string // The line text where the definition starts
	indentation: string
	signature?: string // Full signature if available
	lineCount?: number
	calls?: string[]
	fullBodyRange?: {
		startLine: number
		endLine: number
		startIndex: number
		endIndex: number
	}
}

export interface ParseResult {
	definitions: ParsedDefinition[]
	imports: string[]
}

const MAX_PARSE_SIZE = 50 * 1024 // 50KB safety limit for structural parsing

export async function parseFile(
	filePath: string,
	languageParsers: LanguageParser,
	diracIgnoreController?: DiracIgnoreController,
	options?: { showCallGraph?: boolean },
): Promise<ParsedDefinition[] | null> {
	if (diracIgnoreController && !diracIgnoreController.validateAccess(filePath)) {
		return null
	}
	
	const stats = await fs.stat(filePath)
	let fileContent: string
	if (stats.size > MAX_PARSE_SIZE) {
		// Only parse the first N KB of very large files
		const buffer = Buffer.alloc(MAX_PARSE_SIZE)
		const fileHandle = await fs.open(filePath, "r")
		try {
			await fileHandle.read(buffer, 0, MAX_PARSE_SIZE, 0)
			fileContent = buffer.toString("utf8")
		} finally {
			await fileHandle.close()
		}
	} else {
		fileContent = await fs.readFile(filePath, "utf8")
	}

	const ext = path.extname(filePath).toLowerCase().slice(1)

	const result = await parseContent(fileContent, ext, languageParsers, options)
	return result?.definitions || null
}

export async function parseContent(
	fileContent: string,
	ext: string,
	languageParsers: LanguageParser,
	options?: { showCallGraph?: boolean },
): Promise<ParseResult | null> {
	const { parser, query } = languageParsers[ext] || {}
	if (!parser || !query) {
		return null
	}

	const definitions: ParsedDefinition[] = []
	const imports: string[] = []

	try {
		// Parse the file content into an Abstract Syntax Tree (AST)
		const tree = parser.parse(fileContent)
		if (!tree || !tree.rootNode) {
			return null
		}

		// Apply the query to the AST and get the captures
		const captures = query.captures(tree.rootNode)

		// Collect all defined names for the call graph
		const definedNames = new Set<string>()
		const allReferences: { node: Parser.SyntaxNode; text: string; line: number }[] = []

		// Pre-identify definition blocks for better line count accuracy
		// Use node ID to handle potential multiple captures of the same node
		const definitionNodes = new Map<number, { node: Parser.SyntaxNode; name: string }>()
		captures.forEach((capture) => {
			if (capture.name === "import") {
				imports.push(capture.node.text)
				return
			}
			// Captures that include "definition" but not "name.definition" represent the full encompassing block
			if (capture.name.includes("definition") && !capture.name.includes("name.definition")) {
				definitionNodes.set(capture.node.id, { node: capture.node, name: capture.name })
			}

			if (options?.showCallGraph) {
				if (capture.name.includes("name.definition.function") || capture.name.includes("name.definition.method")) {
					definedNames.add(capture.node.text)
				} else if (capture.name.includes("name.reference")) {
					allReferences.push({
						node: capture.node,
						text: capture.node.text,
						line: capture.node.startPosition.row,
					})
				}
			}
		})

		// Sort captures by their start position
		captures.sort((a, b) => a.node.startPosition.row - b.node.startPosition.row)

		// Split the file content into individual lines
		const lines = fileContent.split("\n")

		// Helper to find containing class for a node
		const findContainingClass = (node: Parser.SyntaxNode): string | null => {
			let current = node.parent
			while (current) {
				const entry = definitionNodes.get(current.id)
				if (entry && entry.name.includes("definition.class")) {
					// Find the name capture for this class
					const classMatch = captures.find(
						(c) => c.node.parent?.id === current?.id && c.name === "name.definition.class",
					)
					if (classMatch) return classMatch.node.text
				}
				current = current.parent
			}
			return null
		}

		// Keep track of the last line we've added to the output
		let lastLineAdded = -1

		captures.forEach((capture) => {
			const { node, name } = capture
			const startLine = node.startPosition.row

			// Only process captures that represent a definition identifier (name.definition)
			if (!name.includes("name.definition") || !lines[startLine]) {
				return
			}

			// Only add the line if it hasn't been added yet (deduplication)
			if (startLine > lastLineAdded) {
				const symbolName = node.text
				const kind = name.split(".").pop() || "unknown"
				let className = findContainingClass(node)
				
				// Don't prepend class name if the symbol itself is a class
				if (kind === "class") {
					className = null
				}

				const handle = `${getKindShorthand(kind)}:${className ? className + "." : ""}${symbolName}`

				const def: ParsedDefinition = {
					id: handle,
					kind,
					name: symbolName,
					lineIndex: startLine,
					text: lines[startLine],
					indentation: lines[startLine].match(/^\s*/)?.[0] || "",
				}
				lastLineAdded = startLine

				// Find the actual definition node (the one that encompasses the whole block)
				let definitionNode: Parser.SyntaxNode | null = null
				let current: Parser.SyntaxNode | null = node
				while (current) {
					if (definitionNodes.has(current.id)) {
						definitionNode = current
						break
					}
					current = current.parent
				}

				if (definitionNode) {
					const startRow = definitionNode.startPosition.row
					const endRow = definitionNode.endPosition.row
					const lineCount = endRow - startRow + 1

					def.lineCount = lineCount
					def.fullBodyRange = {
						startLine: startRow,
						endLine: endRow,
						startIndex: definitionNode.startIndex,
						endIndex: definitionNode.endIndex,
					}

					// Simple signature extraction: first line of the definition
					def.signature = lines[startRow].trim()

					// Add call graph if requested
					if (options?.showCallGraph) {
						if (kind === "function" || kind === "method") {
							const localCalls = new Set<string>()

							allReferences.forEach((ref) => {
								if (
									ref.line >= startRow &&
									ref.line <= endRow &&
									definedNames.has(ref.text) &&
									ref.text !== node.text
								) {
									if (isCallNode(ref.node)) {
										localCalls.add(ref.text)
									}
								}
							})

							if (localCalls.size > 0) {
								def.calls = Array.from(localCalls)
							}
						}
					}
				}
				definitions.push(def)
			}
		})
	} catch (error) {
		Logger.log(`Error parsing content: ${error}\n`)
	}

	return {
		definitions,
		imports
	}
}

function getKindShorthand(kind: string): string {
	switch (kind) {
		case "function":
			return "fn"
		case "method":
			return "fn"
		case "class":
			return "class"
		case "interface":
			return "intf"
		case "variable":
			return "var"
		case "constant":
			return "var"
		case "type":
			return "type"
		case "enum":
			return "enum"
		case "module":
			return "mod"
		default:
			return kind.slice(0, 3)
	}
}

function isCallNode(node: Parser.SyntaxNode): boolean {
	const parent = node.parent
	if (!parent) return false

	const callTypes = [
		"call",
		"call_expression",
		"method_invocation",
		"function_call_expression",
		"member_call_expression",
		"invocation_expression",
	]
	if (callTypes.includes(parent.type)) {
		return true
	}

	const memberTypes = ["member_expression", "member_access_expression", "property_access", "member_call_expression"]
	if (memberTypes.includes(parent.type)) {
		const grandParent = parent.parent
		if (grandParent && callTypes.includes(grandParent.type)) {
			return true
		}
	}

	return false
}

export async function generateSkeleton(
	fileContent: string,
	ext: string,
	languageParsers: LanguageParser,
): Promise<string> {
	const result = await parseContent(fileContent, ext, languageParsers, { showCallGraph: false })
	if (!result || result.definitions.length === 0) {
		return fileContent
	}

	const { definitions } = result

	// Filter to keep only top-level definitions OR definitions that are not contained within another definition's body
	// Actually, we want to strip all bodies, but if we strip a class body, we don't need to strip its methods separately.
	// So we keep only "outermost" definitions that have bodies.
	const bodiesToStrip = definitions
		.filter((d) => d.fullBodyRange && d.fullBodyRange.endLine > d.fullBodyRange.startLine)
		.sort((a, b) => a.fullBodyRange!.startIndex - b.fullBodyRange!.startIndex)

	const outermostBodies: ParsedDefinition[] = []
	for (const def of bodiesToStrip) {
		const isNested = outermostBodies.some(
			(outer) =>
				def.fullBodyRange!.startIndex > outer.fullBodyRange!.startIndex &&
				def.fullBodyRange!.endIndex < outer.fullBodyRange!.endIndex,
		)
		if (!isNested) {
			outermostBodies.push(def)
		}
	}

	// Process from bottom up to maintain indices
	outermostBodies.sort((a, b) => b.fullBodyRange!.startIndex - a.fullBodyRange!.startIndex)

	let skeleton = fileContent
	const placeholder = getPlaceholder(ext)

	for (const def of outermostBodies) {
		const range = def.fullBodyRange!
		const lines = fileContent.split("\n")
		const firstLine = lines[range.startLine]
		
		const replacement = `${firstLine}\n${def.indentation}    ${placeholder}`
		
		skeleton = skeleton.slice(0, range.startIndex) + 
				   replacement + 
				   skeleton.slice(range.endIndex)
	}

	return skeleton
}

function getPlaceholder(ext: string): string {
	switch (ext) {
		case "py":
			return "pass # ..."
		case "rs":
		case "go":
		case "c":
		case "cpp":
		case "cs":
		case "java":
		case "js":
		case "ts":
		case "tsx":
			return "{ ... }"
		case "rb":
			return "# ..."
		default:
			return "... (body stripped)"
	}
}
