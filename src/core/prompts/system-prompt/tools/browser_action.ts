import { DiracDefaultTool } from "@/shared/tools"
import type { DiracToolSpec } from "../spec"

const id = DiracDefaultTool.BROWSER

export const browser_action: DiracToolSpec = {
	id,
	name: "browser_action",
	description: `Request to interact with a Puppeteer-controlled browser. Every action, except \`close\`, will be responded to with a screenshot of the browser's current state, along with any new console logs. You may only perform one browser action per message, and wait for the user's response including a screenshot and logs to determine the next action.
- The sequence of actions **must always start with** launching the browser at a URL, and **must always end with** closing the browser. If you need to visit a new URL that is not possible to navigate to from the current webpage, you must first close the browser, then launch again at the new URL.
- While the browser is active, only the \`browser_action\` tool can be used. No other tools should be called during this time. You may proceed to use other tools only after closing the browser. For example if you run into an error and need to fix a file, you must close the browser, then use other tools to make the necessary changes, then re-launch the browser to verify the result.
- The browser window has a resolution of **{{BROWSER_VIEWPORT_WIDTH}}x{{BROWSER_VIEWPORT_HEIGHT}}** pixels. When performing any click actions, ensure the coordinates are within this resolution range.
- Before clicking on any elements such as icons, links, or buttons, you must consult the provided screenshot of the page to determine the coordinates of the element. The click should be targeted at the **center of the element**, not on its edges.

Usage: browser_action <action> [--url URL] [--coordinate X,Y] [--text TEXT]

Positional:
  action              The action to perform: launch, click, type, scroll_down, scroll_up, close.

Options:
  --url URL           URL for the \`launch\` action.
  --coordinate X,Y    x,y coordinates for the \`click\` action (within viewport resolution).
  --text TEXT         Text string for the \`type\` action.

Examples:
  browser_action launch --url http://localhost:3000
  browser_action click --coordinate 450,300
  browser_action type --text "Hello, world!"
  browser_action scroll_down
  browser_action close`,
	contextRequirements: (context) => context.supportsBrowserUse === true,
	parameters: [
		{
			name: "command",
			required: true,
			type: "string",
			instruction: "CLI arguments for browser_action.",
			usage: "launch --url http://localhost:3000",
		},
	],
	metadata: {
		tags: ["browser", "web", "automation"],
		category: "browser",
		concurrency: "sequential",
		safety: ["network", "interactive"],
		outputSize: "medium",
		llmsBrief: "Perform browser automation actions",
		compactionSafety: "discardable",
	},
}
