//! Lexer de OpenS.
//!
//! Convierte texto en tokens. Nunca se detiene en el primer error: registra un
//! diagnóstico, se recupera y sigue, para reportar todos los problemas de una
//! vez (algo clave cuando el que corrige es un agente de IA).

use crate::diagnostic::{codes, Diagnostic};
use crate::span::Span;
use crate::token::{Keyword, StrPart, Token, TokenKind};

pub struct LexResult {
    pub tokens: Vec<Token>,
    pub diagnostics: Vec<Diagnostic>,
}

/// Tokeniza un archivo completo. El último token siempre es `Eof`.
pub fn lex(src: &str) -> LexResult {
    let mut lx = Lexer {
        src,
        pos: 0,
        diags: Vec::new(),
        saw_newline: true, // el primer token "viene tras un salto de línea".
    };
    let mut tokens = Vec::new();
    loop {
        let t = lx.next_token();
        let eof = t.kind == TokenKind::Eof;
        tokens.push(t);
        if eof {
            break;
        }
    }
    LexResult {
        tokens,
        diagnostics: lx.diags,
    }
}

struct Lexer<'a> {
    src: &'a str,
    pos: usize,
    diags: Vec<Diagnostic>,
    /// Se activa cuando se salta un salto de línea; el próximo token lo lee.
    saw_newline: bool,
}

impl<'a> Lexer<'a> {
    // ---------- utilidades ----------

    fn peek(&self) -> Option<char> {
        self.src[self.pos..].chars().next()
    }

    fn peek_at(&self, n: usize) -> Option<char> {
        self.src[self.pos..].chars().nth(n)
    }

    fn starts_with(&self, s: &str) -> bool {
        self.src[self.pos..].starts_with(s)
    }

    fn bump(&mut self) -> Option<char> {
        let c = self.peek()?;
        self.pos += c.len_utf8();
        Some(c)
    }

    fn eat(&mut self, s: &str) -> bool {
        if self.starts_with(s) {
            self.pos += s.len();
            true
        } else {
            false
        }
    }

    fn error(&mut self, d: Diagnostic) {
        self.diags.push(d);
    }

    fn tok(&self, kind: TokenKind, start: usize) -> Token {
        Token {
            kind,
            span: Span::new(start, self.pos),
            // `next_token` sobreescribe esto según `saw_newline`.
            newline_before: false,
        }
    }

    // ---------- espacios y comentarios ----------

    /// Salta espacios y comentarios. Devuelve un `DocComment` si encuentra uno.
    fn skip_trivia(&mut self) -> Option<Token> {
        loop {
            match self.peek() {
                Some(c) if c.is_whitespace() => {
                    if c == '\n' {
                        self.saw_newline = true;
                    }
                    self.bump();
                }
                Some('/') if self.starts_with("///") && !self.starts_with("////") => {
                    let start = self.pos;
                    self.pos += 3;
                    let text_start = self.pos;
                    while let Some(c) = self.peek() {
                        if c == '\n' {
                            break;
                        }
                        self.bump();
                    }
                    let text = self.src[text_start..self.pos].trim().to_string();
                    return Some(self.tok(TokenKind::DocComment(text), start));
                }
                Some('/') if self.starts_with("//") => {
                    while let Some(c) = self.peek() {
                        if c == '\n' {
                            break;
                        }
                        self.bump();
                    }
                }
                Some('/') if self.starts_with("/*") => {
                    let start = self.pos;
                    self.pos += 2;
                    let mut depth = 1;
                    while depth > 0 {
                        if self.eat("/*") {
                            depth += 1;
                        } else if self.eat("*/") {
                            depth -= 1;
                        } else if let Some(c) = self.bump() {
                            if c == '\n' {
                                self.saw_newline = true;
                            }
                        } else {
                            self.error(
                                Diagnostic::error(
                                    codes::UNTERMINATED_COMMENT,
                                    "comentario de bloque sin cerrar",
                                    Span::new(start, start + 2),
                                )
                                .with_help("agrega `*/` al final del comentario"),
                            );
                            break;
                        }
                    }
                }
                _ => return None,
            }
        }
    }

    // ---------- token principal ----------

    fn next_token(&mut self) -> Token {
        let doc = self.skip_trivia();
        // Captura si hubo salto de línea en la trivia previa a este token.
        let nl = self.saw_newline;
        self.saw_newline = false;
        let mut t = match doc {
            Some(d) => d,
            None => self.scan_token(),
        };
        t.newline_before = nl;
        t
    }

