import { ConstraintID } from "./types"

export const CONSTRAINT_TEXT: Record<ConstraintID, string> = {
	noSelfReview: "Do NOT self-review your output. Do NOT check for mistakes.",
	noExplanations: "Do NOT provide explanations, reasoning, or commentary.",
	noCommentary: "Output ONLY the required response. No additional text.",
	strictJSON: "You MUST output valid JSON conforming to the provided format.",
	noApologies: "Do NOT include apologies, disclaimers, or hedging language.",
	bareMinimum: "Keep the response as short as possible while still being correct.",
}

export const CONSTRAINT_PRESETS = {
	manifest: ["noSelfReview", "strictJSON", "noApologies", "bareMinimum"],
	patch: ["noSelfReview", "strictJSON", "noExplanations", "bareMinimum"],
	interactive: ["noSelfReview", "bareMinimum"],
} as const
