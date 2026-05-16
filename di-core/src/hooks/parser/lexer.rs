use crate::hooks::parser::ast::Span;

#[derive(Debug, Clone, PartialEq)]
pub enum TokenKind {
    Def, If, Elif, Else, In, And, Or, Not, True, False, For, Return,
    AtOn, AtRole,
    Ident(String), String(String), Int(i64),
    LParen, RParen, LBracket, RBracket, LBrace, RBrace,
    Comma, Colon, Dot, Eq, EqEq, Neq, Newline, Indent, Dedent, Eof,
}

#[derive(Debug, Clone)]
pub struct Token {
    pub kind: TokenKind,
    pub span: Span,
}

pub struct Lexer<'a> {
    chars: std::iter::Peekable<std::str::Chars<'a>>,
    pos: usize,
    line: usize,
    col: usize,
    indent_stack: Vec<usize>,
    pub(crate) pending: Vec<Token>,
    bol: bool,
    /// Set when an unterminated string literal is detected. The string
    /// content up to EOF is still returned as a token, but this flag
    /// allows the parser to report an error.
    pub unterminated_string: bool,
    /// First non-zero indent width detected. Used to validate consistent
    /// indentation (e.g., don't mix 2-space and 4-space indents).
    indent_width: usize,
}

impl<'a> Lexer<'a> {
    pub fn new(source: &'a str) -> Self {
        Self {
            chars: source.chars().peekable(),
            pos: 0,
            line: 1,
            col: 1,
            indent_stack: vec![0],
            pending: Vec::new(),
            bol: true,
            unterminated_string: false,
            indent_width: 0,
        }
    }

    fn span(&self, start: usize, line: usize, col: usize) -> Span {
        Span { start, end: self.pos, line, column: col }
    }

    fn advance(&mut self) -> Option<char> {
        let c = self.chars.next()?;
        if c == '\n' {
            self.line += 1;
            self.col = 1;
        } else {
            self.col += 1;
        }
        self.pos += 1;
        Some(c)
    }

    fn peek(&mut self) -> Option<char> {
        self.chars.peek().copied()
    }

    fn emit(&mut self, kind: TokenKind, start: usize, line: usize, col: usize) {
        self.pending.push(Token { kind, span: self.span(start, line, col) });
    }

    fn skip_whitespace(&mut self) {
        while let Some(c) = self.peek() {
            if c == ' ' || c == '\t' || c == '\r' {
                self.advance();
            } else {
                break;
            }
        }
    }

    /// Read remaining characters of an identifier (after the first char has been consumed).
    fn read_ident_rest(&mut self) -> String {
        let mut s = String::new();
        while let Some(c) = self.peek() {
            if c.is_alphanumeric() || c == '_' {
                s.push(self.advance().unwrap());
            } else {
                break;
            }
        }
        s
    }

    fn read_string(&mut self, quote: char) -> String {
        let mut s = String::new();
        let mut closed = false;
        while let Some(c) = self.advance() {
            if c == '\\' {
                if let Some(next) = self.advance() {
                    match next {
                        'n' => s.push('\n'),
                        't' => s.push('\t'),
                        '\\' => s.push('\\'),
                        '"' => s.push('"'),
                        '\'' => s.push('\''),
                        _ => { s.push('\\'); s.push(next); }
                    }
                }
            } else if c == quote {
                closed = true;
                break;
            } else {
                s.push(c);
            }
        }
        if !closed {
            self.unterminated_string = true;
        }
        s
    }

    fn read_int(&mut self, start: char) -> i64 {
        let mut s = String::new();
        s.push(start);
        while let Some(c) = self.peek() {
            if c.is_ascii_digit() {
                s.push(self.advance().unwrap());
            } else {
                break;
            }
        }
        match s.parse::<i64>() {
            Ok(n) => n,
            Err(_) => {
                if s.starts_with('-') {
                    i64::MIN
                } else {
                    i64::MAX
                }
            }
        }
    }

    fn ident_or_keyword(&self, word: &str) -> TokenKind {
        match word {
            "def" => TokenKind::Def,
            "if" => TokenKind::If,
            "elif" => TokenKind::Elif,
            "else" => TokenKind::Else,
            "in" => TokenKind::In,
            "and" => TokenKind::And,
            "or" => TokenKind::Or,
            "not" => TokenKind::Not,
            "True" => TokenKind::True,
            "False" => TokenKind::False,
            "for" => TokenKind::For,
            "return" => TokenKind::Return,
            _ => TokenKind::Ident(word.to_string()),
        }
    }

