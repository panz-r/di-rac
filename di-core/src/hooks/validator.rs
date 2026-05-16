use crate::hooks::parser;
use crate::hooks::parser::ast::*;
use crate::hooks::compiler::HookCompiler;

/// A diagnostic message produced during hook validation.
#[derive(Debug, Clone)]
pub struct Diagnostic {
    pub severity: Severity,
    pub message: String,
    pub line: Option<usize>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
    Note,
}

impl std::fmt::Display for Severity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Severity::Error => write!(f, "error"),
            Severity::Warning => write!(f, "warning"),
            Severity::Note => write!(f, "note"),
        }
    }
}

/// Validate a `.dhook` source file: parse, compile, then run analysis
/// passes to detect potential problems.
pub fn validate_hook(source: &str) -> Vec<Diagnostic> {
    let mut diags = Vec::new();

    // Phase 1: Parse
    let mut parser = parser::Parser::new(source);
    let module = match parser.parse_module() {
        Ok(m) => m,
        Err(errors) => {
            for e in errors {
                diags.push(Diagnostic {
                    severity: Severity::Error,
                    message: format!("Parse error: {}", e.message),
                    line: Some(e.span.line),
                });
            }
            return diags;
        }
    };

    // Phase 2: Compile
    let _compiled = match HookCompiler::compile(&module) {
        Ok(c) => c,
        Err(errors) => {
            for e in errors {
                diags.push(Diagnostic {
                    severity: Severity::Error,
                    message: e,
                    line: None,
                });
            }
            // Still run analysis even with compile errors (partial info)
            analyze_module(&module, &mut diags);
            return diags;
        }
    };

    // Phase 3: Analysis
    analyze_module(&module, &mut diags);

    diags
}

fn analyze_module(module: &Module, diags: &mut Vec<Diagnostic>) {
    // Collect defined groups and roles
    let defined_groups: std::collections::HashSet<&str> =
        module.groups.iter().map(|g| g.name.as_str()).collect();
    let defined_roles: std::collections::HashSet<&str> =
        module.roles.iter().map(|r| r.name.as_str()).collect();

    // Collect referenced groups and roles
    let mut referenced_groups: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut referenced_roles: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut handler_info: Vec<(&str, usize, usize)> = Vec::new();

    for handler in &module.handlers {
        let mut action_count = 0usize;
        let mut max_depth = 0usize;

        for stmt in &handler.body {
            collect_references(stmt, &mut referenced_groups, &mut referenced_roles);
            action_count += count_actions(stmt);
            let depth = condition_depth(stmt);
            if depth > max_depth {
                max_depth = depth;
            }
        }

        handler_info.push((handler.name.as_str(), action_count, max_depth));

        // Check for unknown events (compiler also catches this, but we add line info)
        // Already handled by compiler — skip.
    }

    // Warnings: defined but unused groups
    for g in &module.groups {
        if !referenced_groups.contains(&g.name) {
            diags.push(Diagnostic {
                severity: Severity::Warning,
                message: format!("Group \"{}\" is defined but never used by any handler", g.name),
                line: Some(g.span.line),
            });
        }
    }

    // Warnings: defined but unreferenced roles
    for r in &module.roles {
        if !referenced_roles.contains(&r.name) {
            diags.push(Diagnostic {
                severity: Severity::Warning,
                message: format!("Role \"{}\" is defined but never triggered by any handler", r.name),
                line: Some(r.span.line),
            });
        }
    }

    // Warnings: referenced but undefined groups
    for name in &referenced_groups {
        if !defined_groups.contains(name.as_str()) {
            let suggestion = find_similar(name, defined_groups.iter().copied());
            let msg = match suggestion {
                Some(s) => format!("Group \"{}\" is used but not defined — did you mean \"{}\"?", name, s),
                None => format!("Group \"{}\" is used but not defined (the name will be treated as a literal glob pattern)", name),
            };
            diags.push(Diagnostic {
                severity: Severity::Warning,
                message: msg,
                line: None,
            });
        }
    }

    // Warnings: referenced but undefined roles
    for name in &referenced_roles {
        if !defined_roles.contains(name.as_str()) {
            diags.push(Diagnostic {
                severity: Severity::Warning,
                message: format!("Observer role \"{}\" is triggered but not defined — no observer agent will run", name),
                line: None,
            });
        }
    }

    // Warnings: high action count (approaching the 20 limit)
    for (hname, count, depth) in &handler_info {
        if *count >= 15 {
            diags.push(Diagnostic {
                severity: Severity::Warning,
                message: format!("Handler \"{}\" emits {} actions (approaching limit of 20)", hname, count),
                line: None,
            });
        }
        if *depth >= 4 {
            diags.push(Diagnostic {
                severity: Severity::Note,
                message: format!("Handler \"{}\" has deep condition nesting (depth {}) — consider simplifying", hname, depth),
                line: None,
            });
        }
    }
}

