use super::*;
use crate::agent::trajectory::{Message, Role, ToolMessageMeta};
use crate::context::distiller::schemas::DistilledToolResult;
use crate::context::distiller::validate::{
    validate_tool_result,
    validate_tool_result_faithfulness,
    validate_exact_evidence_faithfulness,
};
use crate::context::task_state::TaskStateReducer;
use crate::util::secrets;
use chrono::Utc;
use std::collections::HashSet;
use uuid::Uuid;

// ---------------------------------------------------------------------------
// ScenarioBuilder
// ---------------------------------------------------------------------------

pub struct ScenarioBuilder {
    trajectory: crate::agent::trajectory::Trajectory,
    edited: HashSet<String>,
    user_texts: Vec<String>,
    token_limit: usize,
}

impl ScenarioBuilder {
    pub fn new() -> Self {
        Self {
            trajectory: crate::agent::trajectory::Trajectory::new(),
            edited: HashSet::new(),
            user_texts: Vec::new(),
            token_limit: 32000,
        }
    }

    pub fn with_token_limit(&mut self, limit: usize) -> &mut Self {
        self.token_limit = limit;
        self
    }

    pub fn with_initial_task(&mut self, text: &str, tokens: usize) -> &mut Self {
        self.user_texts.push(text.to_string());
        self.trajectory.add_message(Role::User, serde_json::json!(text), tokens);
        self
    }

    pub fn with_constraint(&mut self, text: &str, tokens: usize) -> &mut Self {
        let constraint_text = format!("Constraint: {}", text);
        self.user_texts.push(constraint_text.clone());
        self.trajectory.add_message(Role::User, serde_json::json!(constraint_text), tokens);
        self
    }

    pub fn with_correction(&mut self, text: &str, tokens: usize) -> &mut Self {
        self.user_texts.push(text.to_string());
        self.trajectory.add_message(Role::User, serde_json::json!(text), tokens);
        self
    }

    pub fn with_tool_turn(
        &mut self,
        tool_name: &str,
        assistant_text: &str,
        tool_output: &str,
        assistant_tokens: usize,
        tool_tokens: usize,
        paths_read: Vec<&str>,
        paths_written: Vec<&str>,
    ) -> usize {
        self.trajectory.add_message(Role::Assistant, serde_json::json!(assistant_text), assistant_tokens);
        let idx = self.trajectory.messages.len();
        let meta = ToolMessageMeta {
            tool_name: tool_name.to_string(),
            paths_read: paths_read.into_iter().map(|s| s.to_string()).collect(),
            paths_written: paths_written.into_iter().map(|s| s.to_string()).collect(),
            is_compacted: false,
            artifact_ref: None,
        };
        self.trajectory.add_tool_result(
            serde_json::json!(tool_output),
            tool_tokens,
            0,
            meta,
        );
        idx
    }

    pub fn with_filler_turns(&mut self, count: usize) -> &mut Self {
        for i in 0..count {
            self.trajectory.add_message(
                Role::Assistant,
                serde_json::json!(format!("Assistant filler {}", i)),
                30,
            );
            self.trajectory.add_message(
                Role::Tool,
                serde_json::json!(format!("Tool filler output {}", i)),
                40,
            );
        }
        self
    }

    pub fn mark_edited(&mut self, files: Vec<&str>) -> &mut Self {
        for f in files {
            self.edited.insert(f.to_string());
        }
        self
    }

    pub fn build(&self) -> crate::agent::trajectory::Trajectory {
        let mut t = crate::agent::trajectory::Trajectory::new();
        let mut tool_idx = 0;
        for msg in &self.trajectory.messages {
            if matches!(msg.role, Role::Tool) {
                t.add_tool_result(msg.content.clone(), msg.tokens, tool_idx, msg.tool_meta.clone());
                tool_idx += 1;
            } else {
                t.add_message(msg.role, msg.content.clone(), msg.tokens);
            }
            // Copy over tool_calls and thinking for the last message if assistant
            if matches!(msg.role, Role::Assistant) && !msg.tool_calls.is_empty() {
                if let Some(last) = t.messages.last_mut() {
                    last.tool_calls = msg.tool_calls.clone();
                    last.thinking = msg.thinking.clone();
                }
            }
        }
        t
    }