    fn scan_token(&mut self) -> Token {
        let start = self.pos;
        let Some(c) = self.peek() else {
            return self.tok(TokenKind::Eof, start);
        };

        if c.is_alphabetic() || c == '_' {
            return self.ident_or_keyword(start);
        }
        if c.is_ascii_digit() {
            return self.number(start);
        }
        if self.starts_with("\"\"\"") {
            return self.string(start, true);
        }
        if c == '"' {
            return self.string(start, false);
        }
        if c == '\'' {
            return self.single_quote_string(start);
        }
        self.operator(start)
    }

    fn ident_or_keyword(&mut self, start: usize) -> Token {
        while let Some(c) = self.peek() {
            if c.is_alphanumeric() || c == '_' {
                self.bump();
            } else {
                break;
            }
        }
        let text = &self.src[start..self.pos];
        let kind = match Keyword::lookup(text) {
            Some(k) => TokenKind::Keyword(k),
            None => TokenKind::Ident(text.to_string()),
        };
        self.tok(kind, start)
    }

    // ---------- números ----------

    fn number(&mut self, start: usize) -> Token {
        // Hexadecimal y binario
        let radix = if self.starts_with("0x") || self.starts_with("0X") {
            16
        } else if self.starts_with("0b") || self.starts_with("0B") {
            2
        } else {
            10
        };
        if radix != 10 {
            self.pos += 2;
            let digits_start = self.pos;
            while let Some(c) = self.peek() {
                if c.is_digit(radix) || c == '_' {
                    self.bump();
                } else {
                    break;
                }
            }
            let digits: String = self.src[digits_start..self.pos]
                .chars()
                .filter(|c| *c != '_')
                .collect();
            return match i64::from_str_radix(&digits, radix) {
                Ok(n) => self.tok(TokenKind::Int(n), start),
                Err(_) => self.bad_number(start),
            };
        }

        let mut is_float = false;
        self.digits();
        // Parte decimal: `1.5`, pero no `1..10` (rango) ni `1.metodo()`.
        if self.peek() == Some('.') && self.peek_at(1).is_some_and(|c| c.is_ascii_digit()) {
            is_float = true;
            self.bump();
            self.digits();
        }
        // Exponente: `1e10`, `2.5E-3`
        if matches!(self.peek(), Some('e' | 'E')) {
            let save = self.pos;
            self.bump();
            if matches!(self.peek(), Some('+' | '-')) {
                self.bump();
            }
            if self.peek().is_some_and(|c| c.is_ascii_digit()) {
                is_float = true;
                self.digits();
            } else {
                self.pos = save;
            }
        }
        // Un número pegado a letras (`12abc`) es un error claro.
        if self.peek().is_some_and(|c| c.is_alphabetic() || c == '_') {
            while self.peek().is_some_and(|c| c.is_alphanumeric() || c == '_') {
                self.bump();
            }
            return self.bad_number(start);
        }

        let text: String = self.src[start..self.pos]
            .chars()
            .filter(|c| *c != '_')
            .collect();
        if is_float {
            match text.parse::<f64>() {
                Ok(f) => self.tok(TokenKind::Float(f), start),
                Err(_) => self.bad_number(start),
            }
        } else {
            match text.parse::<i64>() {
                Ok(n) => self.tok(TokenKind::Int(n), start),
                Err(_) => {
                    self.error(
                        Diagnostic::error(
                            codes::INVALID_NUMBER,
                            "entero demasiado grande para Int (64 bits)",
                            Span::new(start, self.pos),
                        )
                        .with_help("usa un valor entre -9223372036854775808 y 9223372036854775807, o BigInt"),
                    );
                    self.tok(TokenKind::Int(0), start)
                }
            }
        }
    }

    fn digits(&mut self) {
        while self.peek().is_some_and(|c| c.is_ascii_digit() || c == '_') {
            self.bump();
        }
    }

    fn bad_number(&mut self, start: usize) -> Token {
        let text = self.src[start..self.pos].to_string();
        self.error(Diagnostic::error(
            codes::INVALID_NUMBER,
            format!("literal numérico inválido `{text}`"),
            Span::new(start, self.pos),
        ));
        self.tok(TokenKind::Int(0), start)
    }

    // ---------- strings ----------

