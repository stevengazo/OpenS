//! Diagnósticos (errores y advertencias).
//!
//! Cada diagnóstico tiene un código estable (`E0101`), una ubicación exacta
//! y, cuando es posible, un arreglo automático (`Fix`). Se pueden mostrar en
//! formato legible para personas o en JSON para que un agente de IA los lea,
//! aplique el arreglo y vuelva a compilar sin intervención humana.

use crate::span::{LineIndex, Span};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
}

impl Severity {
    pub fn as_str(self) -> &'static str {
        match self {
            Severity::Error => "error",
            Severity::Warning => "warning",
        }
    }
}

/// Un arreglo mecánico: reemplazar el texto de `span` por `replacement`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fix {
    pub span: Span,
    pub replacement: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    pub code: &'static str,
    pub severity: Severity,
    pub message: String,
    pub span: Span,
    pub help: Option<String>,
    pub fix: Option<Fix>,
}

impl Diagnostic {
    pub fn error(code: &'static str, message: impl Into<String>, span: Span) -> Self {
        Diagnostic {
            code,
            severity: Severity::Error,
            message: message.into(),
            span,
            help: None,
            fix: None,
        }
    }

    pub fn with_help(mut self, help: impl Into<String>) -> Self {
        self.help = Some(help.into());
        self
    }

    pub fn with_fix(mut self, replacement: impl Into<String>) -> Self {
        self.fix = Some(Fix {
            span: self.span,
            replacement: replacement.into(),
        });
        self
    }
}

/// Catálogo de códigos de error del lexer (E01xx).
pub mod codes {
    pub const UNEXPECTED_CHAR: &str = "E0101";
    pub const UNTERMINATED_STRING: &str = "E0102";
    pub const UNTERMINATED_COMMENT: &str = "E0103";
    pub const INVALID_ESCAPE: &str = "E0104";
    pub const INVALID_NUMBER: &str = "E0105";
    pub const FOREIGN_OPERATOR: &str = "E0106";
    pub const UNTERMINATED_INTERPOLATION: &str = "E0107";
    pub const SINGLE_QUOTE_STRING: &str = "E0108";

    /// Explicación larga de cada código (`opens explain E0106`).
    pub fn explain(code: &str) -> Option<&'static str> {
        Some(match code {
            UNEXPECTED_CHAR => "Se encontró un carácter que no forma parte de ningún token de OpenS.",
            UNTERMINATED_STRING => "Un string empezó con comillas pero el archivo o la línea terminó antes de cerrarlo.\nLos strings de una línea usan \"...\"; los de varias líneas usan \"\"\"...\"\"\".",
            UNTERMINATED_COMMENT => "Un comentario de bloque /* ... */ no se cerró.",
            INVALID_ESCAPE => "Secuencia de escape desconocida. Válidas: \\n \\t \\r \\\\ \\\" \\{ \\} \\0 \\u{XXXX}.",
            INVALID_NUMBER => "Literal numérico mal formado.",
            FOREIGN_OPERATOR => "OpenS usa palabras clave para la lógica booleana: `and`, `or`, `not`.\nTampoco existen `===`/`!==`: `==` y `!=` ya comparan sin conversiones implícitas.\n`++` y `--` no existen; usa `x += 1`.",
            UNTERMINATED_INTERPOLATION => "Una interpolación `{expr}` dentro de un string no se cerró con `}`.",
            SINGLE_QUOTE_STRING => "OpenS solo tiene un tipo de string: comillas dobles. Las comillas simples no se usan.",
            _ => return None,
        })
    }
}

fn json_escape(s: &str, out: &mut String) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
}

/// Muestra diagnósticos en formato legible, estilo Rust/Elm.
pub fn render_human(file: &str, src: &str, diags: &[Diagnostic]) -> String {
    let index = LineIndex::new(src);
    let mut out = String::new();
    for d in diags {
        let lc = index.line_col(src, d.span.start);
        out.push_str(&format!(
            "{}[{}]: {}\n",
            d.severity.as_str(),
            d.code,
            d.message
        ));
        out.push_str(&format!("  --> {}:{}:{}\n", file, lc.line, lc.col));
        let line = index.line_text(src, lc.line);
        let gutter = lc.line.to_string().len();
        out.push_str(&format!("{} |\n", " ".repeat(gutter)));
        out.push_str(&format!("{} | {}\n", lc.line, line));
        let width = src
            .get(d.span.start..d.span.end)
            .map(|s| s.lines().next().unwrap_or("").chars().count())
            .unwrap_or(1)
            .max(1);
        out.push_str(&format!(
            "{} | {}{}\n",
            " ".repeat(gutter),
            " ".repeat(lc.col - 1),
            "^".repeat(width)
        ));
        if let Some(h) = &d.help {
            out.push_str(&format!("{} = ayuda: {}\n", " ".repeat(gutter), h));
        }
        if let Some(f) = &d.fix {
            out.push_str(&format!(
                "{} = arreglo: reemplazar por `{}`\n",
                " ".repeat(gutter),
                f.replacement
            ));
        }
        out.push('\n');
    }
    out
}

/// Muestra diagnósticos como JSON estable (esquema `opens.diagnostics/v1`).
pub fn render_json(file: &str, src: &str, diags: &[Diagnostic]) -> String {
    let index = LineIndex::new(src);
    let mut out = String::from("{\"schema\":\"opens.diagnostics/v1\",\"file\":");
    json_escape(file, &mut out);
    out.push_str(&format!(
        ",\"ok\":{},\"diagnostics\":[",
        !diags.iter().any(|d| d.severity == Severity::Error)
    ));
    for (i, d) in diags.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        let a = index.line_col(src, d.span.start);
        let b = index.line_col(src, d.span.end);
        out.push_str("{\"code\":");
        json_escape(d.code, &mut out);
        out.push_str(",\"severity\":");
        json_escape(d.severity.as_str(), &mut out);
        out.push_str(",\"message\":");
        json_escape(&d.message, &mut out);
        out.push_str(&format!(
            ",\"span\":{{\"start\":{},\"end\":{},\"line\":{},\"col\":{},\"end_line\":{},\"end_col\":{}}}",
            d.span.start, d.span.end, a.line, a.col, b.line, b.col
        ));
        out.push_str(",\"help\":");
        match &d.help {
            Some(h) => json_escape(h, &mut out),
            None => out.push_str("null"),
        }
        out.push_str(",\"fix\":");
        match &d.fix {
            Some(f) => {
                out.push_str(&format!(
                    "{{\"start\":{},\"end\":{},\"replacement\":",
                    f.span.start, f.span.end
                ));
                json_escape(&f.replacement, &mut out);
                out.push('}');
            }
            None => out.push_str("null"),
        }
        out.push('}');
    }
    out.push_str("]}");
    out
}

/// Aplica todos los arreglos automáticos (sin solapamientos) a `src`.
pub fn apply_fixes(src: &str, diags: &[Diagnostic]) -> String {
    let mut fixes: Vec<&Fix> = diags.iter().filter_map(|d| d.fix.as_ref()).collect();
    fixes.sort_by_key(|f| f.span.start);
    let mut out = String::with_capacity(src.len());
    let mut pos = 0;
    for f in fixes {
        if f.span.start < pos {
            continue; // se solapa con un arreglo anterior
        }
        out.push_str(&src[pos..f.span.start]);
        out.push_str(&f.replacement);
        pos = f.span.end;
    }
    out.push_str(&src[pos..]);
    out
}
