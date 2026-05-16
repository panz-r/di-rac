use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use crate::hooks::ir;
use crate::hooks::parser;
use crate::hooks::parser::ast::{ImportStmt, ImportSymbol, Module};

const KNOWN_EVENTS: &[&str] = &[
    "session_start", "post_tool_use", "observer_result", "pre_finish",
    "user_prompt", "plan_created", "validation_result", "pre_compact",
    "task_complete", "error_occurred",
];

const KNOWN_ACTIONS: &[&str] = &[
    "hint", "criterion", "warn", "approval_note",
    "require", "trigger_observer", "trigger_planner_review",
    "remember", "audit",
    "block_finish_until",
];

const VALID_SEVERITIES: &[&str] = &["low", "medium", "high", "critical", "info"];

/// Compiles parsed AST into executable IR.
pub struct HookCompiler;

impl HookCompiler {
    pub fn compile(module: &parser::ast::Module) -> Result<ir::CompiledHookModule, Vec<String>> {
        Self::compile_with_base(module, None)
    }

    pub fn compile_with_base(
        module: &parser::ast::Module,
        base_dir: Option<&Path>,
    ) -> Result<ir::CompiledHookModule, Vec<String>> {
        let mut errors = Vec::new();
        let mut seen_events: HashMap<String, usize> = HashMap::new();
        let mut seen_group_names: HashSet<String> = HashSet::new();
        let mut seen_role_names: HashSet<String> = HashSet::new();

        // ── Resolve let bindings ──
        // Build a lookup from binding name to constructor for compile_block.
        let let_bindings: HashMap<String, &parser::ast::LetConstructor> = module.let_bindings.iter()
            .map(|b| (b.name.clone(), &b.constructor))
            .collect();

        // ── Resolve imports ──
        // Pull groups and roles from referenced files and merge them into the
        // local definitions before validation.
        let (imported_groups, imported_roles) = Self::resolve_imports(
            &module.imports, module, base_dir, &mut errors,
        );

        // Validate groups: no duplicates (local + imported)
        let mut groups: Vec<ir::PathGroup> = Vec::new();

        for g in &imported_groups {
            if !seen_group_names.insert(g.name.clone()) {
                errors.push(format!("Group \"{}\" is already defined (imported).", g.name));
                continue;
            }
            let mut patterns: Vec<glob::Pattern> = Vec::new();
            for p in &g.patterns {
                match glob::Pattern::new(p) {
                    Ok(pat) => patterns.push(pat),
                    Err(e) => errors.push(format!("Invalid glob pattern \"{}\" in imported group \"{}\": {}", p, g.name, e)),
                }
            }
            groups.push(ir::PathGroup { name: g.name.clone(), patterns });
        }

        for g in &module.groups {
            if !seen_group_names.insert(g.name.clone()) {
                errors.push(format!("Group \"{}\" is already defined.", g.name));
                continue;
            }
            let mut patterns: Vec<glob::Pattern> = Vec::new();
            for p in &g.patterns {
                match glob::Pattern::new(p) {
                    Ok(pat) => patterns.push(pat),
                    Err(e) => errors.push(format!("Invalid glob pattern \"{}\" in group \"{}\": {}", p, g.name, e)),
                }
            }
            groups.push(ir::PathGroup { name: g.name.clone(), patterns });
        }

        // Validate roles: no duplicates + budget required (local + imported)
        let mut roles: Vec<ir::RoleDef> = Vec::new();

        for r in &imported_roles {
            if !seen_role_names.insert(r.name.clone()) {
                errors.push(format!("Role \"{}\" is already defined (imported).", r.name));
                continue;
            }
            if r.budget.is_none() {
                errors.push(format!("Imported role \"{}\" is missing budget(...) — budgets are required for loop safety.", r.name));
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

            let rules = Self::compile_block(&h.body, &let_bindings, &mut errors);
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

    fn compile_block(
        stmts: &[parser::ast::Stmt],
        let_bindings: &HashMap<String, &parser::ast::LetConstructor>,
        errors: &mut Vec<String>,
    ) -> Vec<ir::Rule> {
        let mut rules = Vec::new();
        for stmt in stmts {
            match stmt {
                parser::ast::Stmt::If { cond, then_branch, else_branch, .. } => {
                    let condition = Some(Self::compile_expr(cond));
                    let actions = Self::compile_actions(then_branch, let_bindings, errors);
                    rules.push(ir::Rule { condition, actions });

                    if let Some(else_stmts) = else_branch {
                        let not_cond = ir::Expr::Not(Box::new(Self::compile_expr(cond)));
                        let else_actions = Self::compile_actions(else_stmts, let_bindings, errors);
                        rules.push(ir::Rule { condition: Some(not_cond), actions: else_actions });
                    }
                }
                parser::ast::Stmt::ActionCall { name, args, .. } => {
                    let action = Self::compile_action(name, args, let_bindings, errors);
                    if let Some(a) = action {
                        rules.push(ir::Rule { condition: None, actions: vec![a] });
                    }
                }
            }
        }
        rules
    }

    fn compile_actions(
        stmts: &[parser::ast::Stmt],
        let_bindings: &HashMap<String, &parser::ast::LetConstructor>,
        errors: &mut Vec<String>,
    ) -> Vec<ir::ActionIR> {
        let mut actions = Vec::new();
        for stmt in stmts {
            match stmt {
                parser::ast::Stmt::ActionCall { name, args, .. } => {
                    if let Some(a) = Self::compile_action(name, args, let_bindings, errors) {
                        actions.push(a);
                    }
                }
                parser::ast::Stmt::If { then_branch, .. } => {
                    actions.extend(Self::compile_actions(then_branch, let_bindings, errors));
                }
                _ => {}
            }
        }
        actions
    }

    fn compile_action(
        name: &str,
        args: &[parser::ast::ActionArg],
        let_bindings: &HashMap<String, &parser::ast::LetConstructor>,
        errors: &mut Vec<String>,
    ) -> Option<ir::ActionIR> {
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

            "require" => {
                // require(ev) / require(vl) / require(fn) — activate a fact at statement level
                let fact_arg = args.first().and_then(|a| match &a.value {
                    parser::ast::Expr::Ident(name, _) => Some(name.clone()),
                    _ => None,
                });
                if let Some(fact_name) = fact_arg {
                    if let Some(ctor) = let_bindings.get(&fact_name) {
                        return match ctor {
                            parser::ast::LetConstructor::Evidence(text) => {
                                Some(ir::ActionIR::RequireEvidence(text.clone()))
                            }
                            parser::ast::LetConstructor::FinalNote(text) => {
                                Some(ir::ActionIR::RequireFinalNote(text.clone()))
                            }
                            parser::ast::LetConstructor::Validation(argv) => {
                                Some(ir::ActionIR::RequireValidation {
                                    argv: argv.clone(),
                                    reason: format!("required: {}", fact_name),
                                })
                            }
                            parser::ast::LetConstructor::Require(_) => {
                                errors.push(format!(
                                    "require() cannot wrap another require(). Use require({}) directly.", fact_name
                                ));
                                None
                            }
                        };
                    }
                }
                errors.push(format!(
                    "require() expects a fact name (evidence, validation, or final_note binding)"
                ));
                None
            }

            "block_finish_until" => {
                let waiver_allowed = true;

                // Resolve condition=require(ev) or condition=r (let r = require(ev)).
                // The condition arg's value can be:
                //   Ident("r")            → file-scope let binding
                //   MethodCall("require") → inline require(ev)
                fn resolve_require_expr<'a>(
                    expr: &'a parser::ast::Expr,
                    let_bindings: &'a HashMap<String, &parser::ast::LetConstructor>,
                ) -> Option<(&'static str, &'a str)> {
                    match expr {
                        // case 1: condition=r where r is a let binding
                        parser::ast::Expr::Ident(name, _) => {
                            if let Some(ctor) = let_bindings.get(name) {
                                match ctor {
                                    parser::ast::LetConstructor::Require(fact_name) => {
                                        if let Some(fact) = let_bindings.get(fact_name) {
                                            match fact {
                                                parser::ast::LetConstructor::Evidence(t) => Some(("evidence", t.as_str())),
                                                parser::ast::LetConstructor::FinalNote(t) => Some(("final_note", t.as_str())),
                                                _ => None,
                                            }
                                        } else { None }
                                    }
                                    _ => None,
                                }
                            } else { None }
                        }
                        // case 2: condition=require(ev) — inline
                        parser::ast::Expr::MethodCall { object, method, args, .. } 
                            if method == "__call__" => 
                        {
                            if let parser::ast::Expr::Ident(callee, _) = object.as_ref() {
                                if callee == "require" {
                                    // require(ev) where ev is an Ident
                                    if let Some(inner) = args.first() {
                                        if let parser::ast::Expr::Ident(fact_name, _) = inner {
                                            if let Some(fact) = let_bindings.get(fact_name) {
                                                match fact {
                                                    parser::ast::LetConstructor::Evidence(t) => Some(("evidence", t.as_str())),
                                                    parser::ast::LetConstructor::FinalNote(t) => Some(("final_note", t.as_str())),
                                                    _ => None,
                                                }
                                            } else { None }
                                        } else { None }
                                    } else { None }
                                } else { None }
                            } else { None }
                        }
                        _ => None,
                    }
                }

                let cond_expr = args.first().map(|a| &a.value);
                let resolved = cond_expr.and_then(|e| resolve_require_expr(e, let_bindings));

                if let Some((kind, text)) = resolved {
                    match kind {
                        "evidence" => {
                            return Some(ir::ActionIR::BlockFinishUntil {
                                condition: super::directive::FinishCondition::EvidencePresent(text.to_string()),
                                waiver_allowed,
                                with_evidence: Some(text.to_string()),
                                with_final_note: None,
                            });
                        }
                        "final_note" => {
                            return Some(ir::ActionIR::BlockFinishUntil {
                                condition: super::directive::FinishCondition::FinalNotePresent,
                                waiver_allowed,
                                with_evidence: None,
                                with_final_note: Some(text.to_string()),
                            });
                        }
                        _ => {}
                    }
                }

                // Fallback: old-style string condition
                let condition = get_str("condition").or_else(|| get_str("text")).unwrap_or_default();
                Some(ir::ActionIR::BlockFinishUntil {
                    condition: super::directive::FinishCondition::EvidencePresent(condition),
                    waiver_allowed,
                    with_evidence: None,
                    with_final_note: None,
                })
            }

            "print" | "println" => {
                errors.push(format!("\"{}\" is not available in .dhook. Use hint(), warn(), or audit() to provide feedback to the user or log information.", name));
                None
            }
            "import" | "from" | "as" => {
                errors.push(format!("\"{}\" is not supported in .dhook files. The DSL is self-contained and does not support imports.", name));
                None
            }
            "class" | "def" => {
                // `def` is handled by the parser for handler/role definitions.
                // If it reaches the compiler, it's in an unexpected context.
                errors.push(format!("Unexpected \"{}\". Function definitions are only allowed after @on(\"...\") or @role(\"...\") decorators.", name));
                None
            }
            "while" | "for" => {
                errors.push(format!("\"{}\" loops are not supported in .dhook v0.1. Only if/elif/else conditional statements are available.", name));
                None
            }
            "try" | "except" | "finally" | "raise" => {
                errors.push(format!("\"{}\" is not supported in .dhook. The DSL does not use exceptions for control flow.", name));
                None
            }
            _ => {
                errors.push(format!("Unknown action \"{}\". Supported actions: {}",
                    name, KNOWN_ACTIONS.join(", ")));
                None
            }
        }
    }

    /// Check if an expression is an observer field access chain (observer.x.y).
    /// Returns Some(field_path) where field_path contains ["name"] or ["output", ...].
    fn extract_observer_path(expr: &parser::ast::Expr) -> Option<Vec<String>> {
        match expr {
            parser::ast::Expr::MemberAccess { object, member, .. } => {
                match object.as_ref() {
                    parser::ast::Expr::Ident(name, _) if name == ir::OBSERVER_BASE => {
                        Some(vec![member.clone()])
                    }
                    parser::ast::Expr::MemberAccess { .. } => {
                        if let Some(mut fields) = Self::extract_observer_path(object) {
                            fields.push(member.clone());
                            Some(fields)
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
            parser::ast::Expr::Ident(name, _) => {
                if name == "changed_files" {
                    ir::Expr::ChangedFiles
                } else {
                    ir::Expr::Ident(name.clone())
                }
            }

            parser::ast::Expr::MethodCall { object, method, args, .. } => {
                // Detect known method patterns
                match method.as_str() {
                    "any_match" | "all_match" => {
                        let pattern = args.first().map(|a| Self::compile_expr(a));
                        match pattern {
                            Some(ir::Expr::String(p)) if method == "any_match" => {
                                ir::Expr::FilesMatch(ir::FileMatch::AnyMatch(p))
                            }
                            Some(ir::Expr::String(p)) => {
                                ir::Expr::FilesMatch(ir::FileMatch::AllMatch(p))
                            }
                            _ => ir::Expr::Bool(false),
                        }
                    }
                    "__call__" => {
                        // Function-call syntax like plan_contains("x") — unknown function
                        ir::Expr::Bool(false)
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
                let strings: Vec<String> = items.iter()
                    .filter_map(|e| match e {
                        parser::ast::Expr::String(s, _) => Some(s.clone()),
                        _ => None,
                    })
                    .collect();
                if strings.is_empty() {
                    // If no strings found, compile as list of generic expressions (truth check)
                    items.first().map(Self::compile_expr).unwrap_or(ir::Expr::Bool(false))
                } else {
                    ir::Expr::StringList(strings)
                }
            }

            parser::ast::Expr::MemberAccess { object, member, .. } => {
                // Detect observer field access: observer.name or observer.output.field
                if let Some(field_path) = Self::extract_observer_path(expr) {
                    let path: Vec<String> = field_path;
                    if path == ["name"] {
                        ir::Expr::Observer(ir::ObserverField::Name)
                    } else if path.first().map(|s| s.as_str()) == Some("output") {
                        ir::Expr::Observer(ir::ObserverField::Output(
                            path[1..].to_vec()
                        ))
                    } else {
                        ir::Expr::Bool(false)
                    }
                } else {
                    // Non-observer member access cannot be resolved at compile time.
                    ir::Expr::Bool(false)
                }
            }

            parser::ast::Expr::Dict(_, _) => ir::Expr::Bool(false),
        }
    }

    // ── Import resolution ──

    /// Resolve all imports in parallel: parse referenced files, extract
    /// requested symbols, return merged (groups, roles).
    fn resolve_imports(
        imports: &[ImportStmt],
        _current_module: &Module,
        base_dir: Option<&Path>,
        errors: &mut Vec<String>,
    ) -> (Vec<parser::ast::PathGroup>, Vec<parser::ast::RoleDef>) {
        let mut out_groups: Vec<parser::ast::PathGroup> = Vec::new();
        let mut out_roles: Vec<parser::ast::RoleDef> = Vec::new();

        for imp in imports {
            let resolved = Self::resolve_import_path(&imp.path, base_dir, errors);
            let Some(file_path) = resolved else { continue };

            let source = match std::fs::read_to_string(&file_path) {
                Ok(s) => s,
                Err(e) => {
                    errors.push(format!(
                        "Cannot read imported file \"{}\" (from \"{}\"): {}",
                        file_path.display(), imp.path, e
                    ));
                    continue;
                }
            };

            let mut p = parser::Parser::new(&source);
            let imported_module = match p.parse_module() {
                Ok(m) => m,
                Err(parse_errs) => {
                    for pe in parse_errs {
                        errors.push(format!(
                            "Parse error in imported file \"{}\" (from \"{}\") line {}: {}",
                            file_path.display(), imp.path, pe.span.line, pe.message
                        ));
                    }
                    continue;
                }
            };

            // Extract requested symbols
            for sym in &imp.symbols {
                match sym {
                    ImportSymbol::Group(name) => {
                        let found = imported_module.groups.iter()
                            .find(|g| g.name == *name)
                            .cloned();
                        match found {
                            Some(g) => out_groups.push(g),
                            None => errors.push(format!(
                                "Group \"{}\" not found in imported file \"{}\"",
                                name, imp.path
                            )),
                        }
                    }
                    ImportSymbol::Role(name) => {
                        let found = imported_module.roles.iter()
                            .find(|r| r.name == *name)
                            .cloned();
                        match found {
                            Some(r) => out_roles.push(r),
                            None => errors.push(format!(
                                "Role \"{}\" not found in imported file \"{}\"",
                                name, imp.path
                            )),
                        }
                    }
                }
            }
        }

        (out_groups, out_roles)
    }

    /// Resolve an import path to an absolute file path.
    /// Tries: base_dir / "{path}.dhook", then ~/.di/hooks/{path}.dhook
    fn resolve_import_path(
        import_path: &str,
        base_dir: Option<&Path>,
        errors: &mut Vec<String>,
    ) -> Option<PathBuf> {
        let filename = if import_path.ends_with(".dhook") {
            import_path.to_string()
        } else {
            format!("{}.dhook", import_path)
        };

        // 1. Try relative to base_dir
        if let Some(base) = base_dir {
            let candidate = base.join(&filename);
            if candidate.exists() {
                return Some(candidate);
            }
        }

        // 2. Try the current directory
        let cwd = PathBuf::from(".");
        let candidate = cwd.join(&filename);
        if candidate.exists() {
            return Some(candidate);
        }

        // 3. Try ~/.di/hooks/
        if let Ok(home) = std::env::var("HOME") {
            let candidate = PathBuf::from(home).join(".di").join("hooks").join(&filename);
            if candidate.exists() {
                return Some(candidate);
            }
        }

        errors.push(format!(
            "Import \"{}\" not found. Tried: {}{}",
            import_path,
            base_dir.map(|b| format!("{}/, ", b.join(&filename).display())).unwrap_or_default(),
            format!(".di/hooks/{}", filename),
        ));
        None
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
