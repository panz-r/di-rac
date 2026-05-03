import { Logger } from "@/shared/services/Logger"
import type { Controller } from "@core/controller/index"

export const TASK_URI_PATH = "/task"

/**
 * Shared URI handler that processes both VSCode URI events and HTTP server callbacks
 */
export class SharedUriHandler {
	private static controller: Controller | null = null

	/**
	 * Set the controller instance for URI handling
	 * Called during TUI startup before the server starts
	 */
	public static setController(controller: Controller): void {
		SharedUriHandler.controller = controller
	}

	/**
	 * Get the controller instance
	 */
	private static getController(): Controller {
		if (!SharedUriHandler.controller) {
			throw new Error("SharedUriHandler: No controller set. Call setController() during startup.")
		}
		return SharedUriHandler.controller
	}

	/**
	 * Processes a URI and routes it to the appropriate handler
	 * @param url The URI to process (can be from VSCode or converted from HTTP)
	 * @returns Promise<boolean> indicating success (true) or failure (false)
	 */
	public static async handleUri(url: string): Promise<boolean> {
		const parsedUrl = new URL(url)
		const path = parsedUrl.pathname

		// Create URLSearchParams from the query string, but preserve plus signs
		// by replacing them with a placeholder before parsing
		const queryString = parsedUrl.search.slice(1) // Remove leading '?'
		const query = new URLSearchParams(queryString.replace(/\+/g, "%2B"))

		Logger.info(
			"SharedUriHandler: Processing URI:" +
				JSON.stringify({
					path: path,
					query: query,
					scheme: parsedUrl.protocol,
				}),
		)

		const controller = SharedUriHandler.getController()

		try {
			switch (path) {
				case "/openrouter": {
					const code = query.get("code")
					if (code) {
						await controller.handleOpenRouterCallback(code)
						return true
					}
					Logger.warn("SharedUriHandler: Missing code parameter for OpenRouter callback")
					return false
				}
				case "/requesty": {
					const code = query.get("code")
					if (code) {
						await controller.handleRequestyCallback(code)
						return true
					}
					Logger.warn("SharedUriHandler: Missing code parameter for Requesty callback")
					return false
				}
				case TASK_URI_PATH: {
					const prompt = query.get("prompt")
					if (prompt) {
						await controller.handleTaskCreation(prompt)
						return true
					}
					Logger.warn("SharedUriHandler: Missing prompt parameter for task creation")
					return false
				}
				case "/hicap": {
					const code = query.get("code")
					if (code) {
						await controller.handleHicapCallback(code)
						return true
					}
					Logger.warn("SharedUriHandler: Missing code parameter for Hicap callback")
					return false
				}
				default:
					Logger.warn(`SharedUriHandler: Unknown path: ${path}`)
					return false
			}
		} catch (error) {
			Logger.error("SharedUriHandler: Error processing URI:", error)
			return false
		}
	}
}
