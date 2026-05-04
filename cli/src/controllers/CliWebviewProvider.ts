import { DiracWebviewProvider } from "@/core/webview"

export class CliWebviewProvider extends DiracWebviewProvider {
	constructor(_context: any) {
		super(_context)
	}

	override getWebviewUrl(path: string): string {
		return `file://${path}`
	}

	override getCspSource(): string {
		return "'self'"
	}

	override isVisible(): boolean {
		return true
	}
}
