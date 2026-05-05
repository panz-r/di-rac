import { Logger } from "@/shared/services/Logger"
import { DiracError } from "../DiracError"
import type { ErrorSettings, IErrorProvider } from "./IErrorProvider"

/**
 * Dirac implementation of the error provider interface
 * Handles Dirac-specific error tracking and logging
 * Telemetry backend removed - this is now a no-op provider
 */
export class DiracErrorProvider implements IErrorProvider {
	private errorSettings: ErrorSettings

	constructor(_clientConfig?: unknown) {
		this.errorSettings = {
			enabled: true,
			hostEnabled: true,
			level: "all",
		}
	}

	public async initialize(): Promise<DiracErrorProvider> {
		return this
	}

	async captureException(error: Error | DiracError, properties?: Record<string, unknown>): Promise<void> {
		if (!this.isEnabled() || this.errorSettings.level === "off") {
			return
		}
		Logger.error("[DiracErrorProvider] captureException", { error: error.message || String(error), properties })
	}

	public logException(error: Error | DiracError, properties: Record<string, unknown> = {}): void {
		if (!this.isEnabled() || this.errorSettings.level === "off") {
			return
		}
		Logger.error("[DiracErrorProvider] logException", { error: error.message || String(error), properties })
	}

	public logMessage(
		message: string,
		level: "error" | "warning" | "log" | "debug" | "info" = "log",
		properties: Record<string, unknown> = {},
	): void {
		if (!this.isEnabled() || this.errorSettings.level === "off") {
			return
		}
		if (this.errorSettings.level === "error" && level !== "error") {
			return
		}
		Logger.log("[DiracErrorProvider]", { message: message.substring(0, 500), level, properties })
	}

	public isEnabled(): boolean {
		return true
	}

	public getSettings(): ErrorSettings {
		return { ...this.errorSettings }
	}

	public async dispose(): Promise<void> {
		// No-op
	}
}
