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
pub enum Expr {
    Bool(bool),
    String(String),
    Int(i64),
    StringList(Vec<String>),
    Ident(String),
    ChangedFilesAnyMatch(String),
    ChangedFilesAllMatch(String),
    ObserverField {
        observer_id: String,
        field_path: Vec<String>,
    },
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
    BlockFinishUntil { condition: super::directive::FinishCondition, waiver_allowed: bool },
}
