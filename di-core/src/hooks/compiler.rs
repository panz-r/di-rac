use std::collections::{HashMap, HashSet};
use crate::hooks::ir;
use crate::hooks::parser;

const KNOWN_EVENTS: &[&str] = &[
    "session_start", "post_tool_use", "observer_result", "pre_finish",
    "user_prompt", "plan_created", "validation_result", "pre_compact",
    "task_complete", "error_occurred",
];

const KNOWN_ACTIONS: &[&str] = &[
    "hint", "criterion", "warn", "approval_note",
    "require_validation", "trigger_observer", "trigger_planner_review",
    "require_evidence", "require_final_note", "remember", "audit",
    "block_finish_until",
];

const VALID_SEVERITIES: &[&str] = &["low", "medium", "high", "critical", "info"];

/// Compiles parsed AST into executable IR.
pub struct HookCompiler;

impl HookCompiler {
    pub fn compile(module: &parser::ast::Module) -> Result<ir::CompiledHookModule, Vec<String>> {
        let mut errors = Vec::new();
        let mut seen_events: HashMap<String, usize> = HashMap::new();
        let mut seen_group_names: HashSet<String> = HashSet::new();
        let mut seen_role_names: HashSet<String> = HashSet::new();

        // Validate groups: no duplicates
        let mut groups: Vec<ir::PathGroup> = Vec::new();
        for g in &module.groups {
            if !seen_group_names.insert(g.name.clone()) {
                errors.push(format!("Group \"{}\" is already defined.", g.name));
                continue;
            }
            let patterns: Vec<glob::Pattern> = g.patterns.iter()
                .filter_map(|p| match glob::Pattern::new(p) {
                    Ok(pat) => Some(pat),
                    Err(e) => {
                        errors.push(format!("Invalid glob pattern \"{}\": {}", p, e));
                        None
                    }
                })
                .collect();
            groups.push(ir::PathGroup { name: g.name.clone(), patterns });
        }

        // Validate roles: no duplicates + budget required
        let mut roles: Vec<ir::RoleDef> = Vec::new();
        for r in &module.roles {
            if !seen_role_names.insert(r.name.clone()) {
                errors.push(format!("Role \"{}\" is already defined.", r.name));
                continue;
            }
            if r.budget.is_none() {
                errors.push(format!("Role \"{}\" is missing budget(...) — budgets are required for loop safety.", r.name));
            }
            roles.push(ir::RoleDef {
                name: r.name.clone(),
                kind: r.kind.clone(),
                system_prompt: r.system_prompt.clone(),
                inputs: r.inputs.clone(),
                output_schema: r.output_schema.clone(),
                budget: r.budget.clone(),
            });
        }

        let mut handlers: Vec<ir::EventHandler> = Vec::new();
        for h in &module.handlers {
            // Validate event name
            if !KNOWN_EVENTS.contains(&h.event.as_str()) {
                let suggestion = KNOWN_EVENTS.iter()
                    .find(|e| e.contains(&h.event) || h.event.contains(*e))
                    .map(|e| format!(". Did you mean \"{}\"?", e))
                    .unwrap_or_default();
                errors.push(format!("Unknown event \"{}\"{}", h.event, suggestion));
                continue;
            }
            if let Some(&prev) = seen_events.get(&h.event) {
                errors.push(format!("Duplicate handler for event '{}': '{}' and '{}'",
                    h.event, handlers[prev].name, h.name));
                continue;
            }
            seen_events.insert(h.event.clone(), handlers.len());

            let rules = Self::compile_block(&h.body, &mut errors);
            handlers.push(ir::EventHandler {
                event: h.event.clone(),
                name: h.name.clone(),
                rules,
            });
        }

        if !errors.is_empty() {
            return Err(errors);
        }

        Ok(ir::CompiledHookModule {
            id: "repo".to_string(),
            source_hash: String::new(), // set by loader
            groups,
            roles,
            handlers,
        })
    }

