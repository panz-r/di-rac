/**
 * Get the latest terminal output for mention parsing.
 * In standalone mode, this returns an empty string as terminal capture
 * is not available without a host editor integration.
 */
export async function getLatestTerminalOutput(): Promise<string> {
	// In standalone mode, we cannot capture terminal output
	// as there is no active terminal session to read from
	return ""
}