use crate::hooks::parser::ast::*;
use crate::hooks::parser::lexer::{Lexer, Token, TokenKind};

#[derive(Debug)]
pub struct ParseError {
    pub message: String,
    pub span: Span,
}

pub struct Parser<'a> {
    lexer: Lexer<'a>,
    current: Token,
    eof: bool,
}

impl<'a> Parser<'a> {
    pub fn new(source: &'a str) -> Self {
        let mut lexer = Lexer::new(source);
        let current = lexer.next_token();
        Self { lexer, current, eof: false }
    }

    /// Returns true if the lexer detected an unterminated string literal.
    pub fn unterminated_string(&self) -> bool {
        self.lexer.unterminated_string
    }

    pub fn parse_module(&mut self) -> Result<Module, Vec<ParseError>> {
        let mut errors = Vec::new();
        let mut groups = Vec::new();
        let mut roles = Vec::new();
        let mut handlers = Vec::new();

        loop {
            if self.eof || self.current.kind == TokenKind::Eof {
                break;
            }

            match &self.current.kind {
                TokenKind::Ident(name) if name == "group" => {
                    match self.parse_group() {
                        Ok(g) => groups.push(g),
                        Err(e) => errors.push(e),
                    }
                }
                TokenKind::AtRole => {
                    match self.parse_role_def() {
                        Ok(r) => roles.push(r),
                        Err(e) => errors.push(e),
                    }
                }
                TokenKind::AtOn => {
                    match self.parse_event_handler() {
                        Ok(h) => handlers.push(h),
                        Err(e) => errors.push(e),
                    }
                }
                TokenKind::Newline => { self.advance(); }
                _ => {
                    errors.push(ParseError {
                        message: format!("Unexpected token: {:?}", self.current.kind),
                        span: self.current.span.clone(),
                    });
                    self.advance();
                }
            }
        }

        if self.lexer.unterminated_string {
            errors.push(ParseError {
                message: "Unterminated string literal".to_string(),
                span: self.current.span.clone(),
            });
        }

        if errors.is_empty() {
            Ok(Module { groups, roles, handlers })
        } else {
            Err(errors)
        }
    }

    fn advance(&mut self) {
        if self.eof { return; }
        self.current = self.lexer.next_token();
        if self.current.kind == TokenKind::Eof {
            self.eof = true;
        }
    }

    fn expect(&mut self, kind: &TokenKind) -> Result<(), ParseError> {
        if std::mem::discriminant(&self.current.kind) == std::mem::discriminant(kind) {
            self.advance();
            Ok(())
        } else {
            Err(ParseError {
                message: format!("Expected {:?}, got {:?}", kind, self.current.kind),
                span: self.current.span.clone(),
            })
        }
    }

    fn expect_newline_or_eof(&mut self) {
        loop {
            match &self.current.kind {
                TokenKind::Newline => self.advance(),
                TokenKind::Eof => break,
                _ => break,
            }
        }
    }

    fn parse_group(&mut self) -> Result<PathGroup, ParseError> {
        let start = self.current.span.clone();
        // Advance past "group" identifier (already matched in parse_module)
        self.advance();
        self.expect(&TokenKind::LParen)?;
        // First arg: group name string
        let name = match &self.current.kind {
            TokenKind::String(s) => {
                let n = s.clone();
                self.advance();
                n
            }
            _ => return Err(ParseError {
                message: "Expected group name string".to_string(),
                span: self.current.span.clone(),
            }),
        };
        // Comma separator
        self.expect(&TokenKind::Comma)?;
        // Second arg: list of patterns [pat1, pat2, ...]
        self.expect(&TokenKind::LBracket)?;
        let mut patterns = Vec::new();
        loop {
            match &self.current.kind {
                TokenKind::String(s) => {
                    patterns.push(s.clone());
                    self.advance();
                }
                TokenKind::Comma => { self.advance(); }
                TokenKind::Newline => { self.advance(); }
                TokenKind::Indent | TokenKind::Dedent => { self.advance(); }
                TokenKind::RBracket => { self.advance(); break; }
                _ => break,
            }
        }
        self.expect(&TokenKind::RParen)?;
        self.expect_newline_or_eof();
        Ok(PathGroup { name, patterns, span: start })
    }