    fn compile_block(stmts: &[parser::ast::Stmt], errors: &mut Vec<String>) -> Vec<ir::Rule> {
        let mut rules = Vec::new();
        for stmt in stmts {
            match stmt {
                parser::ast::Stmt::If { cond, then_branch, else_branch, .. } => {
                    let condition = Some(Self::compile_expr(cond));
                    let actions = Self::compile_actions(then_branch, errors);
                    rules.push(ir::Rule { condition, actions });

                    if let Some(else_stmts) = else_branch {
                        let not_cond = ir::Expr::Not(Box::new(Self::compile_expr(cond)));
                        let else_actions = Self::compile_actions(else_stmts, errors);
                        rules.push(ir::Rule { condition: Some(not_cond), actions: else_actions });
                    }
                }
                parser::ast::Stmt::ActionCall { name, args, .. } => {
                    let action = Self::compile_action(name, args, errors);
                    if let Some(a) = action {
                        rules.push(ir::Rule { condition: None, actions: vec![a] });
                    }
                }
            }
        }
        rules
    }

    fn compile_actions(stmts: &[parser::ast::Stmt], errors: &mut Vec<String>) -> Vec<ir::ActionIR> {
        let mut actions = Vec::new();
        for stmt in stmts {
            match stmt {
                parser::ast::Stmt::ActionCall { name, args, .. } => {
                    if let Some(a) = Self::compile_action(name, args, errors) {
                        actions.push(a);
                    }
                }
                parser::ast::Stmt::If { then_branch, .. } => {
                    actions.extend(Self::compile_actions(then_branch, errors));
                }
                _ => {}
            }
        }
        actions
    }

