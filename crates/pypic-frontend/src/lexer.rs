//! Lexer con sintaxis Python: indentacion significativa (INDENT/DEDENT),
//! union implicita de lineas dentro de parentesis y comentarios con '#'.

use crate::diag::{Diagnostics, Span};

#[derive(Clone, Debug, PartialEq)]
pub enum Tok {
    Ident(String),
    Int(i128),
    /// Contenido literal entre comillas (sin interpretar escapes).
    Str(String, u8),
    /// `f"..."`: el contenido sin decodificar.
    FStr(String),
    Newline,
    Indent,
    Dedent,
    Eof,
    LParen,
    RParen,
    LBracket,
    RBracket,
    Comma,
    Colon,
    Arrow,
    At,
    Assign,
    Plus,
    Minus,
    Star,
    Slash,
    SlashSlash,
    Percent,
    Amp,
    Pipe,
    Caret,
    Tilde,
    Shl,
    Shr,
    StarStar,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    /// Asignacion aumentada; guarda el operador sin el '=' (p. ej. "+", "<<").
    AugAssign(&'static str),
    /// Cualquier otro caracter (no soportado por el subconjunto).
    Other(char),
}

#[derive(Clone, Debug)]
pub struct Token {
    pub tok: Tok,
    pub span: Span,
}

pub fn lex(src: &str, diags: &mut Diagnostics) -> Vec<Token> {
    Lexer { b: src.as_bytes(), src, i: 0, out: Vec::new(), indents: vec![0], depth: 0, d: diags }.run()
}

struct Lexer<'a> {
    b: &'a [u8],
    src: &'a str,
    i: usize,
    out: Vec<Token>,
    indents: Vec<usize>,
    depth: i32,
    d: &'a mut Diagnostics,
}

impl<'a> Lexer<'a> {
    fn push(&mut self, tok: Tok, start: usize) {
        self.out.push(Token { tok, span: Span::new(start, self.i) });
    }

    fn peek(&self, off: usize) -> u8 {
        *self.b.get(self.i + off).unwrap_or(&0)
    }

    fn run(mut self) -> Vec<Token> {
        let mut at_line_start = true;
        while self.i < self.b.len() {
            if at_line_start && self.depth == 0 {
                if !self.handle_indent() {
                    continue; // linea en blanco/comentario: ya consumida
                }
                at_line_start = false;
                if self.i >= self.b.len() {
                    break;
                }
            }
            let c = self.peek(0);
            let start = self.i;
            match c {
                b'\n' => {
                    self.i += 1;
                    if self.depth == 0 {
                        self.push(Tok::Newline, start);
                        at_line_start = true;
                    }
                }
                b'\r' | b' ' => self.i += 1,
                b'\t' => {
                    self.i += 1;
                    self.d.error("PYP0003", Span::new(start, self.i), "tabuladores no permitidos; usa espacios");
                }
                b'#' => {
                    while self.i < self.b.len() && self.peek(0) != b'\n' {
                        self.i += 1;
                    }
                }
                b'\\' => {
                    self.i += 1;
                    self.d.error("PYP0004", Span::new(start, self.i), "la continuacion de linea con '\\' no esta soportada; usa parentesis");
                }
                b'"' | b'\'' => self.string(c),
                b'0'..=b'9' => self.number(),
                c if c.is_ascii_alphabetic() || c == b'_' => {
                    while self.peek(0).is_ascii_alphanumeric() || self.peek(0) == b'_' {
                        self.i += 1;
                    }
                    let s = self.src[start..self.i].to_string();
                    if s == "f" && matches!(self.peek(0), b'"' | b'\'') {
                        let q = self.peek(0);
                        self.string(q);
                        if let Some(Token { tok: Tok::Str(inner, quote), .. }) = self.out.pop() {
                            if quote == 3 {
                                self.d.error("PYP0007", Span::new(start, self.i), "las f-strings de tres comillas no estan soportadas");
                            }
                            self.push(Tok::FStr(inner), start);
                        }
                    } else {
                        self.push(Tok::Ident(s), start);
                    }
                }
                _ => self.punct(),
            }
        }
        // cierre: Newline final, Dedents, Eof
        let end = self.b.len();
        if !matches!(self.out.last().map(|t| &t.tok), Some(Tok::Newline) | None) {
            self.out.push(Token { tok: Tok::Newline, span: Span::new(end, end) });
        }
        while self.indents.len() > 1 {
            self.indents.pop();
            self.out.push(Token { tok: Tok::Dedent, span: Span::new(end, end) });
        }
        self.out.push(Token { tok: Tok::Eof, span: Span::new(end, end) });
        self.out
    }