    fn string(&mut self, start: usize, triple: bool) -> Token {
        self.pos += if triple { 3 } else { 1 };
        // En `"""`, un salto de línea justo después de la apertura se ignora.
        if triple && !self.eat("\r\n") {
            self.eat("\n");
        }
        let mut parts: Vec<StrPart> = Vec::new();
        let mut text = String::new();

        loop {
            if triple && self.starts_with("\"\"\"") {
                self.pos += 3;
                break;
            }
            let Some(c) = self.peek() else {
                self.unterminated_string(start, triple);
                break;
            };
            match c {
                '"' if !triple => {
                    self.bump();
                    break;
                }
                '\n' if !triple => {
                    self.unterminated_string(start, triple);
                    break;
                }
                '\\' => {
                    if let Some(ch) = self.escape() {
                        text.push(ch);
                    }
                }
                '{' => {
                    if !text.is_empty() {
                        parts.push(StrPart::Text(std::mem::take(&mut text)));
                    }
                    let open = self.pos;
                    self.bump();
                    let inner = self.interpolation(open);
                    parts.push(StrPart::Interp(inner));
                }
                _ => {
                    text.push(c);
                    self.bump();
                }
            }
        }
        if triple {
            // Quita el salto de línea + indentación antes del `"""` de cierre.
            if let Some(idx) = text.rfind('\n') {
                if text[idx + 1..].chars().all(|c| c == ' ' || c == '\t') {
                    text.truncate(idx);
                }
            }
        }
        if !text.is_empty() || parts.is_empty() {
            parts.push(StrPart::Text(text));
        }
        self.tok(TokenKind::Str(parts), start)
    }

    fn unterminated_string(&mut self, start: usize, triple: bool) {
        let (msg, help, fix) = if triple {
            (
                "string multilínea sin cerrar",
                "cierra el string con `\"\"\"`",
                "\"\"\"",
            )
        } else {
            (
                "string sin cerrar",
                "cierra el string con `\"` en la misma línea; para varias líneas usa `\"\"\"...\"\"\"`",
                "\"",
            )
        };
        let mut d = Diagnostic::error(codes::UNTERMINATED_STRING, msg, Span::new(start, self.pos))
            .with_help(help);
        // Arreglo: insertar el cierre donde terminó el string.
        d.fix = Some(crate::diagnostic::Fix {
            span: Span::new(self.pos, self.pos),
            replacement: fix.to_string(),
        });
        self.error(d);
    }

    /// Lee una secuencia de escape (el `\` actual incluido).
    fn escape(&mut self) -> Option<char> {
        let start = self.pos;
        self.bump(); // '\'
        let c = self.bump()?;
        let out = match c {
            'n' => '\n',
            't' => '\t',
            'r' => '\r',
            '0' => '\0',
            '\\' => '\\',
            '"' => '"',
            '{' => '{',
            '}' => '}',
            'u' if self.peek() == Some('{') => {
                self.bump();
                let hex_start = self.pos;
                while self.peek().is_some_and(|c| c.is_ascii_hexdigit()) {
                    self.bump();
                }
                let hex = &self.src[hex_start..self.pos];
                let ok = self.eat("}");
                match u32::from_str_radix(hex, 16).ok().and_then(char::from_u32) {
                    Some(ch) if ok => ch,
                    _ => {
                        self.error(
                            Diagnostic::error(
                                codes::INVALID_ESCAPE,
                                "escape Unicode inválido",
                                Span::new(start, self.pos),
                            )
                            .with_help("formato: \\u{1F600}"),
                        );
                        return None;
                    }
                }
            }
            other => {
                self.error(
                    Diagnostic::error(
                        codes::INVALID_ESCAPE,
                        format!("escape desconocido `\\{other}`"),
                        Span::new(start, self.pos),
                    )
                    .with_help("escapes válidos: \\n \\t \\r \\0 \\\\ \\\" \\{ \\} \\u{XXXX}"),
                );
                return None;
            }
        };
        Some(out)
    }

    /// Tokeniza `{expr}` dentro de un string hasta la `}` que la cierra.
    fn interpolation(&mut self, open: usize) -> Vec<Token> {
        let mut tokens = Vec::new();
        let mut depth = 0usize;
        loop {
            if self.peek() == Some('\n') && depth == 0 {
                // Una interpolación no puede cruzar líneas.
                self.unterminated_interp(open);
                return tokens;
            }
            let t = self.next_token();
            match t.kind {
                TokenKind::Eof => {
                    self.unterminated_interp(open);
                    return tokens;
                }
                TokenKind::LBrace => depth += 1,
                TokenKind::RBrace if depth == 0 => return tokens,
                TokenKind::RBrace => depth -= 1,
                _ => {}
            }
            tokens.push(t);
        }
    }