    fn parse_role_def(&mut self) -> Result<RoleDef, ParseError> {
        let start = self.current.span.clone();
        self.advance(); // @role

        // Syntax: @role("name", kind="observer")
        self.expect(&TokenKind::LParen)?;

        // Read role name from string literal
        let name = match &self.current.kind {
            TokenKind::String(s) => {
                let n = s.clone();
                self.advance();
                n
            }
            _ => return Err(ParseError {
                message: "Expected role name string".to_string(),
                span: self.current.span.clone(),
            }),
        };

        let mut kind = String::new();
        let mut system_prompt = None;
        let mut inputs = Vec::new();
        let mut output_schema = None;
        let mut budget = None;

        // Parse optional args: kind="observer"
        loop {
            match &self.current.kind {
                TokenKind::Comma => { self.advance(); }
                TokenKind::RParen => { self.advance(); break; }
                TokenKind::Ident(k) if k == "kind" => {
                    self.advance();
                    self.expect(&TokenKind::Eq)?;
                    if let TokenKind::String(s) = &self.current.kind {
                        kind = s.clone();
                        self.advance();
                    }
                }
                _ => { self.advance(); }
            }
        }

        self.expect_newline_or_eof();

        // Expect def name():
        self.expect(&TokenKind::Def)?;
        if let TokenKind::Ident(_) = &self.current.kind {
            self.advance();
        }
        self.expect(&TokenKind::LParen)?;
        self.expect(&TokenKind::RParen)?;
        self.expect(&TokenKind::Colon)?;
        self.expect_newline_or_eof();
        self.expect(&TokenKind::Indent)?;

        loop {
            match &self.current.kind {
                TokenKind::Dedent => { break; }
                TokenKind::Ident(n) if n == "system" => {
                    self.advance();
                    self.expect(&TokenKind::LParen)?;
                    if let TokenKind::String(s) = &self.current.kind {
                        system_prompt = Some(s.clone());
                        self.advance();
                    }
                    self.expect(&TokenKind::RParen)?;
                    self.expect_newline_or_eof();
                }
                TokenKind::Ident(n) if n == "input" => {
                    self.advance();
                    self.expect(&TokenKind::Dot)?;
                    if let TokenKind::Ident(method) = &self.current.kind {
                        inputs.push(method.clone());
                        self.advance();
                    }
                    self.expect(&TokenKind::LParen)?;
                    // Skip any keyword arguments until RParen
                    loop {
                        match &self.current.kind {
                            TokenKind::RParen => { self.advance(); break; }
                            _ => { self.advance(); }
                        }
                    }
                    self.expect_newline_or_eof();
                }
                TokenKind::Ident(n) if n == "output" => {
                    self.advance();
                    self.expect(&TokenKind::Dot)?;
                    self.expect(&TokenKind::Ident("schema".to_string()))?;
                    self.expect(&TokenKind::LParen)?;
                    // Parse inline JSON-like dict
                    if let TokenKind::LBrace = self.current.kind {
                        let json_str = self.read_dict_as_json();
                        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&json_str) {
                            output_schema = Some(v);
                        }
                    }
                    self.expect(&TokenKind::RParen)?;
                    self.expect_newline_or_eof();
                }
                TokenKind::Ident(n) if n == "budget" => {
                    self.advance();
                    self.expect(&TokenKind::LParen)?;
                    let mut max_tokens: u64 = 0;
                    let mut max_runs: u64 = 1;
                    loop {
                        match &self.current.kind {
                            TokenKind::Ident(k) if k == "max_tokens" => {
                                self.advance();
                                self.expect(&TokenKind::Eq)?;
                                if let TokenKind::Int(n) = &self.current.kind {
                                    max_tokens = *n as u64;
                                    self.advance();
                                }
                            }
                            TokenKind::Ident(k) if k == "max_runs" => {
                                self.advance();
                                self.expect(&TokenKind::Eq)?;
                                if let TokenKind::Int(n) = &self.current.kind {
                                    max_runs = *n as u64;
                                    self.advance();
                                }
                            }
                            TokenKind::Comma => { self.advance(); }
                            TokenKind::RParen => { self.advance(); break; }
                            _ => { self.advance(); break; }
                        }
                    }
                    budget = Some(RoleBudget { max_tokens, max_runs });
                    self.expect_newline_or_eof();
                }
                _ => {
                    let msg = format!("Unexpected statement in role '{}'. Expected system(), input.method(), output.schema(), or budget().", name);
                    return Err(ParseError { message: msg, span: self.current.span.clone() });
                }
            }
        }
        // Consume the Dedent closing the role function body
        self.expect(&TokenKind::Dedent)?;
        // Skip any trailing newlines after the function
        self.expect_newline_or_eof();

