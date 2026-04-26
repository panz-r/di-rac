import { formatResponse } from "@core/prompts/responses"

import { extractSymbolLikeStrings } from "@core/context/instructions/user-instructions/rule-conditionals"
import { getOrDiscoverSkills } from "../context/instructions/user-instructions/skills"
import { SkillMetadata } from "@/shared/skills"
import { listFiles } from "@services/glob/list-files"

import { parseMentions } from "@core/mentions"
import { parseSlashCommands } from "@core/slash-commands"
import { mentionRegexGlobal } from "@shared/context-mentions"
import { GlobalFileNames } from "@core/storage/disk"
import { resolveWorkspacePath } from "@core/workspace"
import { isMultiRootEnabled } from "@core/workspace/multi-root-utils"
import { USER_CONTENT_TAGS } from "@shared/messages/constants"
import { DiracContent, DiracTextContentBlock } from "@shared/messages/content"
import { ASTAnchorBridge } from "@utils/ASTAnchorBridge"
import { getBuddyPaths, isLocatedInWorkspace } from "@utils/path"
import * as fs from "fs/promises"
import * as path from "path"
import { SymbolIndexService, SymbolLocation } from "../../services/symbol-index/SymbolIndexService"
import { ensureLocalDiracDirExists } from "../context/instructions/user-instructions/rule-helpers"
import { refreshWorkflowToggles } from "../context/instructions/user-instructions/workflows"

import { ContextLoaderDependencies } from "./types/context-loader"

// Thresholds for automatic symbol enrichment
const MAX_AUTO_SYMBOL_MATCHES = 3
const MAX_AUTO_SYMBOL_TOTAL_LINES = 20
const MAX_AUTO_SYMBOL_LINE_LENGTH_BYTES = 200

export class ContextLoader {
    constructor(private dependencies: ContextLoaderDependencies) { }

