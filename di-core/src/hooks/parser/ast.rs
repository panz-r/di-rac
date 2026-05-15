use std::collections::HashMap;

/// Source location for diagnostics.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Span {
    pub start: usize,
    pub end: usize,
    pub line: usize,
    pub column: usize,
}

#[derive(Debug, Clone)]
pub struct Located<T> {
    pub inner: T,
    pub span: Span,
}

// ── Top-level module ──

#[derive(Debug, Clone)]
pub struct Module {
    pub groups: Vec<PathGroup>,
    pub roles: Vec<RoleDef>,
    pub handlers: Vec<EventHandler>,
}

#[derive(Debug, Clone)]
pub struct PathGroup {
    pub name: String,
    pub patterns: Vec<String>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct RoleDef {
    pub name: String,
    pub kind: String,
    pub system_prompt: Option<String>,
    pub inputs: Vec<String>,
    pub output_schema: Option<serde_json::Value>,
    pub budget: Option<RoleBudget>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct RoleBudget {
    pub max_tokens: u64,
    pub max_runs: u64,
}

#[derive(Debug, Clone)]
pub struct EventHandler {
    pub event: String,
    pub name: String,
    pub body: Vec<Stmt>,
    pub span: Span,
}

// ── Statements ──

#[derive(Debug, Clone)]
pub enum Stmt {
    If {
        cond: Expr,
        then_branch: Vec<Stmt>,
        else_branch: Option<Vec<Stmt>>,
        span: Span,
    },
    ActionCall {
        name: String,
        args: Vec<ActionArg>,
        span: Span,
    },
}

#[derive(Debug, Clone)]
pub struct ActionArg {
    pub key: Option<String>,
    pub value: Expr,
    pub span: Span,
}

// ── Expressions ──

#[derive(Debug, Clone)]
pub enum Expr {
    Bool(bool, Span),
    String(String, Span),
    Int(i64, Span),
    Ident(String, Span),
    MethodCall {
        object: Box<Expr>,
        method: String,
        args: Vec<Expr>,
        span: Span,
    },
    BinaryOp {
        left: Box<Expr>,
        op: BinOp,
        right: Box<Expr>,
        span: Span,
    },
    List(Vec<Expr>, Span),
    Dict(HashMap<String, Expr>, Span),
    MemberAccess {
        object: Box<Expr>,
        member: String,
        span: Span,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinOp {
    And,
    Or,
    Eq,
    Neq,
    In,
}