    pub fn build_context_manager(&self) -> ContextManager {
        ContextManager::new(self.token_limit, (self.token_limit as f64 * 0.75) as usize)
    }

    pub fn build_task_reducer(&self) -> TaskStateReducer {
        let mut reducer = TaskStateReducer::new();
        for (i, text) in self.user_texts.iter().enumerate() {
            reducer.process(text, i == 0);
        }
        reducer
    }

    pub fn edited_files(&self) -> HashSet<String> {
        self.edited.clone()
    }
}

// ---------------------------------------------------------------------------
// Invariant helpers
// ---------------------------------------------------------------------------

pub struct InvariantResult {
    pub name: String,
    pub passed: bool,
    pub detail: String,
}

pub struct EvalReport {
    pub family: &'static str,
    pub results: Vec<InvariantResult>,
}

impl EvalReport {
    pub fn assert_all_passed(&self) {
        let failures: Vec<_> = self.results.iter().filter(|r| !r.passed).collect();
        if !failures.is_empty() {
            let details: Vec<String> = failures.iter().map(|f| format!("{}: {}", f.name, f.detail)).collect();
            panic!("[eval:{}]\n{}", self.family, details.join("\n"));
        }
    }
}

pub fn assert_content_contains(messages: &[Message], needle: &str) -> InvariantResult {
    let found = messages.iter().any(|m| m.content.to_string().contains(needle));
    InvariantResult {
        name: format!("contains '{}'", needle),
        passed: found,
        detail: if found { "found".into() } else { format!("'{}' not found in {} messages", needle, messages.len()) },
    }
}

pub fn assert_content_excludes(messages: &[Message], needle: &str) -> InvariantResult {
    let found = messages.iter().any(|m| m.content.to_string().contains(needle));
    InvariantResult {
        name: format!("excludes '{}'", needle),
        passed: !found,
        detail: if found { format!("'{}' unexpectedly found", needle) } else { "not present".into() },
    }
}

pub fn assert_total_tokens_within_budget(messages: &[Message], budget: usize) -> InvariantResult {
    let total: usize = messages.iter().map(|m| m.tokens).sum();
    InvariantResult {
        name: "total tokens within budget".into(),
        passed: total <= budget,
        detail: format!("total {} vs budget {}", total, budget),
    }
}

// ---------------------------------------------------------------------------
// EvalMetrics
// ---------------------------------------------------------------------------

#[derive(Debug, Default)]
pub struct EvalMetrics {
    pub constraints_tested: usize,
    pub constraints_retained: usize,
    pub stale_reads_tested: usize,
    pub stale_reads_correct: usize,
    pub distillation_faithful: usize,
    pub distillation_total: usize,
    pub secrets_found: usize,
    pub secrets_redacted: usize,
}

// ---------------------------------------------------------------------------
// Helper for building minimal DistilledToolResult
// ---------------------------------------------------------------------------

fn minimal_distilled_result() -> DistilledToolResult {
    DistilledToolResult {
        summary: "test summary".into(),
        key_facts: Vec::new(),
        errors: Vec::new(),
        files_referenced: Vec::new(),
        estimated_tokens: 100,
        artifact_ref: None,
        exchange_core: String::new(),
        specific_context: Vec::new(),
        thematic_tags: Vec::new(),
        symbols_referenced: Vec::new(),
        exact_evidence: Vec::new(),
        hypotheses: Vec::new(),
        source_event_ids: Vec::new(),
    }
}

