import { DiracToolSet } from "../registry/DiracToolSet"
import { ask } from "./ask"
import { done } from "./done"
import { bash } from "./bash"
import { read } from "./read"
import { write } from "./write"
import { edit } from "./edit"
import { symbols } from "./symbols"
import { search } from "./search"
import { repo } from "./repo"
import { compact } from "./compact"
import { task } from "./task"
import { plan } from "./plan"
import { browser_action } from "./browser_action"
import { use_skill } from "./use_skill"
import { list_skills } from "./list_skills"
import { subagent } from "./subagent"
import { web_fetch } from "./web_fetch"
import { web_search } from "./web_search"
import { tools } from "./tools"
import { memory } from "./memory"
import { recall } from "./recall"

export function registerDiracToolSets(): void {
	const allTools = [
		read,
		write,
		edit,
		symbols,
		search,
		repo,
		bash,
		ask,
		done,
		compact,
		task,
		plan,
		browser_action,
		use_skill,
		list_skills,
		subagent,
		web_fetch,
		web_search,
		tools,
		memory,
		recall,
	]

	allTools.forEach((tool) => {
		DiracToolSet.register(tool)
	})
}
