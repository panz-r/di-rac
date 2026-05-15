use std::collections::HashSet;
use serde::{Serialize, Deserialize};

/// Events the agent loop emits that hooks can react to.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum AgentLoopEvent {
    SessionStart,
    PostToolUse {
        tool_name: String,
        changed_files: Vec<String>,
        success: bool,
    },
    ObserverResult {
        observer_id: String,
        output: serde_json::Value,
    },
    PreFinish,
    /// Before sending a prompt to the LLM.
    UserPrompt {
        prompt_snippet: String,
        token_count: usize,
    },
    /// A plan was created (plan tool or plan mode).
    PlanCreated {
        plan_text: String,
        files: Vec<String>,
    },
    /// A validation command completed.
    ValidationResult {
        command: String,
        exit_code: i32,
        stdout_snippet: String,
        success: bool,
    },
    /// Before context compaction.
    PreCompact {
        current_tokens: usize,
        token_limit: usize,
        reason: String,
    },
    /// A task or subtask completed.
    TaskComplete {
        summary: String,
        success: bool,
    },
    /// An error occurred.
    ErrorOccurred {
        message: String,
        severity: Severity,
        tool_name: Option<String>,
    },
}

/// Severity level for warnings and approval notes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Severity {
    Low,
    Medium,
    High,
    Critical,
}

/// Scope for observation triggers.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ObservationScope {
    Diff,
    FullContext,
}

/// Conditions that must be satisfied before finishing.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum FinishCondition {
    EvidencePresent(String),
    FinalNotePresent,
    ObserverCleared(String),
}

/// De-duplication key for directives that should not repeat idly.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct DedupeKey {
    pub hook_id: String,
    pub event: String,
    pub kind: String,
    pub scope_hash: u64,
}

/// Typed directives that hooks emit. Native systems consume these.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum LoopDirective {
    AddHint {
        text: String,
    },
    AddCriterion {
        text: String,
    },
    Warn {
        severity: Severity,
        message: String,
    },
    ApprovalNote {
        severity: Severity,
        message: String,
    },
    RequireValidation {
        argv: Vec<String>,
        reason: String,
        dedupe_key: DedupeKey,
    },
    TriggerObserver {
        observer_id: String,
        reason: String,
        severity: Severity,
        scope: ObservationScope,
        dedupe_key: DedupeKey,
    },
    TriggerPlannerReview {
        reason: String,
        dedupe_key: DedupeKey,
    },
    RequireEvidence {
        name: String,
    },
    RequireFinalNote {
        text: String,
    },
    Remember {
        fact: String,
    },
    Audit {
        kind: String,
        severity: Severity,
    },
    BlockFinishUntil {
        condition: FinishCondition,
        waiver_allowed: bool,
    },
}

/// Merge rules for accumulated directives.
#[derive(Debug, Clone, Default)]
pub struct MergedDirectives {
    pub hints: Vec<String>,
    pub criteria: Vec<String>,
    pub warnings: Vec<(Severity, String)>,
    pub approval_notes: Vec<(Severity, String)>,
    pub validations: Vec<ValidationRequest>,
    pub observer_triggers: Vec<ObserverTrigger>,
    pub planner_reviews: Vec<PlannerReview>,
    pub evidence_required: Vec<String>,
    pub final_notes: Vec<String>,
    pub remembered_facts: Vec<String>,
    pub audit_events: Vec<AuditEvent>,
    pub finish_gates: Vec<FinishGate>,
    pub(crate) seen_keys: HashSet<DedupeKey>,
}

#[derive(Debug, Clone)]
pub struct ValidationRequest {
    pub argv: Vec<String>,
    pub reason: String,
    pub dedupe_key: DedupeKey,
    pub completed: bool,
}

#[derive(Debug, Clone)]
pub struct ObserverTrigger {
    pub observer_id: String,
    pub reason: String,
    pub severity: Severity,
    pub scope: ObservationScope,
    pub dedupe_key: DedupeKey,
}

#[derive(Debug, Clone)]
pub struct PlannerReview {
    pub reason: String,
    pub dedupe_key: DedupeKey,
}

#[derive(Debug, Clone)]
pub struct AuditEvent {
    pub kind: String,
    pub severity: Severity,
    pub timestamp: i64,
}

#[derive(Debug, Clone)]
pub struct FinishGate {
    pub condition: FinishCondition,
    pub waiver_allowed: bool,
    pub satisfied: bool,
}

/// Merges a batch of directives into the accumulated state.
pub struct DirectiveMerger;

