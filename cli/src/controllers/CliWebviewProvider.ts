import { DiracWebviewProvider } from "@/core/webview"
import type { DiracExtensionContext } from "@/shared/dirac"

export class CliWebviewProvider extends DiracWebviewProvider {
	constructor(context: DiracExtensionContext) {
		super(context)
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