    /// Procesa la indentacion al inicio de linea. Devuelve false si la linea
    /// estaba vacia (se consumio y hay que repetir).
    fn handle_indent(&mut self) -> bool {
        let line_start = self.i;
        let mut width = 0;
        while self.peek(0) == b' ' {
            width += 1;
            self.i += 1;
        }
        match self.peek(0) {
            b'\n' => {
                self.i += 1;
                return false;
            }
            b'\r' => {
                self.i += 1;
                return false;
            }
            b'#' => {
                while self.i < self.b.len() && self.peek(0) != b'\n' {
                    self.i += 1;
                }
                return false;
            }
            0 if self.i >= self.b.len() => return false,
            _ => {}
        }
        let cur = *self.indents.last().unwrap();
        if width > cur {
            self.indents.push(width);
            self.out.push(Token { tok: Tok::Indent, span: Span::new(line_start, self.i) });
        } else if width < cur {
            while *self.indents.last().unwrap() > width {
                self.indents.pop();
                self.out.push(Token { tok: Tok::Dedent, span: Span::new(line_start, self.i) });
            }
            if *self.indents.last().unwrap() != width {
                self.d.error("PYP0005", Span::new(line_start, self.i), "indentacion inconsistente");
                self.indents.push(width);
            }
        }
        true
    }

    fn string(&mut self, quote: u8) {
        let start = self.i;
        let triple = self.peek(1) == quote && self.peek(2) == quote;
        self.i += if triple { 3 } else { 1 };
        loop {
            if self.i >= self.b.len() {
                self.d.error("PYP0006", Span::new(start, self.i), "cadena sin cerrar");
                break;
            }
            let c = self.peek(0);
            if c == b'\\' {
                self.i += 2;
                continue;
            }
            if triple {
                if c == quote && self.peek(1) == quote && self.peek(2) == quote {
                    self.i += 3;
                    break;
                }
            } else if c == quote {
                self.i += 1;
                break;
            } else if c == b'\n' {
                self.d.error("PYP0006", Span::new(start, self.i), "cadena sin cerrar");
                break;
            }
            self.i += 1;
        }
        self.i = self.i.min(self.b.len());
        let raw = &self.src[start..self.i];
        let q = if triple { 3 } else { 1 };
        let inner = if raw.len() >= 2 * q { raw[q..raw.len() - q].to_string() } else { String::new() };
        self.push(Tok::Str(inner, q as u8), start);
    }

    fn number(&mut self) {
        let start = self.i;
        let (radix, skip) = match (self.peek(0), self.peek(1)) {
            (b'0', b'x' | b'X') => (16, 2),
            (b'0', b'b' | b'B') => (2, 2),
            (b'0', b'o' | b'O') => (8, 2),
            _ => (10, 0),
        };
        self.i += skip;
        let digits_start = self.i;
        while self.peek(0).is_ascii_alphanumeric() || self.peek(0) == b'_' {
            self.i += 1;
        }
        let text: String = self.src[digits_start..self.i].chars().filter(|&c| c != '_').collect();
        // flotantes y notacion cientifica no existen en el subconjunto
        if radix == 10 && (self.peek(0) == b'.' || text.contains(['e', 'E', 'j', 'J'])) {
            while self.peek(0).is_ascii_alphanumeric() || self.peek(0) == b'.' {
                self.i += 1;
            }
            self.d.error("PYP0007", Span::new(start, self.i), "los literales flotantes no estan soportados");
            self.push(Tok::Int(0), start);
            return;
        }
        match i128::from_str_radix(&text, radix) {
            Ok(v) if v <= u64::MAX as i128 => self.push(Tok::Int(v), start),
            _ => {
                self.d.error("PYP0008", Span::new(start, self.i), "literal entero invalido o demasiado grande");
                self.push(Tok::Int(0), start);
            }
        }
    }

