export { ObserverOrchestrator } from "./ObserverOrchestrator"
export type { ObserverConfig, ObservationEntry } from "./ObserverConfig"
export type { PrepareContextResult } from "./ObserverOrchestrator"

// Module-level observer health state, readable from TUI without task reference
export let observerFailing = false
export let observerLastError: string | undefined

export function setObserverHealth(failing: boolean, lastError?: string) {
	observerFailing = failing
	observerLastError = lastError
}