    fn handle_indent(&mut self, indent: usize) {
        // Record base indent width on first non-zero indent
        if self.indent_width == 0 && indent > 0 {
            self.indent_width = indent;
        }

        let current = *self.indent_stack.last().unwrap();

        // Validate consistent indent width: if we have a base width,
        // the new indent must be a multiple of it. If not, adjust to
        // the nearest valid level to prevent silent parse corruption.
        let effective = if self.indent_width > 0 && indent > 0 && indent % self.indent_width != 0 {
            // Round to nearest multiple
            let rounded = (indent / self.indent_width) * self.indent_width;
            if rounded > current {
                rounded
            } else {
                indent // use actual value for dedent
            }
        } else {
            indent
        };

        if effective > current {
            self.indent_stack.push(effective);
            let start = self.pos;
            self.emit(TokenKind::Indent, start, self.line, self.col);
        } else if effective < current {
            while let Some(&top) = self.indent_stack.last() {
                if top == effective { break; }
                self.indent_stack.pop();
                let start = self.pos;
                self.emit(TokenKind::Dedent, start, self.line, self.col);
            }
        }
    }

    pub fn next_token(&mut self) -> Token {
        if let Some(tok) = self.pending.pop() {
            return tok;
        }

        loop {
            if self.bol {
                self.skip_whitespace();
                let indent = self.col.saturating_sub(1);
                self.handle_indent(indent);
                self.bol = false;
                if let Some(tok) = self.pending.pop() {
                    return tok;
                }
            }

            match self.advance() {
                None => {
                    self.handle_indent(0);
                    if self.pending.is_empty() {
                        return Token { kind: TokenKind::Eof, span: self.span(self.pos, self.line, self.col) };
                    }
                    return self.pending.pop().unwrap();
                }
                Some(c) => {
                    let start = self.pos - 1;
                    let line = self.line;
                    let col = self.col - 1;

                    match c {
                        ' ' | '\t' | '\r' => continue,
                        '\n' => {
                            self.bol = true;
                            self.emit(TokenKind::Newline, start, line, col);
                            return self.pending.pop().unwrap();
                        }
                        '#' => {
                            while let Some(ch) = self.peek() {
                                if ch == '\n' { break; }
                                self.advance();
                            }
                            continue;
                        }
                        '"' | '\'' => {
                            let s = self.read_string(c);
                            return Token { kind: TokenKind::String(s), span: self.span(start, line, col) };
                        }
                        '(' => return Token { kind: TokenKind::LParen, span: self.span(start, line, col) },
                        ')' => return Token { kind: TokenKind::RParen, span: self.span(start, line, col) },
                        '[' => return Token { kind: TokenKind::LBracket, span: self.span(start, line, col) },
                        ']' => return Token { kind: TokenKind::RBracket, span: self.span(start, line, col) },
                        '{' => return Token { kind: TokenKind::LBrace, span: self.span(start, line, col) },
                        '}' => return Token { kind: TokenKind::RBrace, span: self.span(start, line, col) },
                        ',' => return Token { kind: TokenKind::Comma, span: self.span(start, line, col) },
                        ':' => return Token { kind: TokenKind::Colon, span: self.span(start, line, col) },
                        '.' => return Token { kind: TokenKind::Dot, span: self.span(start, line, col) },

                        '@' if self.peek() == Some('r') => {
                            self.advance(); // consume 'r'
                            let rest = self.read_ident_rest();
                            if rest == "ole" { // r + ole = role
                                return Token { kind: TokenKind::AtRole, span: self.span(start, line, col) };
                            }
                            return Token { kind: TokenKind::Ident(format!("@r{}", rest)), span: self.span(start, line, col) };
                        }
                        '@' if self.peek() == Some('o') => {
                            self.advance(); // consume 'o'
                            let rest = self.read_ident_rest();
                            if rest == "n" { // o + n = on
                                return Token { kind: TokenKind::AtOn, span: self.span(start, line, col) };
                            }
                            return Token { kind: TokenKind::Ident(format!("@o{}", rest)), span: self.span(start, line, col) };
                        }

                        '=' if self.peek() == Some('=') => {
                            self.advance();
                            return Token { kind: TokenKind::EqEq, span: self.span(start, line, col) };
                        }
                        '!' if self.peek() == Some('=') => {
                            self.advance();
                            return Token { kind: TokenKind::Neq, span: self.span(start, line, col) };
                        }
                        '=' => return Token { kind: TokenKind::Eq, span: self.span(start, line, col) },

                        c if c.is_ascii_digit() => {
                            let n = self.read_int(c);
                            return Token { kind: TokenKind::Int(n), span: self.span(start, line, col) };
                        }
                        c if c.is_alphabetic() || c == '_' => {
                            let rest = self.read_ident_rest();
                            let mut word = String::new();
                            word.push(c);
                            word.push_str(&rest);
                            let kind = self.ident_or_keyword(&word);
                            return Token { kind, span: self.span(start, line, col) };
                        }
                    _ => {
                        return Token { kind: TokenKind::Ident(c.to_string()), span: self.span(start, line, col) };
                    }
                }
            }
        }
    }
}
}