    private async extractContext(
        text: string,
        cwd: string,
    ): Promise<{ filePaths: string[]; directoryPaths: string[]; symbols: string[] }> {
        // Collect all regions that need to be scrubbed to avoid false positives
        const scrubRegions: { start: number; end: number }[] = []

        const addRegion = (start: number, end: number) => {
            scrubRegions.push({ start, end })
        }

        // 0) Identify code fences and URLs
        const fenceRegex = /```[\s\S]*?```/g
        let match: RegExpExecArray | null
        while ((match = fenceRegex.exec(text)) !== null) {
            addRegion(match.index, match.index + match[0].length)
        }

        const urlRegex = /\b\w+:\/\/[^\s]+/g
        while ((match = urlRegex.exec(text)) !== null) {
            addRegion(match.index, match.index + match[0].length)
        }

        // 1) Mentions
        while ((match = mentionRegexGlobal.exec(text)) !== null) {
            addRegion(match.index, match.index + match[0].length)
        }

        // 2) Slash commands
        const slashCommandInTextRegex = /(^|\s)\/([a-zA-Z0-9_.:@-]+)(?=\s|$)/g
        while ((match = slashCommandInTextRegex.exec(text)) !== null) {
            const prefix = match[1]
            addRegion(match.index + prefix.length, match.index + match[0].length)
        }

        // Create a version of text where identified regions are replaced with spaces for path matching
        // We use a high-performance array join pattern
        const getScrubbedText = (regions: { start: number; end: number }[]) => {
            if (regions.length === 0) return text
            const sorted = regions.slice().sort((a, b) => a.start - b.start)
            const pieces: string[] = []
            let lastIdx = 0
            for (const r of sorted) {
                if (r.start < lastIdx) continue // Skip overlapping
                pieces.push(text.substring(lastIdx, r.start))
                pieces.push(" ".repeat(r.end - r.start))
                lastIdx = r.end
            }
            pieces.push(text.substring(lastIdx))
            return pieces.join("")
        }

        let scrubbedText = getScrubbedText(scrubRegions)

        const filePaths: string[] = []
        const directoryPaths: string[] = []
        const pathRegex =
            /(?:^|[\s([{"'`])((?:\/?[A-Za-z0-9_.-]+(?:\/[A-Za-z0-9_.-]*)+\/?|[A-Za-z0-9_.-]*\.[A-Za-z0-9_-]+|\.\.\/?|\.\/|\.\.))(?=$|[\s)\]}"'`,.;:!?])/g

        const getPathMatches = (currentText: string) => {
            const matches: { relPath: string; start: number; end: number }[] = []
            let match: RegExpExecArray | null
            pathRegex.lastIndex = 0
            while ((match = pathRegex.exec(currentText)) !== null) {
                let relPath = match[1]
                const start = match.index + match[0].indexOf(relPath)

                while (relPath.length > 0 && /[,.;:!?\-]$/.test(relPath)) {
                    if (relPath === "." || relPath === "..") break
                    relPath = relPath.slice(0, -1)
                }

                if (relPath) {
                    matches.push({ relPath, start, end: start + relPath.length })
                }
            }
            return matches
        }

        // 3) File Paths
        const fileCandidates = getPathMatches(scrubbedText)
        const pathRegions: { start: number; end: number }[] = []

        for (const pc of fileCandidates) {
            try {
                const pathResult = resolveWorkspacePath(
                    {
                        cwd: cwd,
                        workspaceManager: this.dependencies.workspaceManager,
                        isMultiRootEnabled: isMultiRootEnabled(this.dependencies.stateManager),
                    },
                    pc.relPath,
                    "Task.loadContext.context",
                )
                const absolutePath = typeof pathResult === "string" ? pathResult : pathResult.absolutePath
                const stats = await fs.stat(absolutePath)
                if (stats.isFile()) {
                    filePaths.push(pc.relPath)
                    pathRegions.push({ start: pc.start, end: pc.end })

                    // Buddy heuristic: proactively find related header/source files (e.g. .h for .c)
                    // These are highly relevant context in C/C++ projects
                    const buddies = getBuddyPaths(pc.relPath)
                    for (const buddy of buddies) {
                        try {
                            const buddyResult = resolveWorkspacePath(
                                {
                                    cwd: cwd,
                                    workspaceManager: this.dependencies.workspaceManager,
                                    isMultiRootEnabled: isMultiRootEnabled(this.dependencies.stateManager),
                                },
                                buddy,
                                "Task.loadContext.context",
                            )
                            const buddyAbsPath = typeof buddyResult === "string" ? buddyResult : buddyResult.absolutePath
                            const buddyStats = await fs.stat(buddyAbsPath)
                            if (buddyStats.isFile() && (await isLocatedInWorkspace(buddyAbsPath))) {
                                filePaths.push(buddy)
                            }
                        } catch {
                            // Buddy file not found, skip
                        }
                    }
                }
            } catch (e: any) {
                // Ignore errors for individual paths
            }
        }

        // 4) Directory Paths (re-scan scrubbed text to catch paths that might have been partially shadowed)
        scrubbedText = getScrubbedText([...scrubRegions, ...pathRegions])
        const dirCandidates = getPathMatches(scrubbedText)

        for (const pc of dirCandidates) {
            try {
                const pathResult = resolveWorkspacePath(
                    {
                        cwd: cwd,
                        workspaceManager: this.dependencies.workspaceManager,
                        isMultiRootEnabled: isMultiRootEnabled(this.dependencies.stateManager),
                    },
                    pc.relPath,
                    "Task.loadContext.context",
                )
                const absolutePath = typeof pathResult === "string" ? pathResult : pathResult.absolutePath
                const stats = await fs.stat(absolutePath)
                if (stats.isDirectory()) {
                    directoryPaths.push(pc.relPath)
                    pathRegions.push({ start: pc.start, end: pc.end })
                }
            } catch (e: any) {
                // Ignore errors
            }
        }

        // 5) Symbols (final scan on fully scrubbed text)
        const finalScrubbedText = getScrubbedText([...scrubRegions, ...pathRegions])
        const symbols = extractSymbolLikeStrings(finalScrubbedText)

        return { filePaths, directoryPaths, symbols }
    }

    private async getPathContext(
        filePaths: string[],
        directoryPaths: string[],
        cwd: string,
    ): Promise<{ skeletons: string[]; directoryLists: string[] }> {
        const skeletons: string[] = []
        const directoryLists: string[] = []

        if (filePaths.length > 0 || directoryPaths.length > 0) {
            // Process files
            const seenFiles = new Set<string>()
            for (const relPath of filePaths) {
                if (seenFiles.has(relPath)) continue
                seenFiles.add(relPath)

                try {
                    const pathResult = resolveWorkspacePath(
                        {
                            cwd: cwd,
                            workspaceManager: this.dependencies.workspaceManager,
                            isMultiRootEnabled: isMultiRootEnabled(this.dependencies.stateManager),
                        },
                        relPath,
                        "Task.loadContext.context",
                    )
                    const absolutePath = typeof pathResult === "string" ? pathResult : pathResult.absolutePath

                    // --- Skeleton Logic ---
                    const skeleton = await ASTAnchorBridge.getFileSkeleton(
                        absolutePath,
                        this.dependencies.diracIgnoreController,
                        this.dependencies.ulid,
                        { showCallGraph: true },
                    )
                    if (skeleton && !skeleton.includes("Unsupported file type")) {
                        skeletons.push(`<file_skeleton path="${relPath}">\n${skeleton}\n</file_skeleton>`)
                    }
                } catch (error) {
                    // Ignore errors for individual files
                }
            }

            // Process directories
            const seenDirs = new Set<string>()
            let directoryCount = 0
            for (const relPath of directoryPaths) {
                if (seenDirs.has(relPath)) continue
                seenDirs.add(relPath)

                if (directoryCount >= 3) break

                try {
                    const pathResult = resolveWorkspacePath(
                        {
                            cwd: cwd,
                            workspaceManager: this.dependencies.workspaceManager,
                            isMultiRootEnabled: isMultiRootEnabled(this.dependencies.stateManager),
                        },
                        relPath,
                        "Task.loadContext.context",
                    )
                    const absolutePath = typeof pathResult === "string" ? pathResult : pathResult.absolutePath

                    const [fileInfos, didHitLimit] = await listFiles(absolutePath, false, 30)
                    const result = formatResponse.formatFilesList(
                        absolutePath,
                        fileInfos,
                        didHitLimit,
                        this.dependencies.diracIgnoreController,
                    )
                    const note = `Note: The following context was automatically included because the directory "${relPath}" was mentioned in user's message.`
                    directoryLists.push(`<directory_list path="${relPath}">\n${note}\n\n${result}\n</directory_list>`)
                    directoryCount++
                } catch (error) {
                    // Ignore errors
                }
            }
        }

        return { skeletons, directoryLists }
    }

    private async getSymbolContext(symbols: string[], cwd: string, filePaths: string[]): Promise<string[]> {
        const symbolDefinitions: string[] = []
        if (symbols.length > 0 && symbols.length <= MAX_AUTO_SYMBOL_MATCHES) {
            const indexService = SymbolIndexService.getInstance()
            const projectRoot = indexService.getProjectRoot() || cwd
            let totalLinesAdded = 0

            const symbolResults = new Map<
                string,
                {
                    allLocations: SymbolLocation[]
                    addedLines: string[]
                    seenLocations: Set<string>
                }
            >()

            const fileLinesCache = new Map<string, string[]>()

            for (const symbol of symbols) {
                symbolResults.set(symbol, {
                    allLocations: [],
                    addedLines: [],
                    seenLocations: new Set<string>(),
                })
            }

            const processLocation = async (symbol: string, loc: SymbolLocation, filePaths: string[]) => {
                const data = symbolResults.get(symbol)!
                const locKey = `${loc.path}:${loc.startLine}`
                if (data.seenLocations.has(locKey)) return false

                try {
                    const absLocPath = path.isAbsolute(loc.path) ? loc.path : path.join(projectRoot, loc.path)
                    let lines = fileLinesCache.get(absLocPath)

                    if (!lines) {
                        const fileContent = await fs.readFile(absLocPath, "utf8")
                        lines = fileContent.split(/\r?\n/)
                        fileLinesCache.set(absLocPath, lines)
                    }

                    const lineIndex = loc.startLine
                    if (lineIndex >= 0 && lineIndex < lines.length) {
                        let lineContent = lines[lineIndex].trim()
                        if (Buffer.byteLength(lineContent, "utf8") > MAX_AUTO_SYMBOL_LINE_LENGTH_BYTES) {
                            lineContent = "(line too long, skipped)"
                        }

                        const relLocPath = path.relative(cwd, absLocPath)
                        const pointer = `    - ${relLocPath}:${lineIndex + 1} [${loc.type}] \`${lineContent}\``
                        data.addedLines.push(pointer)
                        data.seenLocations.add(locKey)

                        // Dependency heuristic: find files that include this file or that this file includes
                        // and proactively provide their skeletons if relevant.
                        // (Limited to C/C++ for now via the dependencies table)
                        const deps = indexService.getDependencies(loc.path)
                        for (const dep of deps) {
                            if (filePaths.indexOf(dep) === -1) {
                                try {
                                    const depResult = resolveWorkspacePath(
                                        {
                                            cwd: cwd,
                                            workspaceManager: this.dependencies.workspaceManager,
                                            isMultiRootEnabled: isMultiRootEnabled(this.dependencies.stateManager),
                                        },
                                        dep,
                                        "Task.loadContext.context",
                                    )
                                    const depAbsPath = typeof depResult === "string" ? depResult : depResult.absolutePath
                                    if (await isLocatedInWorkspace(depAbsPath)) {
                                        filePaths.push(dep)
                                    }
                                } catch {
                                    // Ignore if can't resolve
                                }
                            }
                        }

                        return true
                    }
                } catch (error: any) {
                    // Ignore errors for individual symbols
                }
                return false
            }

            // Pass 1: Definitions
            for (const symbol of symbols) {
                if (totalLinesAdded >= MAX_AUTO_SYMBOL_TOTAL_LINES) break
                const definitions = indexService.getDefinitions(symbol, MAX_AUTO_SYMBOL_TOTAL_LINES)
                const data = symbolResults.get(symbol)!
                data.allLocations.push(...definitions)

                for (const loc of definitions) {
                    if (totalLinesAdded >= MAX_AUTO_SYMBOL_TOTAL_LINES) break
                    if (await processLocation(symbol, loc, filePaths)) {
                        totalLinesAdded++
                    }
                }
            }

            // Pass 2: Declarations (Prototypes)
            for (const symbol of symbols) {
                if (totalLinesAdded >= MAX_AUTO_SYMBOL_TOTAL_LINES) break
                const remainingLimit = MAX_AUTO_SYMBOL_TOTAL_LINES - totalLinesAdded
                const declarations = indexService.getSymbols(symbol, "declaration", remainingLimit)
                const data = symbolResults.get(symbol)!
                data.allLocations.push(...declarations)

                for (const loc of declarations) {
                    if (totalLinesAdded >= MAX_AUTO_SYMBOL_TOTAL_LINES) break
                    if (await processLocation(symbol, loc, filePaths)) {
                        totalLinesAdded++
                    }
                }
            }

            // Pass 3: References
            for (const symbol of symbols) {
                if (totalLinesAdded >= MAX_AUTO_SYMBOL_TOTAL_LINES) break
                const remainingLimit = MAX_AUTO_SYMBOL_TOTAL_LINES - totalLinesAdded
                const references = indexService.getReferences(symbol, remainingLimit)
                const data = symbolResults.get(symbol)!
                data.allLocations.push(...references)

                for (const loc of references) {
                    if (totalLinesAdded >= MAX_AUTO_SYMBOL_TOTAL_LINES) break
                    if (await processLocation(symbol, loc, filePaths)) {
                        totalLinesAdded++
                    }
                }
            }

            // Assemble final context strings
            for (const symbol of symbols) {
                const data = symbolResults.get(symbol)!
                if (data.addedLines.length === 0) continue

                const symbolLines: string[] = []
                const numLocations = data.allLocations.length

                symbolLines.push(
                    `Note: The following context was automatically included because the symbol "${symbol}" was mentioned in user's message.`,
                )

                if (numLocations <= MAX_AUTO_SYMBOL_TOTAL_LINES) {
                    symbolLines.push(`All ${numLocations} symbols found in the codebase are listed below.`)
                } else {
                    symbolLines.push(`${MAX_AUTO_SYMBOL_TOTAL_LINES} out of ${numLocations} symbol listed below (definitions first).`)
                }

                symbolLines.push(`symbol_context:`)
                symbolLines.push(`  ${symbol}:`)
                symbolLines.push(...data.addedLines)

                symbolDefinitions.push(symbolLines.join("\n"))
            }
        }
        return symbolDefinitions
    }

    private async enrichContext(
        text: string,
        cwd: string,
        localWorkflowToggles: any,
        globalWorkflowToggles: any,
        ulid: string,
        providerInfo: any,
        includePathContext: boolean,
        availableSkills: SkillMetadata[]
    ): Promise<{ enrichedText: string; needsDiracrulesFileCheck: boolean }> {
        const parsedText = await parseMentions(
            text,
            cwd,
            this.urlContentFetcher,
            this.fileContextTracker,
            this.workspaceManager,
        )

        const { processedText, needsDiracrulesFileCheck } = await parseSlashCommands(
            parsedText,
            localWorkflowToggles,
            globalWorkflowToggles,
            ulid,
            providerInfo,
            availableSkills
        )

        // Skip automatic path and symbol detection for subsequent turns
        if (!includePathContext) {
            return { enrichedText: processedText, needsDiracrulesFileCheck }
        }

        const { filePaths, directoryPaths, symbols } = await this.extractContext(text, cwd)
        const { skeletons, directoryLists } = await this.getPathContext(filePaths, directoryPaths, cwd)
        const symbolDefinitions = await this.getSymbolContext(symbols, cwd, filePaths)

        const additionalContext: string[] = []
        if (skeletons.length > 0) additionalContext.push(...skeletons)
        if (directoryLists.length > 0) additionalContext.push(...directoryLists)
        if (symbolDefinitions.length > 0) additionalContext.push(...symbolDefinitions)

        if (additionalContext.length > 0) {
            return {
                enrichedText: `${processedText}\n\n${additionalContext.join("\n\n")}`,
                needsDiracrulesFileCheck,
            }
        }

        return { enrichedText: processedText, needsDiracrulesFileCheck }
    }
    async loadContext(
        userContent: DiracContent[],
        includeFileDetails = false,
        useCompactPrompt = false,
    ): Promise<[DiracContent[], string, boolean, SkillMetadata[]]> {
        let needsDiracrulesFileCheck = false

        // Pre-fetch necessary data to avoid redundant calls within loops
        const ulid = this.dependencies.ulid
        const providerInfo = this.dependencies.getCurrentProviderInfo()
        const cwd = this.dependencies.cwd
        const { localWorkflowToggles, globalWorkflowToggles } = await refreshWorkflowToggles(this.dependencies.controller, cwd)

        // Discover and filter skills
        const resolvedSkills = await getOrDiscoverSkills(cwd, this.dependencies.taskState)
        const globalSkillsToggles = this.dependencies.stateManager.getGlobalSettingsKey("globalSkillsToggles") ?? {}
        const localSkillsToggles = this.dependencies.stateManager.getWorkspaceStateKey("localSkillsToggles") ?? {}
        const availableSkills = resolvedSkills.filter((skill) => {
            const toggles = skill.source === "global" ? globalSkillsToggles : localSkillsToggles
            return toggles[skill.path] !== false
        })
        this.dependencies.taskState.availableSkills = availableSkills

        const hasUserContentTag = (text: string): boolean => {
            return USER_CONTENT_TAGS.some((tag: string) => text.includes(tag))
        }

        const parseTextBlock = async (text: string): Promise<string> => {
            const { enrichedText, needsDiracrulesFileCheck: needsCheck } = await this.enrichContext(
                text,
                cwd,
                localWorkflowToggles,
                globalWorkflowToggles,
                ulid,
                providerInfo,
                includeFileDetails,
                availableSkills
            )

            if (needsCheck) {
                needsDiracrulesFileCheck = true
            }

            return enrichedText
        }

        const processTextContent = async (block: DiracTextContentBlock): Promise<DiracTextContentBlock> => {
            if (block.type !== "text" || !hasUserContentTag(block.text)) {
                return block
            }

            const processedText = await parseTextBlock(block.text)
            return { ...block, text: processedText }
        }

        const processContentBlock = async (block: DiracContent): Promise<DiracContent> => {
            if (block.type === "text") {
                return processTextContent(block)
            }

            if (block.type === "tool_result") {
                if (!block.content) {
                    return block
                }

                // Handle string content
                if (typeof block.content === "string") {
                    // Check if this is likely a read_file result or other tool output that shouldn't be processed
                    // for mentions (e.g. source code containing @ or path-like strings).
                    // ReadFileToolHandler results typically start with "[File Hash:" or "--- path ---".
                    const isLikelyToolOutput = block.content.includes("[File Hash:") || block.content.includes("--- ")

                    if (isLikelyToolOutput) {
                        return block
                    }

                    const processed = await processTextContent({ type: "text", text: block.content })
                    // Creates NEW object and turns the string content as array
                    return { ...block, content: [processed] }
                }

                // Handle array content
                if (Array.isArray(block.content)) {
                    const processedContent = await Promise.all(
                        block.content.map(async (contentBlock: any) => {
                            if (contentBlock.type === "text") {
                                // Check if this specific text block is likely tool output (e.g. from read_file)
                                const isLikelyToolOutput =
                                    contentBlock.text.includes("[File Hash:") || contentBlock.text.includes("--- ")
                                if (isLikelyToolOutput) {
                                    return contentBlock
                                }
                                return processTextContent(contentBlock)
                            }
                            return contentBlock
                        }),
                    )

                    return { ...block, content: processedContent as any }
                }
            }
            return block
        }

        // Process all content and environment details in parallel
        const [processedUserContent, environmentDetails] = await Promise.all([
            Promise.all(userContent.map(processContentBlock)),
            this.dependencies.getEnvironmentDetails(includeFileDetails),
        ])

        // Check diracrulesData if needed
        const diracrulesError = needsDiracrulesFileCheck
            ? await ensureLocalDiracDirExists(this.dependencies.cwd, GlobalFileNames.diracRules)
            : false

        return [processedUserContent, environmentDetails, diracrulesError, availableSkills]
    }

    private get urlContentFetcher() {
        return this.dependencies.urlContentFetcher
    }

    private get fileContextTracker() {
        return this.dependencies.fileContextTracker
    }

    private get workspaceManager() {
        return this.dependencies.workspaceManager
    }
}
