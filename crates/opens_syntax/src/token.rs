//! Tokens de OpenS.

use crate::span::Span;

#[derive(Debug, Clone, PartialEq)]
pub enum TokenKind {
    // Literales e identificadores
    Ident(String),
    Int(i64),
    Float(f64),
    /// String con interpolaciones. `"hola {nombre}"` →
    /// `[Text("hola "), Interp([Ident("nombre")])]`.
    Str(Vec<StrPart>),
    Keyword(Keyword),

    // Delimitadores
    LParen,
    RParen,
    LBrace,
    RBrace,
    LBracket,
    RBracket,

    // Puntuación
    Comma,
    Dot,
    DotDot,
    Colon,
    Semi,
    Arrow,    // ->
    FatArrow, // =>
    Question, // ?
    At,       // @

    // Operadores
    Eq,    // =
    EqEq,  // ==
    NotEq, // !=
    Lt,
    Le,
    Gt,
    Ge,
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    PlusEq,
    MinusEq,
    StarEq,
    SlashEq,
    Pipe, // |> (encadenar llamadas)
    Amp,  // & (referencia / tomar dirección)

    /// Comentario de documentación `/// ...` (se conserva para el LSP).
    DocComment(String),
    Eof,
}

#[derive(Debug, Clone, PartialEq)]
pub enum StrPart {
    Text(String),
    Interp(Vec<Token>),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Token {
    pub kind: TokenKind,
    pub span: Span,
    /// `true` si hay al menos un salto de línea entre el token anterior y
    /// este. OpenS separa sentencias por línea (no usa `;`), así que el parser
    /// usa esta marca como terminador implícito: un encadenamiento postfijo
    /// (`.`, `(`, `[`, `?`) no cruza un salto de línea.
    pub newline_before: bool,
}

macro_rules! keywords {
    ($($variant:ident => $text:literal),* $(,)?) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        pub enum Keyword { $($variant),* }

        impl Keyword {
            pub fn lookup(s: &str) -> Option<Keyword> {
                match s { $($text => Some(Keyword::$variant),)* _ => None }
            }
            pub fn as_str(self) -> &'static str {
                match self { $(Keyword::$variant => $text),* }
            }
            pub const ALL: &'static [Keyword] = &[$(Keyword::$variant),*];
        }
    };
}

keywords! {
    // Declaraciones
    Let => "let", Var => "var", Fn => "fn", Type => "type", Enum => "enum",
    Import => "import", Export => "export", Pub => "pub",
    // Orientación a objetos (multiparadigma)
    Impl => "impl", Trait => "trait", SelfKw => "self",
    // Memoria (referencias, punteros, heap)
    Box => "box",
    // Construcciones de IA / MCP
    Tool => "tool", Server => "server", Client => "client", Expose => "expose",
    Prompt => "prompt", Agent => "agent", Uses => "uses",
    // Control de flujo
    If => "if", Else => "else", Match => "match", For => "for", In => "in",
    While => "while", Break => "break", Continue => "continue", Return => "return",
    // Concurrencia
    Async => "async", Await => "await", Stream => "stream", Yield => "yield",
    // Lógica (OpenS usa palabras, no símbolos)
    And => "and", Or => "or", Not => "not",
    // Literales
    True => "true", False => "false", None => "none",
    // Calidad
    Test => "test", Assert => "assert",
    // Bajo nivel / rendimiento
    Unsafe => "unsafe", Extern => "extern",
}

impl TokenKind {
    /// Descripción corta para mensajes de error.
    pub fn describe(&self) -> String {
        match self {
            TokenKind::Ident(s) => format!("identificador `{s}`"),
            TokenKind::Int(n) => format!("entero `{n}`"),
            TokenKind::Float(n) => format!("decimal `{n}`"),
            TokenKind::Str(_) => "string".into(),
            TokenKind::Keyword(k) => format!("palabra clave `{}`", k.as_str()),
            TokenKind::DocComment(_) => "comentario de documentación".into(),
            TokenKind::Eof => "fin del archivo".into(),
            other => format!("`{}`", other.symbol().unwrap_or("?")),
        }
    }

    pub fn symbol(&self) -> Option<&'static str> {
        use TokenKind::*;
        Some(match self {
            LParen => "(",
            RParen => ")",
            LBrace => "{",
            RBrace => "}",
            LBracket => "[",
            RBracket => "]",
            Comma => ",",
            Dot => ".",
            DotDot => "..",
            Colon => ":",
            Semi => ";",
            Arrow => "->",
            FatArrow => "=>",
            Question => "?",
            At => "@",
            Eq => "=",
            EqEq => "==",
            NotEq => "!=",
            Lt => "<",
            Le => "<=",
            Gt => ">",
            Ge => ">=",
            Plus => "+",
            Minus => "-",
            Star => "*",
            Slash => "/",
            Percent => "%",
            PlusEq => "+=",
            MinusEq => "-=",
            StarEq => "*=",
            SlashEq => "/=",
            Pipe => "|>",
            Amp => "&",
            _ => return None,
        })
    }
}