// ===========================================================================
// EVAL FAMILY TESTS
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // Family 1: Retention — constraint survives 40 turns
    // -----------------------------------------------------------------------

    #[test]
    fn eval_retention_constraint_survives_40_turns() {
        let mut builder = ScenarioBuilder::new();
        builder.with_token_limit(20000);
        builder.with_initial_task("Build a REST API for user management", 200);
        builder.with_constraint("must use PostgreSQL not MySQL for the database", 50);
        for i in 0..37 {
            builder.with_tool_turn(
                "read",
                &format!("Reading file {}", i),
                "file contents here",
                30, 40,
                vec![&format!("src/file{}.rs", i)],
                vec![],
            );
        }

        let traj = builder.build();
        let cm = builder.build_context_manager();
        let reducer = builder.build_task_reducer();
        let messages = cm.build_prompt_with_stale_check(
            &traj, &HashSet::new(), Some(&reducer), 4000,
        );

        let report = EvalReport {
            family: "retention",
            results: vec![
                assert_content_contains(&messages, "PostgreSQL"),
                assert_content_contains(&messages, "REST API"),
                assert_total_tokens_within_budget(&messages, 20000 - 4000),
            ],
        };
        report.assert_all_passed();
    }

    // -----------------------------------------------------------------------
    // Family 2: Staleness — old reads replaced, new reads kept
    // -----------------------------------------------------------------------

    #[test]
    fn eval_staleness_edited_file_reads_replaced() {
        let mut builder = ScenarioBuilder::new();
        builder.with_token_limit(30000);
        builder.with_initial_task("Refactor the auth module", 200);

        // Read of file that will be edited
        builder.with_tool_turn("read", "Reading main.rs", "content of main.rs", 30, 200, vec!["src/main.rs"], vec![]);
        // Read of file that will NOT be edited
        builder.with_tool_turn("read", "Reading utils.rs", "content of utils.rs", 30, 200, vec!["src/utils.rs"], vec![]);
        builder.with_filler_turns(5);

        // Edit src/main.rs
        builder.with_tool_turn("edit", "Editing main.rs", "file updated", 30, 50, vec![], vec!["src/main.rs"]);
        builder.mark_edited(vec!["src/main.rs"]);

        let traj = builder.build();
        let cm = builder.build_context_manager();
        let edited = builder.edited_files();
        let messages = cm.build_prompt_with_stale_check(&traj, &edited, None, 4000);

        // Reads of edited files should be replaced with stale notices
        let stale_count = messages.iter()
            .filter(|m| m.content.to_string().contains("stale file read omitted"))
            .count();
        assert!(stale_count > 0, "reads of edited files should be replaced with stale notice");

        // Reads of non-edited files should be preserved
        let has_utils = messages.iter()
            .any(|m| m.content.to_string().contains("content of utils.rs"));
        assert!(has_utils, "reads of non-edited files should be preserved");

        // Non-read tools should not be stale-checked even if mentioning edited files
        let has_edit = messages.iter()
            .any(|m| {
                let content = m.content.to_string();
                m.tool_meta.tool_name == "edit" && content.contains("file updated")
            });
        assert!(has_edit, "edit tool results should never be stale-checked");
    }

    // -----------------------------------------------------------------------
    // Family 3: Retrieval — (removed: artifact system removed)
    // -----------------------------------------------------------------------

    // -----------------------------------------------------------------------
    // Family 4: Placement — tail reminder contains key info
    // -----------------------------------------------------------------------

    #[test]
    fn eval_placement_tail_reminder() {
        let mut reducer = TaskStateReducer::new();
        reducer.process("Build a CLI tool for log analysis", true);
        reducer.process("must use only the standard library no external crates", false);
        reducer.process("log output must be JSON format", false);

        let stale_files = vec!["src/main.rs".to_string()];
        let reminder = reducer.to_tail_reminder(&stale_files, Some("last error: compilation failed at line 42"));

        let report = EvalReport {
            family: "placement",
            results: vec![
                InvariantResult {
                    name: "starts with REMINDER".into(),
                    passed: reminder.starts_with("[REMINDER]"),
                    detail: format!("starts with: {}...", &reminder[..reminder.len().min(30)]),
                },
                InvariantResult {
                    name: "contains goal".into(),
                    passed: reminder.contains("log analysis"),
                    detail: "goal text must appear".into(),
                },
                InvariantResult {
                    name: "contains constraint".into(),
                    passed: reminder.contains("standard library") || reminder.contains("JSON"),
                    detail: "at least one constraint must appear".into(),
                },
                InvariantResult {
                    name: "contains stale file".into(),
                    passed: reminder.contains("src/main.rs"),
                    detail: "stale files must appear".into(),
                },
                InvariantResult {
                    name: "contains latest failure".into(),
                    passed: reminder.contains("compilation failed"),
                    detail: "latest failure must appear".into(),
                },
            ],
        };
        report.assert_all_passed();
    }

    // -----------------------------------------------------------------------
    // Family 5: Distillation — faithfulness validation
    // -----------------------------------------------------------------------

    #[test]
    fn eval_distillation_faithfulness() {
        let original = "fn process_data(input: &str) -> Result<String, Error> {\n    let parsed = parse(input)?;\n    Ok(parsed)\n}\n";

        // Grounded evidence should pass
        let grounded = DistilledToolResult {
            summary: "process_data function parses input".into(),
            files_referenced: vec!["src/lib.rs".into()],
            exact_evidence: vec!["fn process_data(input: &str) -> Result<String, Error>".into()],
            hypotheses: vec!["possibly used for data transformation pipeline".into()],
            ..minimal_distilled_result()
        };
        let file_warnings = validate_tool_result_faithfulness(&grounded, "src/lib.rs content here");
        assert!(file_warnings.is_empty(), "grounded file refs should pass: {:?}", file_warnings);
        let evidence_warnings = validate_exact_evidence_faithfulness(&grounded, original);
        assert!(evidence_warnings.is_empty(), "grounded exact evidence should pass: {:?}", evidence_warnings);

        // Ungrounded evidence should produce warnings
        let ungrounded = DistilledToolResult {
            files_referenced: vec!["nonexistent_file.rs".into()],
            exact_evidence: vec!["this text does not appear in the original output at all".into()],
            ..minimal_distilled_result()
        };
        let file_warns = validate_tool_result_faithfulness(&ungrounded, "some other content");
        assert!(!file_warns.is_empty(), "ungrounded file refs should produce warnings");

        let evidence_warns = validate_exact_evidence_faithfulness(&ungrounded, original);
        assert!(!evidence_warns.is_empty(), "ungrounded evidence should produce warnings");
    }

    #[test]
    fn eval_distillation_schema_validation() {
        // Valid result passes
        let valid = minimal_distilled_result();
        assert!(validate_tool_result(&valid).is_ok(), "valid result should pass validation");

        // Empty summary fails
        let empty = DistilledToolResult { summary: String::new(), ..minimal_distilled_result() };
        assert!(validate_tool_result(&empty).is_err(), "empty summary should fail");

        // Zero tokens fails
        let zero_tokens = DistilledToolResult { estimated_tokens: 0, ..minimal_distilled_result() };
        assert!(validate_tool_result(&zero_tokens).is_err(), "zero tokens should fail");
    }

    // -----------------------------------------------------------------------
    // Family 6: Reuse — memory vault returns relevant observations
    // -----------------------------------------------------------------------

    #[test]
    fn eval_reuse_memory_vault() {
        let mut vault = MemoryVault::new();

        vault.observations.push(Observation {
            id: Uuid::new_v4(),
            obs_type: "pattern".into(),
            content: "Authentication uses JWT tokens with RS256".into(),
            timestamp: 100,
            tokens: 10,
            confidence: 0.8,
            apis: Some(HashSet::from(["auth".into(), "jwt".into()])),
        });
        vault.observations.push(Observation {
            id: Uuid::new_v4(),
            obs_type: "pattern".into(),
            content: "Database uses connection pooling with PgPool".into(),
            timestamp: 101,
            tokens: 10,
            confidence: 0.9,
            apis: Some(HashSet::from(["database".into()])),
        });
        vault.observations.push(Observation {
            id: Uuid::new_v4(),
            obs_type: "pattern".into(),
            content: "Low confidence observation".into(),
            timestamp: 102,
            tokens: 5,
            confidence: 0.1,
            apis: Some(HashSet::from(["auth".into()])),
        });

        // Query for auth-related observations with min confidence 0.5
        let current = HashSet::from(["auth".into()]);
        let relevant = vault.get_relevant_observations(&current, 0.5);

        assert_eq!(relevant.len(), 1, "only one auth observation above confidence threshold");
        assert!(relevant[0].content.contains("JWT"), "should return the JWT observation");

        // Query for database should return the PgPool observation
        let db_query = HashSet::from(["database".into()]);
        let db_results = vault.get_relevant_observations(&db_query, 0.5);
        assert_eq!(db_results.len(), 1);
        assert!(db_results[0].content.contains("PgPool"));

        // Disjoint API set returns nothing
        let unrelated = HashSet::from(["filesystem".into()]);
        let unrelated_results = vault.get_relevant_observations(&unrelated, 0.5);
        assert!(unrelated_results.is_empty(), "disjoint APIs should return nothing");
    }

    // -----------------------------------------------------------------------
    // Family 7: Latency — p95 prompt build under 5ms
    // -----------------------------------------------------------------------

    #[test]
    fn eval_latency_prompt_build_p95() {
        let mut builder = ScenarioBuilder::new();
        builder.with_token_limit(50000);
        builder.with_initial_task("Large task requiring many turns", 200);
        for i in 0..50 {
            builder.with_tool_turn(
                "read",
                &format!("Assistant turn {}", i),
                &format!("Tool output for turn {} with some content", i),
                30, 40,
                vec![],
                vec![],
            );
        }

        let traj = builder.build();
        let cm = builder.build_context_manager();

        let mut durations: Vec<std::time::Duration> = Vec::new();
        for _ in 0..100 {
            let start = std::time::Instant::now();
            cm.build_prompt_with_stale_check(&traj, &HashSet::new(), None, 4000);
            durations.push(start.elapsed());
        }
        durations.sort();
        let p95 = durations[94];
        assert!(
            p95.as_micros() < 5000,
            "p95 latency {}us exceeds 5000us budget",
            p95.as_micros(),
        );
    }

    // -----------------------------------------------------------------------
    // Family 8: Safety — secrets redacted from outputs
    // -----------------------------------------------------------------------

    #[test]
    fn eval_safety_secrets_redacted() {
        let output_with_secret = "API call succeeded with key sk-abc123def456ghi789jkl012mno345pqr678stu901";

        assert!(secrets::scan_for_secrets(output_with_secret), "should detect secret pattern");

        let redacted = secrets::redact_secrets(output_with_secret);
        assert!(!redacted.contains("sk-abc123"), "redacted output must not contain secret");
        assert!(redacted.contains("[REDACTED]"), "redacted output should contain [REDACTED]");
    }

    #[test]
    fn eval_safety_distiller_rejects_secrets() {
        let with_secret = DistilledToolResult {
            summary: "Used key sk-abc123def456ghi789jkl012mno345pqr678stu901".into(),
            ..minimal_distilled_result()
        };
        assert!(validate_tool_result(&with_secret).is_err(), "results with secrets should be rejected");
    }

    // -----------------------------------------------------------------------
    // Eval metrics aggregation test
    // -----------------------------------------------------------------------

    #[test]
    fn eval_metrics_aggregation() {
        let mut metrics = EvalMetrics::default();

        // Simulate 5 constraint checks, 4 retained
        metrics.constraints_tested = 5;
        metrics.constraints_retained = 4;
        assert!(
            metrics.constraints_retained as f64 / metrics.constraints_tested as f64 >= 0.8,
            "constraint retention rate should be >= 80%",
        );

        // Simulate 10 stale read checks, all correct
        metrics.stale_reads_tested = 10;
        metrics.stale_reads_correct = 10;
        assert_eq!(
            metrics.stale_reads_correct as f64 / metrics.stale_reads_tested as f64,
            1.0,
            "stale read correctness should be 100%",
        );
    }
}