    fn unterminated_interp(&mut self, open: usize) {
        self.error(
            Diagnostic::error(
                codes::UNTERMINATED_INTERPOLATION,
                "interpolación sin cerrar",
                Span::new(open, open + 1),
            )
            .with_help("cierra la interpolación con `}`, o escribe `\\{` para un `{` literal"),
        );
    }

    fn single_quote_string(&mut self, start: usize) -> Token {
        self.bump();
        let mut content = String::new();
        while let Some(c) = self.peek() {
            if c == '\'' || c == '\n' {
                break;
            }
            content.push(c);
            self.bump();
        }
        self.eat("'");
        let escaped = content.replace('"', "\\\"");
        self.error(
            Diagnostic::error(
                codes::SINGLE_QUOTE_STRING,
                "los strings usan comillas dobles",
                Span::new(start, self.pos),
            )
            .with_help("OpenS solo tiene un tipo de string: \"...\"")
            .with_fix(format!("\"{escaped}\"")),
        );
        self.tok(TokenKind::Str(vec![StrPart::Text(content)]), start)
    }

    // ---------- operadores ----------

    /// Rodea `word` de espacios solo donde hagan falta, para que el arreglo
    /// automático produzca código ya bien formateado (`a&&b` → `a and b`).
    fn spaced(&self, start: usize, word: &str, pad: bool) -> String {
        if !pad {
            return word.to_string();
        }
        let before = self.src[..start]
            .chars()
            .next_back()
            .is_some_and(|c| !c.is_whitespace());
        let after = self.peek().is_some_and(|c| !c.is_whitespace());
        format!(
            "{}{}{}",
            if before { " " } else { "" },
            word,
            if after { " " } else { "" }
        )
    }

    fn operator(&mut self, start: usize) -> Token {
        use TokenKind::*;

        // Operadores de otros lenguajes: error útil + token equivalente para seguir.
        let foreign: &[(&str, &str, TokenKind)] = &[
            ("===", "==", EqEq),
            ("!==", "!=", NotEq),
            ("&&", "and", Keyword(crate::token::Keyword::And)),
            ("||", "or", Keyword(crate::token::Keyword::Or)),
            ("++", "+= 1", PlusEq),
            ("--", "-= 1", MinusEq),
        ];
        for (op, replacement, kind) in foreign {
            if self.eat(op) {
                let replacement = self.spaced(start, replacement, *op != "!==" && *op != "===");
                self.error(
                    Diagnostic::error(
                        codes::FOREIGN_OPERATOR,
                        format!("`{op}` no existe en OpenS"),
                        Span::new(start, self.pos),
                    )
                    .with_help(format!("usa `{}`", replacement.trim()))
                    .with_fix(replacement),
                );
                return self.tok(kind.clone(), start);
            }
        }

        let table: &[(&str, TokenKind)] = &[
            ("->", Arrow),
            ("=>", FatArrow),
            ("==", EqEq),
            ("!=", NotEq),
            ("<=", Le),
            (">=", Ge),
            ("+=", PlusEq),
            ("-=", MinusEq),
            ("*=", StarEq),
            ("/=", SlashEq),
            ("|>", Pipe),
            ("&", Amp),
            ("..", DotDot),
            ("(", LParen),
            (")", RParen),
            ("{", LBrace),
            ("}", RBrace),
            ("[", LBracket),
            ("]", RBracket),
            (",", Comma),
            (".", Dot),
            (":", Colon),
            (";", Semi),
            ("?", Question),
            ("@", At),
            ("=", Eq),
            ("<", Lt),
            (">", Gt),
            ("+", Plus),
            ("-", Minus),
            ("*", Star),
            ("/", Slash),
            ("%", Percent),
        ];
        for (op, kind) in table {
            if self.eat(op) {
                return self.tok(kind.clone(), start);
            }
        }

        // `!` solo (negación al estilo C/JS)
        if self.eat("!") {
            self.error(
                Diagnostic::error(
                    codes::FOREIGN_OPERATOR,
                    "`!` no existe en OpenS",
                    Span::new(start, self.pos),
                )
                .with_help("usa `not`")
                .with_fix(if self.peek().is_some_and(char::is_whitespace) {
                    "not"
                } else {
                    "not "
                }),
            );
            return self.tok(Keyword(crate::token::Keyword::Not), start);
        }

        let c = self.bump().unwrap_or('\0');
        self.error(Diagnostic::error(
            codes::UNEXPECTED_CHAR,
            format!("carácter inesperado `{c}`"),
            Span::new(start, self.pos),
        ));
        // Recuperación: saltar el carácter y continuar.
        self.next_token()
    }
}