/// Collect group and role references from a statement tree.
fn collect_references(
    stmt: &Stmt,
    groups: &mut std::collections::HashSet<String>,
    roles: &mut std::collections::HashSet<String>,
) {
    match stmt {
        Stmt::ActionCall { name, args, .. } => {
            // trigger_observer(observer_id="name") → references that role
            if name == "trigger_observer" {
                for arg in args {
                    if let Some(key) = &arg.key {
                        if key == "observer_id" {
                            if let Expr::String(s, _) = &arg.value {
                                roles.insert(s.clone());
                            }
                        }
                    }
                }
            }
        }
        Stmt::If { cond, then_branch, else_branch, .. } => {
            collect_expr_refs(cond, groups, roles);
            for s in then_branch {
                collect_references(s, groups, roles);
            }
            if let Some(eb) = else_branch {
                for s in eb {
                    collect_references(s, groups, roles);
                }
            }
        }
    }
}

/// Collect group and role references from an expression.
fn collect_expr_refs(
    expr: &Expr,
    groups: &mut std::collections::HashSet<String>,
    _roles: &mut std::collections::HashSet<String>,
) {
    match expr {
        Expr::MethodCall { object, method, args, .. } => {
            if method == "any_match" || method == "all_match" {
                if let Some(arg) = args.first() {
                    if let Expr::String(s, _) = arg {
                        groups.insert(s.clone());
                    }
                }
            }
            // Recurse into object (for chained calls)
            collect_expr_refs(object, groups, _roles);
            for a in args {
                collect_expr_refs(a, groups, _roles);
            }
        }
        Expr::BinaryOp { left, right, .. } => {
            collect_expr_refs(left, groups, _roles);
            collect_expr_refs(right, groups, _roles);
        }
        Expr::MemberAccess { object, .. } => {
            collect_expr_refs(object, groups, _roles);
        }
        _ => {}
    }
}

/// Count the number of action calls in a statement tree.
fn count_actions(stmt: &Stmt) -> usize {
    match stmt {
        Stmt::ActionCall { .. } => 1,
        Stmt::If { then_branch, else_branch, .. } => {
            let mut n = 0;
            for s in then_branch { n += count_actions(s); }
            if let Some(eb) = else_branch {
                for s in eb { n += count_actions(s); }
            }
            n
        }
    }
}

/// Compute the maximum nesting depth of conditions in a statement tree.
fn condition_depth(stmt: &Stmt) -> usize {
    match stmt {
        Stmt::ActionCall { .. } => 0,
        Stmt::If { then_branch, else_branch, .. } => {
            let mut d = 1;
            for s in then_branch {
                let sd = condition_depth(s);
                if 1 + sd > d { d = 1 + sd; }
            }
            if let Some(eb) = else_branch {
                for s in eb {
                    let sd = condition_depth(s);
                    if 1 + sd > d { d = 1 + sd; }
                }
            }
            d
        }
    }
}

/// Simple fuzzy name matcher for "did you mean?" suggestions.
fn find_similar<'a>(name: &str, candidates: impl Iterator<Item = &'a str>) -> Option<String> {
    let name_lower = name.to_lowercase();
    candidates
        .filter_map(|c| {
            let dist = lev_distance(&name_lower, &c.to_lowercase());
            if dist <= 2 { Some((c, dist)) } else { None }
        })
        .min_by_key(|(_, d)| *d)
        .map(|(c, _)| c.to_string())
}

fn lev_distance(a: &str, b: &str) -> usize {
    let (a, b) = if a.len() < b.len() { (a, b) } else { (b, a) };
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.chars().enumerate() {
        let mut curr = vec![i + 1];
        for (j, cb) in b.chars().enumerate() {
            let cost = if ca == cb { 0 } else { 1 };
            curr.push(std::cmp::min(
                std::cmp::min(curr[j] + 1, prev[j + 1] + 1),
                prev[j] + cost,
            ));
        }
        prev = curr;
    }
    *prev.last().unwrap_or(&0)
}