    fn compile_action(name: &str, args: &[parser::ast::ActionArg], errors: &mut Vec<String>) -> Option<ir::ActionIR> {
        let get_str = |key: &str| -> Option<String> {
            args.iter().find(|a| a.key.as_deref() == Some(key))
                .and_then(|a| match &a.value {
                    parser::ast::Expr::String(s, _) => Some(s.clone()),
                    _ => None,
                })
        };
        let get_str_list = |key: &str| -> Option<Vec<String>> {
            args.iter().find(|a| a.key.as_deref() == Some(key))
                .and_then(|a| match &a.value {
                    parser::ast::Expr::List(items, _) => {
                        let strs: Vec<String> = items.iter()
                            .filter_map(|e| match e {
                                parser::ast::Expr::String(s, _) => Some(s.clone()),
                                _ => None,
                            }).collect();
                        if strs.is_empty() { None } else { Some(strs) }
                    }
                    _ => None,
                })
        };
        fn parse_severity(s: &str, errors: &mut Vec<String>) -> Option<super::directive::Severity> {
            match s {
                "info" | "low" => Some(super::directive::Severity::Low),
                "medium" => Some(super::directive::Severity::Medium),
                "high" => Some(super::directive::Severity::High),
                "critical" => Some(super::directive::Severity::Critical),
                _ => {
                    errors.push(format!("Invalid severity \"{}\". Valid severities: {}",
                        s, VALID_SEVERITIES.join(", ")));
                    None
                }
            }
        }

        let mut get_severity = |key: &str| -> Option<super::directive::Severity> {
            let s = get_str(key)?;
            parse_severity(&s, errors)
        };

        match name {
            "hint" => get_str("text").or_else(|| args.first().and_then(|a| match &a.value {
                parser::ast::Expr::String(s, _) => Some(s.clone()),
                _ => None,
            })).map(|t| ir::ActionIR::Hint(t)),

            "criterion" => get_str("text").or_else(|| args.first().and_then(|a| match &a.value {
                parser::ast::Expr::String(s, _) => Some(s.clone()),
                _ => None,
            })).map(ir::ActionIR::Criterion),

            "warn" => {
                let severity = get_severity("severity").unwrap_or(super::directive::Severity::Medium);
                let message = get_str("message").or_else(|| args.first().and_then(|a| match &a.value {
                    parser::ast::Expr::String(s, _) => Some(s.clone()),
                    _ => None,
                })).unwrap_or_default();
                Some(ir::ActionIR::Warn { severity, message })
            }

            "require_validation" => {
                let argv = get_str_list("argv").or_else(|| {
                    args.first().and_then(|a| match &a.value {
                        parser::ast::Expr::List(items, _) => {
                            items.iter().filter_map(|e| match e {
                                parser::ast::Expr::String(s, _) => Some(s.clone()),
                                _ => None,
                            }).collect::<Vec<_>>().into()
                        },
                        _ => None,
                    })
                }).unwrap_or_default();
                let reason = get_str("reason").unwrap_or_default();
                Some(ir::ActionIR::RequireValidation { argv, reason })
            }

            "trigger_observer" => {
                let observer_id = get_str("observer_id").or_else(|| get_str("name")).unwrap_or_default();
                let reason = get_str("reason").unwrap_or_default();
                let severity = get_severity("severity").unwrap_or(super::directive::Severity::Medium);
                Some(ir::ActionIR::TriggerObserver { observer_id, reason, severity })
            }

            "trigger_planner_review" => {
                let reason = get_str("reason").unwrap_or_default();
                Some(ir::ActionIR::TriggerPlannerReview { reason })
            }

            "approval_note" => {
                let severity = get_severity("severity").unwrap_or(super::directive::Severity::Medium);
                let message = get_str("message").unwrap_or_default();
                Some(ir::ActionIR::ApprovalNote { severity, message })
            }

            "require_evidence" => {
                let name = get_str("name").or_else(|| get_str("text")).or_else(|| {
                    args.first().and_then(|a| match &a.value {
                        parser::ast::Expr::String(s, _) => Some(s.clone()),
                        _ => None,
                    })
                }).unwrap_or_default();
                Some(ir::ActionIR::RequireEvidence(name))
            }

            "require_final_note" => {
                let text = get_str("text").or_else(|| args.first().and_then(|a| match &a.value {
                    parser::ast::Expr::String(s, _) => Some(s.clone()),
                    _ => None,
                })).unwrap_or_default();
                Some(ir::ActionIR::RequireFinalNote(text))
            }

            "remember" => {
                let fact = get_str("fact").or_else(|| args.first().and_then(|a| match &a.value {
                    parser::ast::Expr::String(s, _) => Some(s.clone()),
                    _ => None,
                })).unwrap_or_default();
                Some(ir::ActionIR::Remember(fact))
            }

            "audit" => {
                let kind = get_str("kind").unwrap_or_default();
                let severity = get_severity("severity").unwrap_or(super::directive::Severity::Low);
                Some(ir::ActionIR::Audit { kind, severity })
            }

            "block_finish_until" => {
                let condition = get_str("condition").or_else(|| get_str("text")).unwrap_or_default();
                let waiver_allowed = true;
                Some(ir::ActionIR::BlockFinishUntil {
                    condition: super::directive::FinishCondition::EvidencePresent(condition),
                    waiver_allowed,
                })
            }

            _ => {
                errors.push(format!("Unknown action \"{}\". Supported actions: {}",
                    name, KNOWN_ACTIONS.join(", ")));
                None
            }
        }
    }

    /// Check if an expression is an observer field access chain (observer.x.y).
    /// Returns Some((observer_id, field_path)) or None.
    fn extract_observer_path(expr: &parser::ast::Expr) -> Option<(String, Vec<String>)> {
        match expr {
            parser::ast::Expr::MemberAccess { object, member, .. } => {
                match object.as_ref() {
                    parser::ast::Expr::Ident(name, _) if name == "observer" => {
                        Some((name.clone(), vec![member.clone()]))
                    }
                    parser::ast::Expr::MemberAccess { .. } => {
                        if let Some((base, mut fields)) = Self::extract_observer_path(object) {
                            if base == "observer" {
                                fields.push(member.clone());
                                Some((base, fields))
                            } else {
                                None
                            }
                        } else {
                            None
                        }
                    }
                    _ => None,
                }
            }
            _ => None,
        }
    }

