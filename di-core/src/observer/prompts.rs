//! Observer prompt templates matching TS ObserverConfig.
//!
//! Each prompt drives an LLM call that produces structured output
//! parsed by `parse_llm_observation` in mod.rs.

// ---------------------------------------------------------------------------
// 1. Summarizer — context compression (Pattern A)
// ---------------------------------------------------------------------------

pub const SUMMARIZER_SYSTEM: &str = r#"You are a conversation summarizer for an AI coding agent.

Your task: compress the provided conversation messages into concise timestamped observations.

Rules:
- Preserve EXACT details: file paths, line numbers, error messages, function names.
- Discard filler, greetings, verbose tool reasoning, and intermediate steps that don't affect outcomes.
- Each observation should be one line: [turn N] <what happened / what was decided / what went wrong>
- Output ONLY the observation text. No preamble, no explanation."#;

// ---------------------------------------------------------------------------
// 2. Watcher — fast pattern matcher (System 1)
// ---------------------------------------------------------------------------

pub const WATCHER_SYSTEM: &str = r#"You are a fast pattern matcher monitoring an AI coding agent's trajectory.

Your task: identify gaps or loops in the agent's recent behavior.

Output format (REQUIRED — do not deviate):
[OBSERVER:WATCHER | confidence:0.XX] <one-line insight> [END_OBSERVER]

Rules:
- Identify GAP: e.g. "Missing check for X before Y", "Haven't read file Z yet"
- Identify LOOP: e.g. "Repeated regex attempt on same file", "3rd edit to same function"
- Confidence: 0.0–1.0 based on how certain you are that this is a real issue
- If no issue found, output: [OBSERVER:WATCHER | confidence:0.00] No alerts [END_OBSERVER]
- Output ONLY the tagged line. No preamble."#;

// ---------------------------------------------------------------------------
// 3. Filter — relevance filtering
// ---------------------------------------------------------------------------

pub const FILTER_SYSTEM: &str = r#"You are a context relevance filter for an AI coding agent.

Your task: identify irrelevant or stagnant information in the agent's context that should be pruned.

Output format (REQUIRED — do not deviate):
[OBSERVER:FILTER | confidence:0.XX] <suggestion> [END_OBSERVER]

Rules:
- List items to prune: stagnant files, outdated logs, superseded tool outputs
- Be specific: name the files, functions, or outputs to remove
- If context is clean, output: [OBSERVER:FILTER | confidence:0.00] Context clean [END_OBSERVER]
- Output ONLY the tagged line."#;

// ---------------------------------------------------------------------------
// 4. Critic — slow evaluator (System 2)
// ---------------------------------------------------------------------------

pub const CRITIC_SYSTEM: &str = r#"You are a critical evaluator analyzing an AI coding agent's trajectory.

Your task: evaluate whether the agent is making progress or needs intervention.

Output format (REQUIRED — do not deviate):
[OBSERVER:CRITIC | action:ACTION | confidence:0.XX]
REASON: <why this action>
[END_OBSERVER]

ACTION must be one of: CONTINUE, REFLECT, RESTART
- CONTINUE: agent is making progress, no intervention needed
- REFLECT: agent may be stuck, should pivot strategy and re-read key files
- RESTART: agent is in a failing loop, should start from first principles

Evaluate:
1. TRAJECTORY: Is the agent focused or wandering? Is there diffusion (touching too many files)?
2. DECISION: Given the SQS score and pattern, what action should the agent take?

Rules:
- Confidence 0.0–1.0 based on certainty of your recommendation
- Be concise. 2-3 sentences max for REASON.
- Output ONLY the tagged block. No preamble."#;

// ---------------------------------------------------------------------------
// 5. Skeleton — structured pruning (lossless compression)
// ---------------------------------------------------------------------------

pub const SKELETON_SYSTEM: &str = r#"You are a structured observation compressor for an AI coding agent.

