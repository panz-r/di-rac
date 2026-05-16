/// Base name for observer field access (observer.name, observer.output).
pub const OBSERVER_BASE: &str = "observer";

// ── Micro-DSL: FileMatch ────────────────────────────────────────
/// Predicate on the set of changed files. Used via FilesMatch(FileMatch).
#[derive(Debug, Clone)]
pub enum FileMatch {
    AnyMatch(String),
    AllMatch(String),
}

// ── Micro-DSL: ObserverField ────────────────────────────────────
/// Describes a field access path into the observer result.
#[derive(Debug, Clone)]
pub enum ObserverField {
    /// The observer's name/id.
    Name,
    /// A nested JSON path inside observer.output.
    Output(Vec<String>),
}

// ── Composed micro-DSL: Condition (was Expr) ────────────────────
/// The expression language for hook conditions. Composes FileMatch
/// and ObserverField as independent sub-languages.
#[derive(Debug, Clone)]
pub enum Expr {
    // Literals
    Bool(bool),
    String(String),
    Int(i64),
    StringList(Vec<String>),

    // Context queries
    /// General identifier (used for bare `observer` truthy-check).
    Ident(String),
    /// Bare `changed_files` — true when files have changed.
    ChangedFiles,
    /// `changed_files.any_match("x")` / changed_files.all_match("x")
    FilesMatch(FileMatch),
    /// `observer.name`, `observer.output.path`
    Observer(ObserverField),

    // Logical composition
    BinaryOp {
        left: Box<Expr>,
        op: BinOp,
        right: Box<Expr>,
    },
    Not(Box<Expr>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinOp {
    And,
    Or,
    Eq,
    Neq,
    In,
}

/// Compiled hook module — ready for evaluation.
#[derive(Debug, Clone)]
pub struct CompiledHookModule {
    pub id: String,
    pub source_hash: String,
    pub groups: Vec<PathGroup>,
    pub roles: Vec<RoleDef>,
    pub handlers: Vec<EventHandler>,
}

#[derive(Debug, Clone)]
pub struct PathGroup {
    pub name: String,
    pub patterns: Vec<glob::Pattern>,
}

#[derive(Debug, Clone)]
pub struct RoleDef {
    pub name: String,
    pub kind: String,
    pub system_prompt: Option<String>,
    pub inputs: Vec<String>,
    pub output_schema: Option<serde_json::Value>,
    pub budget: Option<super::parser::ast::RoleBudget>,
}

#[derive(Debug, Clone)]
pub struct EventHandler {
    pub event: String,
    pub name: String,
    pub rules: Vec<Rule>,
}

#[derive(Debug, Clone)]
pub struct Rule {
    pub condition: Option<Expr>,
    pub actions: Vec<ActionIR>,
}

#[derive(Debug, Clone)]
pub enum ActionIR {
    Hint(String),
    Criterion(String),
    Warn { severity: super::directive::Severity, message: String },
    ApprovalNote { severity: super::directive::Severity, message: String },
    RequireValidation { argv: Vec<String>, reason: String },
    TriggerObserver { observer_id: String, reason: String, severity: super::directive::Severity },
    TriggerPlannerReview { reason: String },
    RequireEvidence(String),
    RequireFinalNote(String),
    Remember(String),
    Audit { kind: String, severity: super::directive::Severity },
    BlockFinishUntil {
        condition: super::directive::FinishCondition,
        waiver_allowed: bool,
        with_evidence: Option<String>,
        with_final_note: Option<String>,
    },
}