    fn punct(&mut self) {
        let start = self.i;
        let c = self.peek(0);
        let n = self.peek(1);
        let n2 = self.peek(2);
        // (texto, token) por orden de longitud
        let (len, tok) = match (c, n, n2) {
            (b'*', b'*', _) => (2, Tok::StarStar),
            (b'/', b'/', b'=') => (3, Tok::AugAssign("//")),
            (b'<', b'<', b'=') => (3, Tok::AugAssign("<<")),
            (b'>', b'>', b'=') => (3, Tok::AugAssign(">>")),
            (b'/', b'/', _) => (2, Tok::SlashSlash),
            (b'<', b'<', _) => (2, Tok::Shl),
            (b'>', b'>', _) => (2, Tok::Shr),
            (b'=', b'=', _) => (2, Tok::Eq),
            (b'!', b'=', _) => (2, Tok::Ne),
            (b'<', b'=', _) => (2, Tok::Le),
            (b'>', b'=', _) => (2, Tok::Ge),
            (b'-', b'>', _) => (2, Tok::Arrow),
            (b'+', b'=', _) => (2, Tok::AugAssign("+")),
            (b'-', b'=', _) => (2, Tok::AugAssign("-")),
            (b'*', b'=', _) => (2, Tok::AugAssign("*")),
            (b'%', b'=', _) => (2, Tok::AugAssign("%")),
            (b'&', b'=', _) => (2, Tok::AugAssign("&")),
            (b'|', b'=', _) => (2, Tok::AugAssign("|")),
            (b'^', b'=', _) => (2, Tok::AugAssign("^")),
            (b'(', _, _) => (1, Tok::LParen),
            (b')', _, _) => (1, Tok::RParen),
            (b'[', _, _) => (1, Tok::LBracket),
            (b']', _, _) => (1, Tok::RBracket),
            (b',', _, _) => (1, Tok::Comma),
            (b':', _, _) => (1, Tok::Colon),
            (b'@', _, _) => (1, Tok::At),
            (b'=', _, _) => (1, Tok::Assign),
            (b'+', _, _) => (1, Tok::Plus),
            (b'-', _, _) => (1, Tok::Minus),
            (b'*', _, _) => (1, Tok::Star),
            (b'/', _, _) => (1, Tok::Slash),
            (b'%', _, _) => (1, Tok::Percent),
            (b'&', _, _) => (1, Tok::Amp),
            (b'|', _, _) => (1, Tok::Pipe),
            (b'^', _, _) => (1, Tok::Caret),
            (b'~', _, _) => (1, Tok::Tilde),
            (b'<', _, _) => (1, Tok::Lt),
            (b'>', _, _) => (1, Tok::Gt),
            _ => {
                let ch = self.src[self.i..].chars().next().unwrap();
                (ch.len_utf8(), Tok::Other(ch))
            }
        };
        self.i += len;
        match tok {
            Tok::LParen | Tok::LBracket => self.depth += 1,
            Tok::RParen | Tok::RBracket => self.depth = (self.depth - 1).max(0),
            Tok::Other('{') => self.depth += 1,
            Tok::Other('}') => self.depth = (self.depth - 1).max(0),
            _ => {}
        }
        self.push(tok, start);
    }
}
