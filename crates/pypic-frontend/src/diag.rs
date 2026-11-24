//! Spans y diagnosticos con ubicacion en el fuente.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Span {
    pub start: u32,
    pub end: u32,
}

impl Span {
    pub fn new(start: usize, end: usize) -> Self {
        Span { start: start as u32, end: end as u32 }
    }
    pub fn to(self, other: Span) -> Span {
        Span { start: self.start, end: other.end.max(self.end) }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
}

#[derive(Clone, Debug)]
pub struct Diagnostic {
    pub severity: Severity,
    pub code: &'static str,
    pub message: String,
    pub span: Span,
}

#[derive(Default, Debug)]
pub struct Diagnostics {
    pub items: Vec<Diagnostic>,
}

impl Diagnostics {
    pub fn error(&mut self, code: &'static str, span: Span, message: impl Into<String>) {
        self.items.push(Diagnostic { severity: Severity::Error, code, message: message.into(), span });
    }
    pub fn warning(&mut self, code: &'static str, span: Span, message: impl Into<String>) {
        self.items.push(Diagnostic { severity: Severity::Warning, code, message: message.into(), span });
    }
    pub fn has_errors(&self) -> bool {
        self.items.iter().any(|d| d.severity == Severity::Error)
    }
    pub fn codes(&self) -> Vec<&'static str> {
        self.items.iter().map(|d| d.code).collect()
    }
    /// `archivo:linea:col: error[CODIGO]: mensaje`, ordenado por posicion.
    pub fn render(&self, source: &str, file: &str) -> String {
        let mut items: Vec<&Diagnostic> = self.items.iter().collect();
        items.sort_by_key(|d| d.span.start);
        let mut out = String::new();
        for d in items {
            let (line, col) = line_col(source, d.span.start as usize);
            let sev = match d.severity {
                Severity::Error => "error",
                Severity::Warning => "warning",
            };
            out.push_str(&format!("{file}:{line}:{col}: {sev}[{}]: {}\n", d.code, d.message));
        }
        out
    }
}

/// Linea y columna (1-based, en caracteres) de un offset en bytes.
pub fn line_col(source: &str, offset: usize) -> (usize, usize) {
    let offset = offset.min(source.len());
    let before = &source.as_bytes()[..offset];
    let line = before.iter().filter(|&&b| b == b'\n').count() + 1;
    let line_start = before.iter().rposition(|&b| b == b'\n').map_or(0, |i| i + 1);
    let col = String::from_utf8_lossy(&before[line_start..]).chars().count() + 1;
    (line, col)
}