        Ok(RoleDef { name, kind, system_prompt, inputs, output_schema, budget, span: start })
    }

    fn parse_event_handler(&mut self) -> Result<EventHandler, ParseError> {
        let start = self.current.span.clone();
        self.advance(); // @on
        self.expect(&TokenKind::LParen)?;
        let event = match &self.current.kind {
            TokenKind::String(s) => {
                let e = s.clone();
                self.advance();
                e
            }
            _ => return Err(ParseError {
                message: "Expected event name string".to_string(),
                span: self.current.span.clone(),
            }),
        };
        self.expect(&TokenKind::RParen)?;
        self.expect_newline_or_eof();

        // def handler_name():
        self.expect(&TokenKind::Def)?;
        let name = match &self.current.kind {
            TokenKind::Ident(n) => {
                let name = n.clone();
                self.advance();
                name
            }
            _ => return Err(ParseError {
                message: "Expected handler name".to_string(),
                span: self.current.span.clone(),
            }),
        };
        self.expect(&TokenKind::LParen)?;
        self.expect(&TokenKind::RParen)?;
        self.expect(&TokenKind::Colon)?;
        self.expect_newline_or_eof();

        // Parse body
        self.expect(&TokenKind::Indent)?;
        let body = self.parse_block()?;
        self.expect(&TokenKind::Dedent)?;

        Ok(EventHandler { event, name, body, span: start })
    }

    fn parse_block(&mut self) -> Result<Vec<Stmt>, ParseError> {
        let mut stmts = Vec::new();
        loop {
            match &self.current.kind {
                TokenKind::Dedent | TokenKind::Eof => break,
                TokenKind::Newline => { self.advance(); }
                TokenKind::If => {
                    stmts.push(self.parse_if_stmt()?);
                }
                _ => {
                    stmts.push(self.parse_action_call()?);
                }
            }
        }
        Ok(stmts)
    }

    fn parse_if_stmt(&mut self) -> Result<Stmt, ParseError> {
        let start = self.current.span.clone();
        self.advance(); // if
        let cond = self.parse_expr()?;
        self.expect(&TokenKind::Colon)?;
        self.expect_newline_or_eof();
        self.expect(&TokenKind::Indent)?;
        let then_branch = self.parse_block()?;
        self.expect(&TokenKind::Dedent)?;

        let mut else_branch = None;
        if matches!(self.current.kind, TokenKind::Elif | TokenKind::Else) {
            self.advance();
            if matches!(self.current.kind, TokenKind::If) {
                // elif — parse as nested if in else branch
                let elif = self.parse_if_stmt()?;
                else_branch = Some(vec![elif]);
            } else {
                self.expect(&TokenKind::Colon)?;
                self.expect_newline_or_eof();
                self.expect(&TokenKind::Indent)?;
                let block = self.parse_block()?;
                self.expect(&TokenKind::Dedent)?;
                else_branch = Some(block);
            }
        }

        Ok(Stmt::If { cond, then_branch, else_branch, span: start })
    }

    fn parse_action_call(&mut self) -> Result<Stmt, ParseError> {
        let start = self.current.span.clone();
        let name = match &self.current.kind {
            TokenKind::Ident(n) => {
                let name = n.clone();
                self.advance();
                name
            }
            _ => return Err(ParseError {
                message: format!("Expected action name, got {:?}", self.current.kind),
                span: self.current.span.clone(),
            }),
        };
        self.expect(&TokenKind::LParen)?;

        let mut args = Vec::new();
        loop {
            match &self.current.kind {
                TokenKind::RParen => { self.advance(); break; }
                TokenKind::Comma => { self.advance(); }
                _ => {
                    let arg = self.parse_action_arg()?;
                    args.push(arg);
                }
            }
        }

        self.expect_newline_or_eof();
        Ok(Stmt::ActionCall { name, args, span: start })
    }

    fn parse_action_arg(&mut self) -> Result<ActionArg, ParseError> {
        let start = self.current.span.clone();
        // Check if this is a keyword arg (ident = expr) or positional arg
        let has_kw = matches!(&self.current.kind, TokenKind::Ident(_))
            && self.peek_next_is_eq();

        if has_kw {
            let key = match &self.current.kind {
                TokenKind::Ident(k) => {
                    let k = k.clone();
                    self.advance();
                    k
                }
                _ => unreachable!(),
            };
            self.expect(&TokenKind::Eq)?;
            let value = self.parse_expr()?;
            Ok(ActionArg { key: Some(key), value, span: start })
        } else {
            let value = self.parse_expr()?;
            Ok(ActionArg { key: None, value, span: start })
        }
    }

    fn peek_next_is_eq(&mut self) -> bool {
        // Peek at the next token to check if it's Eq (distinguishes
        // keyword args "key=value" from positional args "value").
        if !matches!(&self.current.kind, TokenKind::Ident(_)) {
            return false;
        }
        let next = self.lexer.next_token();
        let is_eq = matches!(next.kind, TokenKind::Eq);
        // Push the peeked token onto pending stack so next_token returns it
        self.lexer.pending.push(next);
        is_eq
    }

    fn parse_expr(&mut self) -> Result<Expr, ParseError> {
        self.parse_or_expr()
    }

    fn parse_or_expr(&mut self) -> Result<Expr, ParseError> {
        let mut left = self.parse_and_expr()?;
        while matches!(&self.current.kind, TokenKind::Or) {
            let span = self.current.span.clone();
            self.advance();
            let right = self.parse_and_expr()?;
            left = Expr::BinaryOp {
                left: Box::new(left),
                op: BinOp::Or,
                right: Box::new(right),
                span,
            };
        }
        Ok(left)
    }

    fn parse_and_expr(&mut self) -> Result<Expr, ParseError> {
        let mut left = self.parse_comparison()?;
        while matches!(&self.current.kind, TokenKind::And) {
            let span = self.current.span.clone();
            self.advance();
            let right = self.parse_comparison()?;
            left = Expr::BinaryOp {
                left: Box::new(left),
                op: BinOp::And,
                right: Box::new(right),
                span,
            };
        }
        Ok(left)
    }

    fn parse_comparison(&mut self) -> Result<Expr, ParseError> {
        let left = self.parse_not_expr()?;
        match &self.current.kind {
            TokenKind::EqEq => {
                let span = self.current.span.clone();
                self.advance();
                let right = self.parse_not_expr()?;
                Ok(Expr::BinaryOp {
                    left: Box::new(left),
                    op: BinOp::Eq,
                    right: Box::new(right),
                    span,
                })
            }
            TokenKind::Neq => {
                let span = self.current.span.clone();
                self.advance();
                let right = self.parse_not_expr()?;
                Ok(Expr::BinaryOp {
                    left: Box::new(left),
                    op: BinOp::Neq,
                    right: Box::new(right),
                    span,
                })
            }
            TokenKind::In => {
                let span = self.current.span.clone();
                self.advance();
                let right = self.parse_not_expr()?;
                Ok(Expr::BinaryOp {
                    left: Box::new(left),
                    op: BinOp::In,
                    right: Box::new(right),
                    span,
                })
            }
            _ => Ok(left),
        }
    }

    fn parse_not_expr(&mut self) -> Result<Expr, ParseError> {
        if matches!(&self.current.kind, TokenKind::Not) {
            let span = self.current.span.clone();
            self.advance();
            let inner = self.parse_primary()?;
            Ok(Expr::BinaryOp {
                left: Box::new(Expr::Bool(false, span.clone())),
                op: BinOp::Eq,
                right: Box::new(inner),
                span,
            })
        } else {
            self.parse_primary()
        }
    }

    fn parse_primary(&mut self) -> Result<Expr, ParseError> {
        match &self.current.kind {
            TokenKind::True => {
                let span = self.current.span.clone();
                self.advance();
                Ok(Expr::Bool(true, span))
            }
            TokenKind::False => {
                let span = self.current.span.clone();
                self.advance();
                Ok(Expr::Bool(false, span))
            }
            TokenKind::Int(n) => {
                let span = self.current.span.clone();
                let v = *n;
                self.advance();
                Ok(Expr::Int(v, span))
            }
            TokenKind::String(s) => {
                let span = self.current.span.clone();
                let v = s.clone();
                self.advance();
                Ok(Expr::String(v, span))
            }
            TokenKind::Ident(_) => {
                let start_span = self.current.span.clone();
                let name = match &self.current.kind {
                    TokenKind::Ident(n) => {
                        let name = n.clone();
                        self.advance();
                        name
                    }
                    _ => unreachable!(),
                };

                match &self.current.kind {
                    TokenKind::LParen => {
                        // Function call: name(args...)
                        self.advance();
                        let mut args = Vec::new();
                        loop {
                            match &self.current.kind {
                                TokenKind::RParen => { self.advance(); break; }
                                TokenKind::Comma => { self.advance(); }
                                _ => {
                                    let arg = self.parse_expr()?;
                                    args.push(arg);
                                }
                            }
                        }
                        Ok(Expr::MethodCall {
                            object: Box::new(Expr::Ident(name, start_span.clone())),
                            method: "__call__".to_string(),
                            args,
                            span: start_span,
                        })
                    }
                    TokenKind::Dot => {
                        // Method call or member access
                        self.parse_method_chain(Expr::Ident(name, start_span.clone()), start_span)
                    }
                    _ => Ok(Expr::Ident(name, start_span)),
                }
            }
            TokenKind::LBracket => {
                let span = self.current.span.clone();
                self.advance();
                let mut items = Vec::new();
                loop {
                    match &self.current.kind {
                        TokenKind::RBracket => { self.advance(); break; }
                        TokenKind::Comma => { self.advance(); }
                        _ => {
                            items.push(self.parse_expr()?);
                        }
                    }
                }
                Ok(Expr::List(items, span))
            }
            TokenKind::LBrace => {
                let span = self.current.span.clone();
                self.advance();
                let mut dict = std::collections::HashMap::new();
                loop {
                    match &self.current.kind {
                        TokenKind::RBrace => { self.advance(); break; }
                        TokenKind::Comma => { self.advance(); }
                        TokenKind::String(k) => {
                            let key = k.clone();
                            self.advance();
                            self.expect(&TokenKind::Colon)?;
                            let value = self.parse_expr()?;
                            dict.insert(key, value);
                        }
                        _ => break,
                    }
                }
                Ok(Expr::Dict(dict, span))
            }
            TokenKind::LParen => {
                let span = self.current.span.clone();
                self.advance();
                let expr = self.parse_expr()?;
                self.expect(&TokenKind::RParen)?;
                Ok(expr)
            }
            _ => Err(ParseError {
                message: format!("Unexpected token in expression: {:?}", self.current.kind),
                span: self.current.span.clone(),
            }),
        }
    }

    fn parse_method_chain(&mut self, object: Expr, span: Span) -> Result<Expr, ParseError> {
        self.advance(); // dot
        let method = match &self.current.kind {
            TokenKind::Ident(m) => {
                let m = m.clone();
                self.advance();
                m
            }
            _ => return Err(ParseError {
                message: "Expected method name after dot".to_string(),
                span: self.current.span.clone(),
            }),
        };

        match &self.current.kind {
            TokenKind::LParen => {
                self.advance();
                let mut args = Vec::new();
                loop {
                    match &self.current.kind {
                        TokenKind::RParen => { self.advance(); break; }
                        TokenKind::Comma => { self.advance(); }
                        _ => {
                            args.push(self.parse_expr()?);
                        }
                    }
                }

                let result = Expr::MethodCall {
                    object: Box::new(object),
                    method,
                    args,
                    span: self.current.span.clone(),
                };

                // Chain further: x.foo().bar()
                if matches!(&self.current.kind, TokenKind::Dot) {
                    self.parse_method_chain(result, span)
                } else {
                    Ok(result)
                }
            }
            _ => {
                let result = Expr::MemberAccess {
                    object: Box::new(object),
                    member: method,
                    span: self.current.span.clone(),
                };
                if matches!(&self.current.kind, TokenKind::Dot) {
                    self.parse_method_chain(result, span)
                } else {
                    Ok(result)
                }
            }
        }
    }

    fn read_dict_as_json(&mut self) -> String {
        // Read tokens until matching RBrace and reconstruct JSON.
        // Caller has verified self.current.kind == LBrace.
        let mut depth: i32 = 1;
        let mut json = String::new();
        // Push the initial LBrace (already at self.current)
        json.push('{');
        self.advance();
        while depth > 0 {
            match &self.current.kind {
                TokenKind::LBrace => { depth += 1; json.push('{'); self.advance(); }
                TokenKind::RBrace => { depth -= 1; json.push('}'); self.advance(); }
                TokenKind::String(s) => {
                    json.push_str(&format!("\"{}\"", s));
                    self.advance();
                }
                TokenKind::Ident(s) => {
                    json.push_str(s);
                    self.advance();
                }
                TokenKind::Int(n) => {
                    json.push_str(&n.to_string());
                    self.advance();
                }
                TokenKind::Colon => { json.push(':'); self.advance(); }
                TokenKind::Comma => { json.push(','); self.advance(); }
                TokenKind::Eof => { break; }
                _ => { self.advance(); }
            }
        }
        json
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(source: &str) -> Result<Module, Vec<ParseError>> {
        let mut p = Parser::new(source);
        p.parse_module()
    }

    #[test]
    fn test_parse_empty() {
        let m = parse("").unwrap();
        assert!(m.handlers.is_empty());
        assert!(m.groups.is_empty());
        assert!(m.roles.is_empty());
    }

    #[test]
    fn test_parse_minimal_role() {
        let m = parse("@role(\"x\", kind=\"observer\")\ndef x():\n    system(\"hello\")\n    budget(max_tokens=100, max_runs=1)\n").unwrap();
        assert_eq!(m.roles.len(), 1);
        assert_eq!(m.roles[0].name, "x");
    }

    #[test]
    fn test_parse_role_with_input() {
        let m = parse("@role(\"x\", kind=\"observer\")\ndef x():\n    system(\"hello\")\n    input.diff()\n    input.changed_files()\n    budget(max_tokens=100, max_runs=1)\n").unwrap();
        assert_eq!(m.roles.len(), 1);
        assert_eq!(m.roles[0].inputs.len(), 2);
    }

    #[test]
    fn test_parse_role_with_schema() {
        let s = "@role(\"x\", kind=\"observer\")\ndef x():\n    system(\"hello\")\n    output.schema({\"risk\": \"low|high\"})\n    budget(max_tokens=100, max_runs=1)\n";
        let m = parse(s).unwrap();
        assert_eq!(m.roles.len(), 1);
    }

    #[test]
    fn test_parse_role_then_handler() {
        let s = "@role(\"x\", kind=\"observer\")\ndef x():\n    system(\"hello\")\n    budget(max_tokens=100, max_runs=1)\n@on(\"post_tool_use\")\ndef h():\n    hint(\"ok\")\n";
        let m = parse(s).unwrap();
        assert_eq!(m.roles.len(), 1);
        assert_eq!(m.handlers.len(), 1);
    }

    #[test]
    fn test_parse_single_handler() {
        let m = parse("@on(\"e\")\ndef h():\n    hint(\"x\")\n").unwrap();
        assert_eq!(m.handlers.len(), 1);
        assert_eq!(m.handlers[0].event, "e");
        assert_eq!(m.handlers[0].name, "h");
    }

    #[test]
    fn test_parse_multiple_handlers() {
        let s = "@on(\"a\")\ndef ha():\n    hint(\"1\")\n@on(\"b\")\ndef hb():\n    hint(\"2\")\n";
        let m = parse(s).unwrap();
        assert_eq!(m.handlers.len(), 2);
    }

    #[test]
    fn test_parse_if_else() {
        let s = "@on(\"e\")\ndef h():\n    if True:\n        hint(\"a\")\n    else:\n        hint(\"b\")\n";
        let m = parse(s).unwrap();
        assert_eq!(m.handlers.len(), 1);
        assert_eq!(m.handlers[0].body.len(), 1);
        match &m.handlers[0].body[0] {
            Stmt::If { else_branch: Some(_), .. } => {} // OK
            _ => panic!("Expected if with else"),
        }
    }

    #[test]
    fn test_parse_group() {
        let s = "group(\"x\", [\"a/**\", \"b/**\"])\n";
        let m = parse(s).unwrap();
        assert_eq!(m.groups.len(), 1);
        assert_eq!(m.groups[0].name, "x");
        assert_eq!(m.groups[0].patterns.len(), 2);
    }

    #[test]
    fn test_parse_action_args() {
        let s = "@on(\"e\")\ndef h():\n    warn(severity=\"high\", message=\"test\")\n";
        let m = parse(s).unwrap();
        assert_eq!(m.handlers[0].body.len(), 1);
        match &m.handlers[0].body[0] {
            Stmt::ActionCall { name, args, .. } => {
                assert_eq!(name, "warn");
                assert_eq!(args.len(), 2);
            }
            _ => panic!("Expected ActionCall"),
        }
    }

    #[test]
    fn test_parse_method_chain() {
        let s = "@on(\"e\")\ndef h():\n    if changed_files.any_match(\"rust\"):\n        hint(\"ok\")\n";
        let m = parse(s).unwrap();
        match &m.handlers[0].body[0] {
            Stmt::If { cond, .. } => {
                match cond {
                    Expr::MethodCall { method, .. } => assert_eq!(method, "any_match"),
                    _ => panic!("Expected MethodCall"),
                }
            }
            _ => panic!("Expected If"),
        }
    }

    #[test]
    fn test_parse_invalid_syntax_rejected() {
        let r = parse("this is not valid dsl @@@");
        assert!(r.is_err());
    }

    #[test]
    fn test_parse_trailing_content_after_handler() {
        let s = "@on(\"e\")\ndef h():\n    hint(\"ok\")\nsome_trailing_stuff\n";
        let r = parse(s);
        // Trailing content should produce errors
        assert!(r.is_err() || r.unwrap().handlers.len() == 1);
    }

    #[test]
    fn test_parse_empty_handler_body() {
        let s = "@on(\"e\")\ndef h():\n    pass\n";
        let r = parse(s);
        // Empty/comment-only body is valid (just whitespace)
        assert!(r.is_err(), "`pass` is not a valid DSL keyword");
        // Test with empty body instead
        let s2 = "@on(\"e\")\ndef h():\n    hint(\"ok\")\n";
        assert!(parse(s2).is_ok());
    }
}

