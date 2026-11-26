//! Lexer de PyPIC BASIC (estilo PICAXE): una sentencia por linea, sin distinguir mayusculas de minusculas, comentarios con `'` o `;`.

use crate::diag::{Diagnostics, Span};

#[derive(Clone, Debug, PartialEq)]
pub enum T {
    Ident(String),
    Int(i128),
    Str(String),
    Newline,
    Eof,
    Colon,
    Comma,
    Dot,
    LParen,
    RParen,
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    Amp,
    Pipe,
    Caret,
    Tilde,
    /// `#b0` en SERTXD: el valor en decimal.
    Hash,
    Shl,
    Shr,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    /// `+=`, `-=`, ...: el operador sin el `=`.
    AugAssign(char),
}

#[derive(Clone, Debug)]
pub struct Token {
    pub t: T,
    pub span: Span,
}

pub fn lex(src: &str, d: &mut Diagnostics) -> Vec<Token> {
    let b = src.as_bytes();
    let mut out: Vec<Token> = Vec::new();
    let mut i = 0;
    let push = |out: &mut Vec<Token>, t: T, s: usize, e: usize| out.push(Token { t, span: Span::new(s, e) });
    while i < b.len() {
        let c = b[i];
        match c {
            b' ' | b'\t' | b'\r' => i += 1,
            // `_` al final de la linea une la linea siguiente
            b'_' if b[i + 1..].iter().take_while(|x| **x != b'\n').all(|x| matches!(x, b' ' | b'\t' | b'\r'))
                && i < b.len()
                && b[i + 1..].contains(&b'\n')
                && out.last().is_some_and(|t| t.t != T::Newline) =>
            {
                while i < b.len() && b[i] != b'\n' {
                    i += 1;
                }
                i += 1;
            }
            b'\n' => {
                push(&mut out, T::Newline, i, i + 1);
                i += 1;
            }
            b'\'' | b';' => {
                while i < b.len() && b[i] != b'\n' {
                    i += 1;
                }
            }
            b'"' => {
                let s = i;
                i += 1;
                let mut text = String::new();
                while i < b.len() && b[i] != b'"' && b[i] != b'\n' {
                    text.push(b[i] as char);
                    i += 1;
                }
                if i < b.len() && b[i] == b'"' {
                    i += 1;
                } else {
                    d.error("PYB0001", Span::new(s, i), "texto sin cerrar: falta la comilla final");
                }
                push(&mut out, T::Str(text), s, i);
            }
            b'0'..=b'9' | b'$' => {
                let s = i;
                let (radix, digits_from) = if c == b'$' {
                    (16, i + 1)
                } else if c == b'0' && i + 1 < b.len() && (b[i + 1] == b'x' || b[i + 1] == b'X') {
                    (16, i + 2)
                } else {
                    (10, i)
                };
                i = digits_from;
                while i < b.len() && (b[i] as char).is_digit(radix) || (radix == 10 && i < b.len() && b[i] == b'_') {
                    i += 1;
                }
                let digits: String = src[digits_from..i].chars().filter(|c| *c != '_').collect();
                match i128::from_str_radix(&digits, radix) {
                    Ok(v) => push(&mut out, T::Int(v), s, i),
                    Err(_) => {
                        d.error("PYB0002", Span::new(s, i.max(s + 1)), "numero invalido (decimal, $FF o 0xFF en hexadecimal, %1010 en binario)");
                        push(&mut out, T::Int(0), s, i.max(s + 1));
                    }
                }
            }
            // `%1010` es binario cuando no sigue a un valor; despues de un valor es el modulo
            b'%' if b.get(i + 1).is_some_and(|x| matches!(x, b'0' | b'1')) && !out.last().is_some_and(|t| matches!(t.t, T::Ident(_) | T::Int(_) | T::RParen)) => {
                let s = i;
                i += 1;
                let from = i;
                while i < b.len() && matches!(b[i], b'0' | b'1' | b'_') {
                    i += 1;
                }
                let digits: String = src[from..i].chars().filter(|c| *c != '_').collect();
                push(&mut out, T::Int(i128::from_str_radix(&digits, 2).unwrap_or(0)), s, i);
            }
            c if c.is_ascii_alphabetic() || c == b'_' => {
                let s = i;
                while i < b.len() && (b[i].is_ascii_alphanumeric() || b[i] == b'_') {
                    i += 1;
                }
                push(&mut out, T::Ident(src[s..i].to_string()), s, i);
            }
            _ => {
                let s = i;
                let two = if i + 1 < b.len() { &src[i..i + 2] } else { "" };
                let (t, n) = match (c, two) {
                    (_, "<<") => (T::Shl, 2),
                    (_, ">>") => (T::Shr, 2),
                    (_, "<>") => (T::Ne, 2),
                    (_, "<=") => (T::Le, 2),
                    (_, ">=") => (T::Ge, 2),
                    (_, "+=") => (T::AugAssign('+'), 2),
                    (_, "-=") => (T::AugAssign('-'), 2),
                    (_, "*=") => (T::AugAssign('*'), 2),
                    (b':', _) => (T::Colon, 1),
                    (b',', _) => (T::Comma, 1),
                    (b'.', _) => (T::Dot, 1),
                    (b'(', _) => (T::LParen, 1),
                    (b')', _) => (T::RParen, 1),
                    (b'+', _) => (T::Plus, 1),
                    (b'-', _) => (T::Minus, 1),
                    (b'*', _) => (T::Star, 1),
                    (b'/', _) => (T::Slash, 1),
                    (b'%', _) => (T::Percent, 1),
                    (b'&', _) => (T::Amp, 1),
                    (b'|', _) => (T::Pipe, 1),
                    (b'^', _) => (T::Caret, 1),
                    (b'~', _) => (T::Tilde, 1),
                    (b'#', _) => (T::Hash, 1),
                    (b'=', _) => (T::Eq, 1),
                    (b'<', _) => (T::Lt, 1),
                    (b'>', _) => (T::Gt, 1),
                    _ => {
                        let ch = src[i..].chars().next().unwrap();
                        d.error("PYB0003", Span::new(i, i + ch.len_utf8()), format!("caracter no soportado en PyPIC BASIC: '{ch}'"));
                        i += ch.len_utf8();
                        continue;
                    }
                };
                i += n;
                push(&mut out, t, s, i);
            }
        }
    }
    push(&mut out, T::Newline, b.len(), b.len());
    push(&mut out, T::Eof, b.len(), b.len());
    out
}