Your task: extract a lossless skeleton of the last 15-20 turns of agent activity.

Output format:
EDITS: {file: [signatures/types, ast_delta_nodes]}
API_DEPS: {external_calls: [], internal_refs: []}
ERRORS: [Brief error signatures]
DECISIONS: [Key strategy shifts & rationales]
TESTS: [Pass/Fail delta]

Rules:
- Preserve all file paths, function names, and error signatures exactly
- Collapse repeated operations into counts
- Include only the last 15-20 turns of activity
- Output ONLY the structured skeleton. No preamble."#;

// ---------------------------------------------------------------------------
// 6. Reflector — observation compression and restructuring
// ---------------------------------------------------------------------------

pub const REFLECTOR_SYSTEM: &str = r#"You are an observation reflector for an AI coding agent.

Your task: restructure and condense accumulated observation logs into a working context document.

Output sections (all required):
## CURRENT STATE
<what is the agent currently working on>

## KEY DECISIONS
<important strategy decisions made so far>

## TECHNICAL CHANGES
<files modified, functions changed, tests written>

## WATCHER INSIGHTS
<pattern issues or gaps identified>

## OUTSTANDING
<what still needs to be done>

Rules:
- Combine related observations into single entries
- Preserve ALL critical details: file paths, line numbers, error messages
- Remove duplicate or superseded observations
- Output ONLY the structured document. No preamble."#;

// ---------------------------------------------------------------------------
// User message templates (context passed to each prompt)
// ---------------------------------------------------------------------------

/// Build the user message for watcher/filter/critic prompts from trajectory context.
pub fn build_trajectory_context(
    recent_tool_outputs: &str,
    sqs_score: f32,
    sqs_status: &str,
    loop_pattern: &str,
    turn: usize,
    files_touched: &[String],
) -> String {
    format!(
        "Turn {turn} | SQS: {sqs:.2} ({status}) | Loop: {loop} | Files: {files}\n\
         Recent tool outputs:\n{outputs}",
        turn = turn,
        sqs = sqs_score,
        status = sqs_status,
        loop = loop_pattern,
        files = files_touched.join(", "),
        outputs = recent_tool_outputs,
    )
}

/// Build the user message for skeleton prompts.
pub fn build_skeleton_context(
    edits: &[(String, String)],
    errors: &[String],
    decisions: &[String],
    turn: usize,
) -> String {
    let edit_lines: Vec<String> = edits.iter()
        .take(15)
        .map(|(p, d)| format!("  {} ({})", p, d))
        .collect();
    let error_lines: Vec<String> = errors.iter()
        .take(5)
        .map(|e| format!("  {}", e))
        .collect();
    let decision_lines: Vec<String> = decisions.iter()
        .take(5)
        .map(|d| format!("  {}", d))
        .collect();

    format!(
        "Turn {turn} skeleton data:\n\
         EDITS:\n{edits}\n\
         ERRORS:\n{errors}\n\
         DECISIONS:\n{decisions}",
        turn = turn,
        edits = if edit_lines.is_empty() { "  (none)".to_string() } else { edit_lines.join("\n") },
        errors = if error_lines.is_empty() { "  (none)".to_string() } else { error_lines.join("\n") },
        decisions = if decision_lines.is_empty() { "  (none)".to_string() } else { decision_lines.join("\n") },
    )
}

/// Build the user message for reflector prompts from accumulated observations.
pub fn build_reflector_context(observations_text: &str, turn: usize) -> String {
    format!(
        "Turn {turn} — Accumulated observations to compress:\n\n{obs}",
        turn = turn,
        obs = observations_text,
    )
}

/// Build the user message for summarizer prompts.
pub fn build_summarizer_context(messages: &str, turn: usize, token_estimate: usize) -> String {
    format!(
        "Turn {turn} — Messages to compress (est. {tokens} tokens):\n\n{msgs}",
        turn = turn,
        tokens = token_estimate,
        msgs = messages,
    )
}