impl DirectiveMerger {
    pub fn merge(into: &mut MergedDirectives, directives: Vec<LoopDirective>) {
        for d in directives {
            match d {
                LoopDirective::AddHint { text } => {
                    into.hints.push(text);
                }
                LoopDirective::AddCriterion { text } => {
                    into.criteria.push(text);
                }
                LoopDirective::Warn { severity, message } => {
                    into.warnings.push((severity, message));
                    into.warnings.sort_by_key(|(s, _)| *s);
                }
                LoopDirective::ApprovalNote { severity, message } => {
                    into.approval_notes.push((severity, message));
                    into.approval_notes.sort_by_key(|(s, _)| *s);
                }
                LoopDirective::RequireValidation { argv, reason, dedupe_key } => {
                    if into.seen_keys.insert(dedupe_key.clone()) {
                        into.validations.push(ValidationRequest {
                            argv,
                            reason,
                            dedupe_key,
                            completed: false,
                        });
                    }
                }
                LoopDirective::TriggerObserver { observer_id, reason, severity, scope, dedupe_key } => {
                    if into.seen_keys.insert(dedupe_key.clone()) {
                        into.observer_triggers.push(ObserverTrigger {
                            observer_id,
                            reason,
                            severity,
                            scope,
                            dedupe_key,
                        });
                    }
                }
                LoopDirective::TriggerPlannerReview { reason, dedupe_key } => {
                    if into.seen_keys.insert(dedupe_key.clone()) {
                        into.planner_reviews.push(PlannerReview { reason, dedupe_key });
                    }
                }
                LoopDirective::RequireEvidence { name } => {
                    into.evidence_required.push(name);
                }
                LoopDirective::RequireFinalNote { text } => {
                    into.final_notes.push(text);
                }
                LoopDirective::Remember { fact } => {
                    into.remembered_facts.push(fact);
                }
                LoopDirective::Audit { kind, severity } => {
                    into.audit_events.push(AuditEvent {
                        kind,
                        severity,
                        timestamp: chrono::Utc::now().timestamp(),
                    });
                }
                LoopDirective::BlockFinishUntil { condition, waiver_allowed } => {
                    into.finish_gates.push(FinishGate {
                        condition,
                        waiver_allowed,
                        satisfied: false,
                    });
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_severity_ordering() {
        assert!(Severity::Low < Severity::Medium);
        assert!(Severity::Medium < Severity::High);
        assert!(Severity::High < Severity::Critical);
        assert!(Severity::Low < Severity::Critical);
        assert_eq!(Severity::Low, Severity::Low);
        assert_eq!(Severity::Critical, Severity::Critical);
    }

    #[test]
    fn test_severity_max() {
        let sevs = vec![Severity::Low, Severity::Critical, Severity::Medium];
        let max = sevs.iter().max().unwrap();
        assert_eq!(*max, Severity::Critical);
    }

    #[test]
    fn test_dedupe_key_equality() {
        let a = DedupeKey { hook_id: "h".into(), event: "e".into(), kind: "k".into(), scope_hash: 42 };
        let b = DedupeKey { hook_id: "h".into(), event: "e".into(), kind: "k".into(), scope_hash: 42 };
        let c = DedupeKey { hook_id: "h2".into(), event: "e".into(), kind: "k".into(), scope_hash: 42 };
        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    #[test]
    fn test_dedupe_key_hash() {
        use std::collections::HashSet;
        let mut set = HashSet::new();
        set.insert(DedupeKey { hook_id: "h".into(), event: "e".into(), kind: "k".into(), scope_hash: 1 });
        set.insert(DedupeKey { hook_id: "h".into(), event: "e".into(), kind: "k".into(), scope_hash: 1 });
        assert_eq!(set.len(), 1, "Same dedupe key should hash to same bucket");
    }

    #[test]
    fn test_finish_condition_display() {
        let ev = FinishCondition::EvidencePresent("test".into());
        let fnp = FinishCondition::FinalNotePresent;
        let oc = FinishCondition::ObserverCleared("obs".into());
        // Just check derives
        assert_ne!(format!("{:?}", ev), "");
        assert_ne!(format!("{:?}", fnp), "");
        assert_ne!(format!("{:?}", oc), "");
    }

    #[test]
    fn test_observation_scope() {
        assert_ne!(ObservationScope::Diff, ObservationScope::FullContext);
    }

    #[test]
    fn test_merged_directives_default_is_empty() {
        let m = MergedDirectives::default();
        assert!(m.hints.is_empty());
        assert!(m.criteria.is_empty());
        assert!(m.warnings.is_empty());
        assert!(m.approval_notes.is_empty());
        assert!(m.validations.is_empty());
        assert!(m.observer_triggers.is_empty());
        assert!(m.planner_reviews.is_empty());
        assert!(m.evidence_required.is_empty());
        assert!(m.final_notes.is_empty());
        assert!(m.remembered_facts.is_empty());
        assert!(m.audit_events.is_empty());
        assert!(m.finish_gates.is_empty());
    }

    #[test]
    fn test_merger_hint_accumulates() {
        let mut m = MergedDirectives::default();
        DirectiveMerger::merge(&mut m, vec![LoopDirective::AddHint { text: "a".into() }]);
        DirectiveMerger::merge(&mut m, vec![LoopDirective::AddHint { text: "b".into() }]);
        assert_eq!(m.hints.len(), 2);
        assert_eq!(m.hints[0], "a");
        assert_eq!(m.hints[1], "b");
    }

    #[test]
    fn test_merger_criterion_accumulates() {
        let mut m = MergedDirectives::default();
        DirectiveMerger::merge(&mut m, vec![LoopDirective::AddCriterion { text: "c1".into() }]);
        DirectiveMerger::merge(&mut m, vec![LoopDirective::AddCriterion { text: "c2".into() }]);
        assert_eq!(m.criteria.len(), 2);
    }

    #[test]
    fn test_merger_warning_sorted_by_severity() {
        let mut m = MergedDirectives::default();
        DirectiveMerger::merge(&mut m, vec![
            LoopDirective::Warn { severity: Severity::Critical, message: "critical".into() },
            LoopDirective::Warn { severity: Severity::Low, message: "low".into() },
        ]);
        assert_eq!(m.warnings[0].0, Severity::Low);
        assert_eq!(m.warnings[1].0, Severity::Critical);
    }

    #[test]
    fn test_merger_validation_dedupe() {
        let mut m = MergedDirectives::default();
        let dk = DedupeKey { hook_id: "h".into(), event: "e".into(), kind: "v".into(), scope_hash: 1 };
        DirectiveMerger::merge(&mut m, vec![
            LoopDirective::RequireValidation { argv: vec!["cargo".into()], reason: "r".into(), dedupe_key: dk.clone() },
        ]);
        assert_eq!(m.validations.len(), 1);
        DirectiveMerger::merge(&mut m, vec![
            LoopDirective::RequireValidation { argv: vec!["cargo".into()], reason: "r".into(), dedupe_key: dk },
        ]);
        assert_eq!(m.validations.len(), 1, "Dedupe across merge calls");
    }

    #[test]
    fn test_merger_all_directive_types() {
        let mut m = MergedDirectives::default();
        let dk = DedupeKey { hook_id: "h".into(), event: "e".into(), kind: "k".into(), scope_hash: 0 };
        let directives = vec![
            LoopDirective::AddHint { text: "hint".into() },
            LoopDirective::AddCriterion { text: "crit".into() },
            LoopDirective::Warn { severity: Severity::Medium, message: "warn".into() },
            LoopDirective::ApprovalNote { severity: Severity::High, message: "approve".into() },
            LoopDirective::RequireValidation { argv: vec!["test".into()], reason: "r".into(), dedupe_key: DedupeKey { kind: "v".into(), ..dk.clone() } },
            LoopDirective::TriggerObserver { observer_id: "obs".into(), reason: "r".into(), severity: Severity::High, scope: ObservationScope::Diff, dedupe_key: DedupeKey { kind: "to".into(), ..dk.clone() } },
            LoopDirective::TriggerPlannerReview { reason: "r".into(), dedupe_key: DedupeKey { kind: "pr".into(), ..dk.clone() } },
            LoopDirective::RequireEvidence { name: "ev".into() },
            LoopDirective::RequireFinalNote { text: "note".into() },
            LoopDirective::Remember { fact: "fact".into() },
            LoopDirective::Audit { kind: "audit".into(), severity: Severity::Low },
            LoopDirective::BlockFinishUntil { condition: FinishCondition::EvidencePresent("x".into()), waiver_allowed: true },
        ];
        DirectiveMerger::merge(&mut m, directives);
        assert_eq!(m.hints.len(), 1);
        assert_eq!(m.criteria.len(), 1);
        assert_eq!(m.warnings.len(), 1);
        assert_eq!(m.approval_notes.len(), 1);
        assert_eq!(m.validations.len(), 1);
        assert_eq!(m.observer_triggers.len(), 1);
        assert_eq!(m.planner_reviews.len(), 1);
        assert_eq!(m.evidence_required.len(), 1);
        assert_eq!(m.final_notes.len(), 1);
        assert_eq!(m.remembered_facts.len(), 1);
        assert_eq!(m.audit_events.len(), 1);
        assert_eq!(m.finish_gates.len(), 1);
    }

    #[test]
    fn test_merger_empty_noop() {
        let mut m = MergedDirectives::default();
        DirectiveMerger::merge(&mut m, vec![]);
        assert!(m.hints.is_empty());
    }

    #[test]
    fn test_merger_remember_accumulates() {
        let mut m = MergedDirectives::default();
        DirectiveMerger::merge(&mut m, vec![
            LoopDirective::Remember { fact: "f1".into() },
            LoopDirective::Remember { fact: "f2".into() },
        ]);
        assert_eq!(m.remembered_facts.len(), 2);
    }

    #[test]
    fn test_merger_audit_timestamped() {
        let mut m = MergedDirectives::default();
        DirectiveMerger::merge(&mut m, vec![
            LoopDirective::Audit { kind: "test".into(), severity: Severity::Low },
        ]);
        assert_eq!(m.audit_events.len(), 1);
        assert!(m.audit_events[0].timestamp > 0);
        assert_eq!(m.audit_events[0].kind, "test");
    }

    #[test]
    fn test_validation_request_tracking() {
        let dk = DedupeKey { hook_id: "h".into(), event: "e".into(), kind: "v".into(), scope_hash: 1 };
        let vr = ValidationRequest { argv: vec!["test".into()], reason: "r".into(), dedupe_key: dk, completed: false };
        assert!(!vr.completed);
    }
}