    fn compile_expr(expr: &parser::ast::Expr) -> ir::Expr {
        match expr {
            parser::ast::Expr::Bool(b, _) => ir::Expr::Bool(*b),
            parser::ast::Expr::String(s, _) => ir::Expr::String(s.clone()),
            parser::ast::Expr::Int(n, _) => ir::Expr::Int(*n),
            parser::ast::Expr::Ident(name, _) => ir::Expr::Ident(name.clone()),

            parser::ast::Expr::MethodCall { object, method, args, .. } => {
                // Detect known method patterns
                match method.as_str() {
                    "any_match" | "all_match" => {
                        let pattern = args.first().map(|a| Self::compile_expr(a));
                        match pattern {
                            Some(ir::Expr::String(p)) if method == "any_match" => {
                                ir::Expr::ChangedFilesAnyMatch(p)
                            }
                            Some(ir::Expr::String(p)) => {
                                ir::Expr::ChangedFilesAllMatch(p)
                            }
                            _ => ir::Expr::Bool(false),
                        }
                    }
                    _ => {
                        // Generic method call — treat as identifier lookup for now
                        ir::Expr::Ident(format!("{}.{}", Self::expr_to_debug(object), method))
                    }
                }
            }

            parser::ast::Expr::BinaryOp { left, op, right, .. } => {
                ir::Expr::BinaryOp {
                    left: Box::new(Self::compile_expr(left)),
                    op: match op {
                        parser::ast::BinOp::And => ir::BinOp::And,
                        parser::ast::BinOp::Or => ir::BinOp::Or,
                        parser::ast::BinOp::Eq => ir::BinOp::Eq,
                        parser::ast::BinOp::Neq => ir::BinOp::Neq,
                        parser::ast::BinOp::In => ir::BinOp::In,
                    },
                    right: Box::new(Self::compile_expr(right)),
                }
            }

            parser::ast::Expr::List(items, _) => {
                // Represent list as first item for now (simplified)
                items.first().map(Self::compile_expr).unwrap_or(ir::Expr::Bool(false))
            }

            parser::ast::Expr::MemberAccess { object, member, .. } => {
                // Detect observer field access: observer.name or observer.output.field
                if let Some(fields) = Self::extract_observer_path(expr) {
                    let (observer_id, field_path) = fields;
                    ir::Expr::ObserverField { observer_id, field_path }
                } else {
                    let obj_str = Self::expr_to_debug(object);
                    ir::Expr::Ident(format!("{}.{}", obj_str, member))
                }
            }

            parser::ast::Expr::Dict(_, _) => ir::Expr::Bool(true),
        }
    }

    fn expr_to_debug(expr: &parser::ast::Expr) -> String {
        match expr {
            parser::ast::Expr::Ident(name, _) => name.clone(),
            parser::ast::Expr::String(s, _) => format!("\"{}\"", s),
            parser::ast::Expr::Bool(b, _) => b.to_string(),
            parser::ast::Expr::Int(n, _) => n.to_string(),
            parser::ast::Expr::MethodCall { object, method, .. } => {
                format!("{}.{}()", Self::expr_to_debug(object), method)
            }
            parser::ast::Expr::MemberAccess { object, member, .. } => {
                format!("{}.{}", Self::expr_to_debug(object), member)
            }
            parser::ast::Expr::BinaryOp { left, op, right, .. } => {
                format!("{} {:?} {}", Self::expr_to_debug(left), op, Self::expr_to_debug(right))
            }
            parser::ast::Expr::List(_, _) => "[...]".to_string(),
            parser::ast::Expr::Dict(_, _) => "{...}".to_string(),
        }
    }
}
