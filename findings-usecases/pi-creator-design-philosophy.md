# Pi Creator's Philosophy — Condensed

**Mario Zechner's first-person design rationale. "If I don't need it, it won't be built."**

## Intentional Omissions

| Feature | Why Not | Alternative |
|---------|---------|-------------|
| Plan mode | "Telling the agent to think is generally sufficient" | `PLAN.md` files |
| MCP | "MCP dumps 13.7k tokens of tools into every session" | CLI tools with READMEs |
| Background bash | "tmux gives you full observability" | tmux |
| Sub-agents | "Black box within a black box, zero visibility" | Separate sessions |
| Built-in todos | "To-do lists confuse models more than help" | `TODO.md` files |
| Security theater | "If agent can read/write/run, game over" | Run in container |
| Max steps | "I never found a use case" | Loop until done |

## What Pi Does Have

| Feature | Reasoning |
|---------|-----------|
| ~1000 token system prompt | "Models are RL-trained, they know what to do" |
| 4 tools (read, write, edit, bash) | "All you need for an effective coding agent" |
| Cross-provider handoff | Designed from day 1 |
| AbortSignal throughout | "Unacceptable if you can't abort in production" |
| Full observability | "I want to inspect every aspect" |
| 200+ extension packages | Community proves minimalism works |

## Key Lesson

> "If pi doesn't fit your needs, I implore you to fork it."
