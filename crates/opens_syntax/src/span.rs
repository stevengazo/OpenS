//! Posiciones dentro del código fuente.

/// Rango de bytes `[start, end)` dentro del archivo fuente.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

impl Span {
    pub fn new(start: usize, end: usize) -> Self {
        Span { start, end }
    }

    pub fn len(&self) -> usize {
        self.end - self.start
    }

    pub fn is_empty(&self) -> bool {
        self.start == self.end
    }

    /// Une dos spans en uno que cubre ambos.
    pub fn to(self, other: Span) -> Span {
        Span::new(self.start.min(other.start), self.end.max(other.end))
    }
}

/// Línea y columna (ambas empiezan en 1, columna contada en caracteres).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LineCol {
    pub line: usize,
    pub col: usize,
}

/// Índice de inicios de línea para convertir offsets en línea/columna.
#[derive(Debug, Clone)]
pub struct LineIndex {
    line_starts: Vec<usize>,
}

impl LineIndex {
    pub fn new(src: &str) -> Self {
        let mut line_starts = vec![0];
        for (i, b) in src.bytes().enumerate() {
            if b == b'\n' {
                line_starts.push(i + 1);
            }
        }
        LineIndex { line_starts }
    }

    pub fn line_col(&self, src: &str, offset: usize) -> LineCol {
        let line = match self.line_starts.binary_search(&offset) {
            Ok(i) => i,
            Err(i) => i - 1,
        };
        let start = self.line_starts[line];
        let end = offset.min(src.len());
        let col = src.get(start..end).map(|s| s.chars().count()).unwrap_or(0) + 1;
        LineCol {
            line: line + 1,
            col,
        }
    }

    /// Texto de la línea `line` (base 1), sin el salto de línea.
    pub fn line_text<'a>(&self, src: &'a str, line: usize) -> &'a str {
        let start = self.line_starts[line - 1];
        let end = self.line_starts.get(line).copied().unwrap_or(src.len());
        src[start..end].trim_end_matches(['\n', '\r'])
    }
}
