//! Parser de PyxPic BASIC: del texto a un arbol propio (`BStmt`, `BExpr`) que `lower.rs` traduce al AST comun.

use super::lexer::{lex, Token, T};
use crate::diag::{Diagnostics, Span};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BOp {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    And,
    Or,
    Xor,
    Shl,
    Shr,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BCmp {
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
}

#[derive(Clone, Debug)]
pub struct BArg {
    pub name: Option<String>,
    pub value: BExpr,
}

#[derive(Clone, Debug)]
pub struct BExpr {
    pub span: Span,
    pub kind: BExprKind,
}

#[derive(Clone, Debug)]
pub enum BExprKind {
    Int(i128),
    /// Un literal con unidad (`8MHZ`, `500ms`).
    Quantity(i128, crate::units::Dim),
    Name(String),
    /// `B.0` o `pinB.0`: el pin RB0.
    Pin(char, u8),
    /// Un texto entre comillas (solo en SERTXD).
    Str(String),
    /// `#expr` (solo en SERTXD): el valor en decimal.
    Dec(Box<BExpr>),
    Neg(Box<BExpr>),
    /// `~x` y `NOT x`: negacion de bits o logica, segun el contexto.
    Not(Box<BExpr>),
    Bin(BOp, Box<BExpr>, Box<BExpr>),
    Cmp(BCmp, Box<BExpr>, Box<BExpr>),
    /// `nombre(args)` (arreglo o funcion) y `objeto.metodo(args)` (el SDK).
    Call {
        obj: Option<String>,
        name: String,
        args: Vec<BArg>,
    },
}

#[derive(Clone, Debug)]
pub struct BStmt {
    pub span: Span,
    pub kind: BStmtKind,
}

#[derive(Clone, Debug)]
pub enum CaseItem {
    Value(BExpr),
    Range(BExpr, BExpr),
}

#[derive(Clone, Debug)]
pub enum BStmtKind {
    Device(String),
    /// `BAUD 9600`: la velocidad de la UART (la que usan SERTXD y SERRXD y la que inicia el programa); 8N1.
    Baud(BExpr),
    /// `CLOCK CRYSTAL 16MHZ [MODE HS]`: la fuente, su frecuencia (un literal con unidad; la forma `4 MHZ` tambien vale) y el modo.
    Clock {
        kind: String,
        freq: Option<BExpr>,
        mode: Option<String>,
    },
    /// `PLL 4`, `CLOCKDIVIDER 2`, `CLOCKOUTPUT ON`, `FAILSAFE ON`, `CLOCKSWITCHING ON`: un ajuste del reloj (con su nombre en Python).
    ClockSetting {
        name: &'static str,
        word: Option<String>,
        value: Option<i128>,
    },
    Import {
        module: String,
        names: Vec<(String, Span)>,
    },
    Symbol {
        name: String,
        value: BExpr,
    },
    Dim {
        name: String,
        bits: u8,
        len: Option<i128>,
        init: Option<BExpr>,
    },
    Assign {
        target: String,
        target_span: Span,
        index: Option<BExpr>,
        value: BExpr,
    },
    Label(String),
    Goto(String),
    Gosub(String),
    Return,
    End,
    Exit,
    /// `ON INTERRUPT etiqueta`: la etiqueta (una subrutina que termina en RETURN) es la rutina de interrupcion.
    OnInterrupt(String),
    /// `ASM nombre ... END ASM`: una rutina escrita en ensamblador PIC-AS (`text`), que se llama con `GOSUB nombre`. `offset` es la posicion en bytes del
    /// primer caracter del cuerpo en el fuente.
    Asm {
        name: String,
        text: String,
        offset: usize,
    },
    If {
        arms: Vec<(BExpr, Vec<BStmt>)>,
        orelse: Vec<BStmt>,
    },
    For {
        var: String,
        var_span: Span,
        from: BExpr,
        to: BExpr,
        step: Option<BExpr>,
        body: Vec<BStmt>,
    },
    /// `DO [WHILE|UNTIL c] ... LOOP [WHILE|UNTIL c]`: cada condicion es (es_until, condicion).
    Do {
        pre: Option<(bool, BExpr)>,
        post: Option<(bool, BExpr)>,
        body: Vec<BStmt>,
    },
    While {
        cond: BExpr,
        body: Vec<BStmt>,
    },
    Select {
        subject: BExpr,
        cases: Vec<(Vec<CaseItem>, Vec<BStmt>)>,
        default: Vec<BStmt>,
    },
    /// Un comando (`HIGH`, `PAUSE`, `READADC`...) o una llamada al SDK (`UART.write(b0)`).
    Cmd {
        obj: Option<String>,
        name: String,
        name_span: Span,
        args: Vec<BArg>,
    },
}

const BLOCK_END: &[&str] = &["ENDIF", "ELSE", "ELSEIF", "NEXT", "LOOP", "WEND", "CASE"];

struct Parser<'a> {
    toks: Vec<Token>,
    pos: usize,
    d: &'a mut Diagnostics,
    /// Los cuerpos `ASM ... END ASM` sacados del fuente antes de analizarlo, en orden.
    asm: std::collections::VecDeque<(String, usize)>,
}

/// Saca los cuerpos de ensamblador del fuente: en el texto que se analiza quedan en blanco (con los mismos saltos de linea y la misma longitud en bytes, para que
/// las posiciones no cambien) y aparte van su texto y su posicion.
fn extract_asm(src: &str) -> (String, std::collections::VecDeque<(String, usize)>) {
    let mut out = String::with_capacity(src.len());
    let mut blocks = std::collections::VecDeque::new();
    let mut current: Option<(usize, String)> = None;
    let mut offset = 0;
    for line in src.split_inclusive('\n') {
        let words: Vec<&str> = line.split('\'').next().unwrap_or("").split_whitespace().collect();
        let is_header = words.len() == 2 && words[0].eq_ignore_ascii_case("ASM") && words[1].chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
        let is_end = words.len() == 2 && words[0].eq_ignore_ascii_case("END") && words[1].eq_ignore_ascii_case("ASM");
        match current.as_mut() {
            None => {
                out.push_str(line);
                if is_header {
                    current = Some((offset + line.len(), String::new()));
                }
            }
            Some(_) if is_end => {
                let (start, text) = current.take().unwrap();
                blocks.push_back((text, start));
                out.push_str(line);
            }
            Some((_, text)) => {
                text.push_str(line);
                out.extend(line.chars().map(|c| if c == '\n' { "\n".to_string() } else { " ".repeat(c.len_utf8()) }));
            }
        }
        offset += line.len();
    }
    (out, blocks)
}

pub fn parse(src: &str, d: &mut Diagnostics) -> Vec<BStmt> {
    let (clean, asm) = extract_asm(src);
    let toks = lex(&clean, d);
    let mut p = Parser { toks, pos: 0, d, asm };
    let mut out = Vec::new();
    loop {
        p.skip_newlines();
        if p.at_eof() {
            break;
        }
        match p.statement() {
            Some(s) => out.extend(s),
            None => p.recover(),
        }
    }
    out
}

fn kw(t: &T, word: &str) -> bool {
    matches!(t, T::Ident(s) if s.eq_ignore_ascii_case(word))
}

impl<'a> Parser<'a> {
    fn peek(&self) -> &T {
        &self.toks[self.pos].t
    }
    fn peek_at(&self, n: usize) -> &T {
        &self.toks[(self.pos + n).min(self.toks.len() - 1)].t
    }
    fn span(&self) -> Span {
        self.toks[self.pos].span
    }
    fn prev_span(&self) -> Span {
        self.toks[self.pos.saturating_sub(1)].span
    }
    fn bump(&mut self) -> Token {
        let t = self.toks[self.pos].clone();
        if self.pos < self.toks.len() - 1 {
            self.pos += 1;
        }
        t
    }
    fn at_eof(&self) -> bool {
        matches!(self.peek(), T::Eof)
    }
    fn skip_newlines(&mut self) {
        while matches!(self.peek(), T::Newline | T::Colon) {
            self.bump();
        }
    }
    fn is_kw(&self, word: &str) -> bool {
        kw(self.peek(), word)
    }
    fn eat_kw(&mut self, word: &str) -> bool {
        if self.is_kw(word) {
            self.bump();
            true
        } else {
            false
        }
    }
    fn err(&mut self, code: &'static str, span: Span, msg: impl Into<String>) {
        self.d.error(code, span, msg);
    }
    /// Descarta lo que queda de la linea tras un error.
    fn recover(&mut self) {
        while !matches!(self.peek(), T::Newline | T::Eof) {
            self.bump();
        }
    }
    fn end_of_statement(&mut self) -> bool {
        if matches!(self.peek(), T::Newline | T::Eof | T::Colon) || self.is_kw("ELSE") {
            true
        } else {
            let s = self.span();
            self.err("PYB0010", s, "sobra algo al final de la sentencia (una sentencia por linea; ':' separa varias)");
            self.recover();
            false
        }
    }
    fn ident(&mut self, what: &str) -> Option<(String, Span)> {
        match self.peek().clone() {
            T::Ident(s) => {
                let sp = self.span();
                self.bump();
                Some((s, sp))
            }
            _ => {
                let sp = self.span();
                self.err("PYB0011", sp, format!("se esperaba {what}"));
                None
            }
        }
    }
    fn expect(&mut self, t: T, what: &str) -> Option<Span> {
        if *self.peek() == t {
            Some(self.bump().span)
        } else {
            let sp = self.span();
            self.err("PYB0012", sp, format!("se esperaba {what}"));
            None
        }
    }

    // ---------------------------------------------------------------- expresiones
    fn expr(&mut self) -> Option<BExpr> {
        self.or_expr()
    }

    fn bin(&self, op: BOp, l: BExpr, r: BExpr) -> BExpr {
        BExpr { span: l.span.to(r.span), kind: BExprKind::Bin(op, Box::new(l), Box::new(r)) }
    }

    fn or_expr(&mut self) -> Option<BExpr> {
        let mut l = self.and_expr()?;
        loop {
            let op = if self.is_kw("OR") {
                BOp::Or
            } else if self.is_kw("XOR") {
                BOp::Xor
            } else {
                return Some(l);
            };
            self.bump();
            let r = self.and_expr()?;
            l = self.bin(op, l, r);
        }
    }

    fn and_expr(&mut self) -> Option<BExpr> {
        let mut l = self.not_expr()?;
        while self.is_kw("AND") {
            self.bump();
            let r = self.not_expr()?;
            l = self.bin(BOp::And, l, r);
        }
        Some(l)
    }

    fn not_expr(&mut self) -> Option<BExpr> {
        if self.is_kw("NOT") {
            let s = self.bump().span;
            let x = self.not_expr()?;
            return Some(BExpr { span: s.to(x.span), kind: BExprKind::Not(Box::new(x)) });
        }
        self.cmp_expr()
    }

    fn cmp_expr(&mut self) -> Option<BExpr> {
        let l = self.bitor_expr()?;
        let op = match self.peek() {
            T::Eq => BCmp::Eq,
            T::Ne => BCmp::Ne,
            T::Lt => BCmp::Lt,
            T::Le => BCmp::Le,
            T::Gt => BCmp::Gt,
            T::Ge => BCmp::Ge,
            _ => return Some(l),
        };
        self.bump();
        let r = self.bitor_expr()?;
        Some(BExpr { span: l.span.to(r.span), kind: BExprKind::Cmp(op, Box::new(l), Box::new(r)) })
    }

    fn bitor_expr(&mut self) -> Option<BExpr> {
        let mut l = self.bitand_expr()?;
        loop {
            let op = match self.peek() {
                T::Pipe => BOp::Or,
                T::Caret => BOp::Xor,
                _ => return Some(l),
            };
            self.bump();
            let r = self.bitand_expr()?;
            l = self.bin(op, l, r);
        }
    }

    fn bitand_expr(&mut self) -> Option<BExpr> {
        let mut l = self.shift_expr()?;
        while matches!(self.peek(), T::Amp) {
            self.bump();
            let r = self.shift_expr()?;
            l = self.bin(BOp::And, l, r);
        }
        Some(l)
    }

    fn shift_expr(&mut self) -> Option<BExpr> {
        let mut l = self.add_expr()?;
        loop {
            let op = match self.peek() {
                T::Shl => BOp::Shl,
                T::Shr => BOp::Shr,
                _ => return Some(l),
            };
            self.bump();
            let r = self.add_expr()?;
            l = self.bin(op, l, r);
        }
    }

    fn add_expr(&mut self) -> Option<BExpr> {
        let mut l = self.mul_expr()?;
        loop {
            let op = match self.peek() {
                T::Plus => BOp::Add,
                T::Minus => BOp::Sub,
                _ => return Some(l),
            };
            self.bump();
            let r = self.mul_expr()?;
            l = self.bin(op, l, r);
        }
    }

    fn mul_expr(&mut self) -> Option<BExpr> {
        let mut l = self.unary_expr()?;
        loop {
            let op = match self.peek() {
                T::Star => BOp::Mul,
                T::Slash => BOp::Div,
                T::Percent => BOp::Mod,
                t if kw(t, "MOD") => BOp::Mod,
                _ => return Some(l),
            };
            self.bump();
            let r = self.unary_expr()?;
            l = self.bin(op, l, r);
        }
    }

    fn unary_expr(&mut self) -> Option<BExpr> {
        match self.peek() {
            T::Minus => {
                let s = self.bump().span;
                let x = self.unary_expr()?;
                Some(BExpr { span: s.to(x.span), kind: BExprKind::Neg(Box::new(x)) })
            }
            T::Hash => {
                let s = self.bump().span;
                let x = self.unary_expr()?;
                Some(BExpr { span: s.to(x.span), kind: BExprKind::Dec(Box::new(x)) })
            }
            T::Tilde => {
                let s = self.bump().span;
                let x = self.unary_expr()?;
                Some(BExpr { span: s.to(x.span), kind: BExprKind::Not(Box::new(x)) })
            }
            T::Plus => {
                self.bump();
                self.unary_expr()
            }
            _ => self.atom(),
        }
    }

    fn args(&mut self) -> Option<Vec<BArg>> {
        let mut args = Vec::new();
        if matches!(self.peek(), T::RParen) {
            self.bump();
            return Some(args);
        }
        loop {
            // `nombre = valor` dentro de una llamada es un argumento con nombre (`ADC.start(clock = FOSC_8)`)
            let name = match (self.peek(), self.peek_at(1)) {
                (T::Ident(n), T::Eq) => Some(n.clone()),
                _ => None,
            };
            if name.is_some() {
                self.bump();
                self.bump();
            }
            args.push(BArg { name, value: self.expr()? });
            if matches!(self.peek(), T::Comma) {
                self.bump();
                continue;
            }
            self.expect(T::RParen, "')' o ','")?;
            return Some(args);
        }
    }

    fn atom(&mut self) -> Option<BExpr> {
        let sp = self.span();
        match self.peek().clone() {
            T::Int(v) => {
                self.bump();
                Some(BExpr { span: sp, kind: BExprKind::Int(v) })
            }
            T::Quantity(v, d) => {
                self.bump();
                Some(BExpr { span: sp, kind: BExprKind::Quantity(v, d) })
            }
            T::Str(text) => {
                self.bump();
                Some(BExpr { span: sp, kind: BExprKind::Str(text) })
            }
            T::LParen => {
                self.bump();
                let e = self.expr()?;
                self.expect(T::RParen, "')'")?;
                Some(e)
            }
            T::Ident(name) => {
                self.bump();
                // B.0 / pinB.0
                if matches!(self.peek(), T::Dot) {
                    let upper = name.to_ascii_uppercase();
                    let port = upper.strip_prefix("PIN").unwrap_or(&upper);
                    if port.len() == 1 && matches!(port.as_bytes()[0], b'A'..=b'E') && matches!(self.peek_at(1), T::Int(_)) {
                        self.bump();
                        let T::Int(bit) = self.bump().t else { unreachable!() };
                        if bit > 7 {
                            self.err("PYB0013", sp.to(self.prev_span()), "un puerto tiene los bits 0 a 7");
                            return None;
                        }
                        return Some(BExpr { span: sp.to(self.prev_span()), kind: BExprKind::Pin(port.chars().next().unwrap(), bit as u8) });
                    }
                    // objeto.metodo(args)
                    if let T::Ident(mut m) = self.peek_at(1).clone() {
                        self.bump();
                        self.bump();
                        // REGISTRO.BIT.metodo: `INTCON.GIE.set`; el bit y el metodo van juntos en el nombre (`GIE.set`)
                        if matches!(self.peek(), T::Dot) {
                            if let T::Ident(method) = self.peek_at(1).clone() {
                                self.bump();
                                self.bump();
                                m = format!("{m}.{method}");
                            }
                        }
                        let args = if matches!(self.peek(), T::LParen) {
                            self.bump();
                            self.args()?
                        } else {
                            Vec::new()
                        };
                        return Some(BExpr { span: sp.to(self.prev_span()), kind: BExprKind::Call { obj: Some(name), name: m, args } });
                    }
                    let s = self.span();
                    self.err("PYB0014", s, "despues del punto se esperaba un metodo (UART.read()) o un bit de puerto (B.0)");
                    return None;
                }
                if matches!(self.peek(), T::LParen) {
                    self.bump();
                    let args = self.args()?;
                    return Some(BExpr { span: sp.to(self.prev_span()), kind: BExprKind::Call { obj: None, name, args } });
                }
                Some(BExpr { span: sp, kind: BExprKind::Name(name) })
            }
            _ => {
                self.err("PYB0015", sp, "se esperaba un valor: un numero, una variable o una expresion");
                None
            }
        }
    }

    // ---------------------------------------------------------------- sentencias
    /// Una sentencia (o varias si la linea es `IF c THEN a : b`); `None` si hubo un error ya reportado.
    fn statement(&mut self) -> Option<Vec<BStmt>> {
        let sp = self.span();
        let T::Ident(word) = self.peek().clone() else {
            self.err("PYB0020", sp, "una sentencia empieza por una palabra (HIGH, IF, una variable...)");
            return None;
        };
        let upper = word.to_ascii_uppercase();
        // `etiqueta:`
        if matches!(self.peek_at(1), T::Colon) && !is_keyword(&upper) {
            self.bump();
            self.bump();
            return Some(vec![BStmt { span: sp, kind: BStmtKind::Label(word) }]);
        }
        let kind = match upper.as_str() {
            "DEVICE" => {
                self.bump();
                let (n, _) = self.ident("el nombre del PIC (DEVICE PIC16F887)")?;
                BStmtKind::Device(n)
            }
            "BAUD" => {
                self.bump();
                BStmtKind::Baud(self.expr()?)
            }
            "CLOCK" => {
                self.bump();
                let (kind, ksp) = self.ident("la fuente del reloj: INTERNAL, CRYSTAL, EXTERNAL, RESONATOR o RC")?;
                let kind = match kind.to_ascii_uppercase().as_str() {
                    "INTERNAL" => "Internal",
                    "CRYSTAL" => "Crystal",
                    "EXTERNAL" => "External",
                    "RESONATOR" => "Resonator",
                    "RC" => "RC",
                    _ => {
                        self.err("PYB0021", ksp, "la fuente del reloj es INTERNAL, CRYSTAL, EXTERNAL, RESONATOR o RC: CLOCK INTERNAL 4 MHZ");
                        return None;
                    }
                };
                // `CLOCK CRYSTAL 16MHZ` o la forma antigua `CLOCK CRYSTAL 16 MHZ`; `CLOCK INTERNAL` solo vale 4 MHz
                let freq = match self.peek().clone() {
                    T::Quantity(..) => Some(self.expr()?),
                    T::Int(v) => {
                        let sp = self.span();
                        self.bump();
                        let (unit, usp) = self.ident("la unidad: MHZ, KHZ o HZ")?;
                        let mult = match unit.to_ascii_uppercase().as_str() {
                            "MHZ" => 1_000_000,
                            "KHZ" => 1_000,
                            "HZ" => 1,
                            _ => {
                                self.err("PYB0021", usp, "la unidad del reloj es MHZ, KHZ o HZ");
                                return None;
                            }
                        };
                        Some(BExpr { span: sp, kind: BExprKind::Quantity(v * mult, crate::units::Dim::Frequency) })
                    }
                    _ => None,
                };
                let mode = if self.eat_kw("MODE") { Some(self.ident("el modo del oscilador (AUTO, LP, XT, HS, EC...)")?.0) } else { None };
                BStmtKind::Clock { kind: kind.to_string(), freq, mode }
            }
            "PLL" | "CLOCKDIVIDER" | "CLOCKOUTPUT" | "FAILSAFE" | "CLOCKSWITCHING" => {
                let name = match self.ident("el ajuste del reloj")?.0.to_ascii_uppercase().as_str() {
                    "PLL" => "PLL",
                    "CLOCKDIVIDER" => "CLOCK_DIVIDER",
                    "CLOCKOUTPUT" => "CLOCK_OUTPUT",
                    "FAILSAFE" => "FAIL_SAFE_CLOCK",
                    _ => "CLOCK_SWITCHING",
                };
                match self.peek().clone() {
                    T::Int(v) => {
                        self.bump();
                        BStmtKind::ClockSetting { name, word: None, value: Some(v) }
                    }
                    T::Ident(w) => {
                        let sp = self.span();
                        self.bump();
                        match w.to_ascii_uppercase().as_str() {
                            "ON" => BStmtKind::ClockSetting { name, word: Some("On".into()), value: None },
                            "OFF" => BStmtKind::ClockSetting { name, word: Some("Off".into()), value: None },
                            _ => {
                                self.err("PYB0021", sp, "el valor es ON, OFF o un numero: PLL 4, CLOCKOUTPUT ON");
                                return None;
                            }
                        }
                    }
                    _ => {
                        let sp = self.span();
                        self.err("PYB0021", sp, "falta el valor: PLL 4, CLOCKOUTPUT ON");
                        return None;
                    }
                }
            }
            "FROM" => {
                self.bump();
                let mut module = self.ident("el modulo (FROM pyxpic.periph.adc IMPORT ADC)")?.0;
                while matches!(self.peek(), T::Dot) {
                    self.bump();
                    module.push('.');
                    module.push_str(&self.ident("el resto del nombre del modulo")?.0);
                }
                if !self.eat_kw("IMPORT") {
                    let s = self.span();
                    self.err("PYB0022", s, "se esperaba IMPORT: FROM pyxpic.periph.adc IMPORT ADC, AN0");
                    return None;
                }
                let mut names = Vec::new();
                loop {
                    names.push(self.ident("un nombre que importar")?);
                    if matches!(self.peek(), T::Comma) {
                        self.bump();
                    } else {
                        break;
                    }
                }
                BStmtKind::Import { module, names }
            }
            "SYMBOL" => {
                self.bump();
                let (name, _) = self.ident("el nombre del simbolo")?;
                self.expect(T::Eq, "'=' (SYMBOL nombre = valor)")?;
                BStmtKind::Symbol { name, value: self.expr()? }
            }
            "DIM" => {
                self.bump();
                let (name, nsp) = self.ident("el nombre de la variable")?;
                let mut len = None;
                if matches!(self.peek(), T::LParen) {
                    self.bump();
                    match self.bump().t {
                        T::Int(n) if n > 0 => len = Some(n),
                        _ => {
                            self.err("PYB0023", nsp, "el tamano de un arreglo es un numero positivo: DIM tabla(16) AS BYTE");
                            return None;
                        }
                    }
                    self.expect(T::RParen, "')'")?;
                }
                if !self.eat_kw("AS") {
                    let s = self.span();
                    self.err("PYB0023", s, "se esperaba AS: DIM x AS BYTE (BYTE, BIT o WORD)");
                    return None;
                }
                let (ty, tsp) = self.ident("el tipo: BYTE, BIT o WORD")?;
                let bits = match ty.to_ascii_uppercase().as_str() {
                    "BYTE" | "BIT" => 8,
                    "WORD" => 16,
                    _ => {
                        self.err("PYB0023", tsp, "el tipo es BYTE, BIT o WORD");
                        return None;
                    }
                };
                let init = if matches!(self.peek(), T::Eq) {
                    self.bump();
                    Some(self.expr()?)
                } else {
                    None
                };
                BStmtKind::Dim { name, bits, len, init }
            }
            "LET" => {
                self.bump();
                return self.assignment();
            }
            "GOTO" | "GOSUB" => {
                self.bump();
                let (l, _) = self.ident("el nombre de una etiqueta")?;
                if upper == "GOTO" {
                    BStmtKind::Goto(l)
                } else {
                    BStmtKind::Gosub(l)
                }
            }
            "ON" => {
                self.bump();
                if !self.eat_kw("INTERRUPT") {
                    let s = self.span();
                    self.err("PYB0100", s, "se esperaba INTERRUPT: ON INTERRUPT etiqueta");
                    return None;
                }
                let (l, _) = self.ident("el nombre de la etiqueta de la interrupcion")?;
                BStmtKind::OnInterrupt(l)
            }
            "ASM" => {
                self.bump();
                let (name, _) = self.ident("el nombre de la rutina: ASM nombre")?;
                self.skip_newlines();
                let (text, offset) = self.asm.pop_front().unwrap_or_default();
                if !(self.eat_kw("END") && self.eat_kw("ASM")) {
                    let s = self.span();
                    self.err("PYB0101", s, "falta END ASM para cerrar el bloque de ensamblador");
                    return None;
                }
                BStmtKind::Asm { name, text, offset }
            }
            "RETURN" => {
                self.bump();
                BStmtKind::Return
            }
            "EXIT" => {
                self.bump();
                BStmtKind::Exit
            }
            "END" => {
                self.bump();
                BStmtKind::End
            }
            "IF" => return self.if_stmt(),
            "FOR" => return self.for_stmt(),
            "DO" => return self.do_stmt(),
            "WHILE" => return self.while_stmt(),
            "SELECT" => return self.select_stmt(),
            _ if BLOCK_END.contains(&upper.as_str()) || matches!(upper.as_str(), "ENDSELECT" | "THEN" | "TO" | "STEP") => {
                self.err("PYB0024", sp, format!("'{word}' sin su bloque: no hay un IF, FOR, DO, WHILE o SELECT abierto"));
                return None;
            }
            _ => return self.assignment_or_command(),
        };
        if !self.end_of_statement() {
            return None;
        }
        Some(vec![BStmt { span: sp.to(self.prev_span()), kind }])
    }

    /// `x = expr`, `x(i) = expr`, `x += expr`, `obj.metodo(args)` o un comando con argumentos separados por comas.
    fn assignment_or_command(&mut self) -> Option<Vec<BStmt>> {
        let (name, nsp) = match self.peek().clone() {
            T::Ident(n) => (n, self.span()),
            _ => unreachable!(),
        };
        match self.peek_at(1) {
            T::Eq | T::LParen if !is_command(&name) => self.assignment(),
            T::AugAssign(_) => self.assignment(),
            T::Dot => {
                // objeto.metodo(args), o B.0 = ... (no se asigna a un pin: se usa HIGH/LOW)
                let call = self.atom()?;
                let sp = call.span;
                let BExprKind::Call { obj, name, mut args } = call.kind else {
                    self.err("PYB0025", sp, "un pin no se asigna: usa HIGH, LOW o TOGGLE");
                    return None;
                };
                // forma de comando, sin parentesis: `UART.start baud = 9600`, `TMR2.every_ms 20`
                if !matches!(self.peek(), T::Newline | T::Eof | T::Colon) && !self.is_kw("ELSE") {
                    args.extend(self.command_args()?);
                }
                if !self.end_of_statement() {
                    return None;
                }
                Some(vec![BStmt { span: sp, kind: BStmtKind::Cmd { obj, name, name_span: sp, args } }])
            }
            _ => {
                if !is_command(&name) {
                    self.bump();
                    self.err("PYB0026", nsp, format!("'{name}' no es un comando ni una variable asignada: una variable se asigna con '{name} = valor'"));
                    return None;
                }
                self.bump();
                let mut args = Vec::new();
                // SERTXD y SERRXD llevan sus datos entre parentesis, como en PICAXE: SERTXD("x = ", #b0, CR, LF)
                let parens = matches!(name.to_ascii_uppercase().as_str(), "SERTXD" | "SERRXD") && matches!(self.peek(), T::LParen);
                if parens {
                    self.bump();
                    if !matches!(self.peek(), T::RParen) {
                        args = self.command_args()?;
                    }
                    self.expect(T::RParen, "')'")?;
                } else if !matches!(self.peek(), T::Newline | T::Eof | T::Colon) {
                    args = self.command_args()?;
                }
                if !self.end_of_statement() {
                    return None;
                }
                Some(vec![BStmt { span: nsp.to(self.prev_span()), kind: BStmtKind::Cmd { obj: None, name, name_span: nsp, args } }])
            }
        }
    }

    /// Los argumentos de un comando sin parentesis: valores separados por comas; `nombre = valor` es un argumento con nombre.
    fn command_args(&mut self) -> Option<Vec<BArg>> {
        let mut args = Vec::new();
        loop {
            let name = match (self.peek(), self.peek_at(1)) {
                (T::Ident(n), T::Eq) => Some(n.clone()),
                _ => None,
            };
            if name.is_some() {
                self.bump();
                self.bump();
            }
            args.push(BArg { name, value: self.expr()? });
            if matches!(self.peek(), T::Comma) {
                self.bump();
            } else {
                return Some(args);
            }
        }
    }

    fn assignment(&mut self) -> Option<Vec<BStmt>> {
        let (target, tsp) = self.ident("una variable")?;
        let mut index = None;
        if matches!(self.peek(), T::LParen) {
            self.bump();
            index = Some(self.expr()?);
            self.expect(T::RParen, "')'")?;
        }
        let value = match self.bump().t {
            T::Eq => self.expr()?,
            T::AugAssign(op) => {
                // x += e  ->  x = x + e (con el indice repetido en un arreglo)
                let lhs = match &index {
                    Some(i) => BExpr { span: tsp, kind: BExprKind::Call { obj: None, name: target.clone(), args: vec![BArg { name: None, value: i.clone() }] } },
                    None => BExpr { span: tsp, kind: BExprKind::Name(target.clone()) },
                };
                let r = self.expr()?;
                let bop = match op {
                    '+' => BOp::Add,
                    '-' => BOp::Sub,
                    _ => BOp::Mul,
                };
                self.bin(bop, lhs, r)
            }
            _ => {
                self.err("PYB0027", tsp, "se esperaba '=' en la asignacion");
                return None;
            }
        };
        if !self.end_of_statement() {
            return None;
        }
        Some(vec![BStmt { span: tsp.to(value.span), kind: BStmtKind::Assign { target, target_span: tsp, index, value } }])
    }

    /// Las sentencias hasta una palabra de cierre (sin consumirla).
    fn block(&mut self, until: &[&str]) -> Option<Vec<BStmt>> {
        let mut out = Vec::new();
        loop {
            self.skip_newlines();
            if self.at_eof() {
                let s = self.span();
                self.err("PYB0030", s, format!("falta cerrar el bloque: se esperaba {}", until.join(" o ")));
                return None;
            }
            if until.iter().any(|u| self.is_kw(u)) {
                return Some(out);
            }
            // `END IF` / `END SELECT` se leen como ENDIF / ENDSELECT
            if self.is_kw("END") && matches!(self.peek_at(1), T::Ident(n) if n.eq_ignore_ascii_case("IF") || n.eq_ignore_ascii_case("SELECT")) {
                let which = if matches!(self.peek_at(1), T::Ident(n) if n.eq_ignore_ascii_case("IF")) { "ENDIF" } else { "ENDSELECT" };
                if until.contains(&which) {
                    return Some(out);
                }
            }
            match self.statement() {
                Some(s) => out.extend(s),
                None => self.recover(),
            }
        }
    }

    fn close(&mut self, word: &str) -> bool {
        if self.eat_kw(word) {
            return true;
        }
        // ENDIF o END IF
        let (a, b) = word.split_at(3.min(word.len()));
        if a.eq_ignore_ascii_case("END") && self.is_kw("END") && matches!(self.peek_at(1), T::Ident(n) if n.eq_ignore_ascii_case(b)) {
            self.bump();
            self.bump();
            return true;
        }
        false
    }

    fn if_stmt(&mut self) -> Option<Vec<BStmt>> {
        let sp = self.bump().span;
        let cond = self.expr()?;
        if !self.eat_kw("THEN") {
            let s = self.span();
            self.err("PYB0031", s, "se esperaba THEN: IF condicion THEN");
            return None;
        }
        // una linea: IF c THEN etiqueta | IF c THEN sentencia [ELSE sentencia]
        if !matches!(self.peek(), T::Newline | T::Eof) {
            let body = self.inline_body()?;
            let mut orelse = Vec::new();
            if self.eat_kw("ELSE") {
                orelse = self.inline_body()?;
            }
            return Some(vec![BStmt { span: sp.to(self.prev_span()), kind: BStmtKind::If { arms: vec![(cond, body)], orelse } }]);
        }
        let mut arms = Vec::new();
        let mut orelse = Vec::new();
        let mut cur = cond;
        loop {
            let body = self.block(&["ELSE", "ELSEIF", "ENDIF", "ENDSELECT"])?;
            arms.push((cur.clone(), body));
            if self.eat_kw("ELSEIF") {
                cur = self.expr()?;
                if !self.eat_kw("THEN") {
                    let s = self.span();
                    self.err("PYB0031", s, "se esperaba THEN: ELSEIF condicion THEN");
                    return None;
                }
                continue;
            }
            if self.eat_kw("ELSE") {
                orelse = self.block(&["ENDIF", "ENDSELECT"])?;
            }
            break;
        }
        if !self.close("ENDIF") {
            let s = self.span();
            self.err("PYB0030", s, "falta ENDIF (o END IF) para cerrar el IF");
            return None;
        }
        Some(vec![BStmt { span: sp.to(self.prev_span()), kind: BStmtKind::If { arms, orelse } }])
    }

    /// El cuerpo de un IF de una linea: `etiqueta` (un GOTO) o sentencias hasta ELSE o el fin de la linea.
    fn inline_body(&mut self) -> Option<Vec<BStmt>> {
        let sp = self.span();
        if let T::Ident(n) = self.peek().clone() {
            let after_is_end = matches!(self.peek_at(1), T::Newline | T::Eof | T::Colon) || kw(self.peek_at(1), "ELSE");
            if after_is_end && !is_keyword(&n.to_ascii_uppercase()) && !is_command(&n) {
                self.bump();
                return Some(vec![BStmt { span: sp, kind: BStmtKind::Goto(n) }]);
            }
        }
        let mut out = Vec::new();
        loop {
            out.extend(self.statement()?);
            // `:` encadena sentencias dentro del cuerpo; ELSE o el fin de la linea lo cierran
            if matches!(self.peek(), T::Colon) {
                self.bump();
                continue;
            }
            return Some(out);
        }
    }

    fn for_stmt(&mut self) -> Option<Vec<BStmt>> {
        let sp = self.bump().span;
        let (var, var_span) = self.ident("la variable del FOR")?;
        self.expect(T::Eq, "'=' (FOR b0 = 1 TO 10)")?;
        let from = self.expr()?;
        if !self.eat_kw("TO") {
            let s = self.span();
            self.err("PYB0032", s, "se esperaba TO: FOR b0 = 1 TO 10");
            return None;
        }
        let to = self.expr()?;
        let step = if self.eat_kw("STEP") { Some(self.expr()?) } else { None };
        let body = self.block(&["NEXT"])?;
        self.bump(); // NEXT
        if let T::Ident(n) = self.peek().clone() {
            if !is_keyword(&n.to_ascii_uppercase()) && !matches!(self.peek_at(1), T::Eq | T::LParen | T::Dot | T::Colon) {
                self.bump(); // NEXT variable
            }
        }
        Some(vec![BStmt { span: sp.to(self.prev_span()), kind: BStmtKind::For { var, var_span, from, to, step, body } }])
    }

    fn cond_clause(&mut self) -> Option<Option<(bool, BExpr)>> {
        if self.eat_kw("WHILE") {
            return Some(Some((false, self.expr()?)));
        }
        if self.eat_kw("UNTIL") {
            return Some(Some((true, self.expr()?)));
        }
        Some(None)
    }

    fn do_stmt(&mut self) -> Option<Vec<BStmt>> {
        let sp = self.bump().span;
        let pre = self.cond_clause()?;
        let body = self.block(&["LOOP"])?;
        self.bump(); // LOOP
        let post = self.cond_clause()?;
        if pre.is_some() && post.is_some() {
            self.err("PYB0033", sp, "DO ... LOOP lleva la condicion al principio o al final, no en las dos");
            return None;
        }
        Some(vec![BStmt { span: sp.to(self.prev_span()), kind: BStmtKind::Do { pre, post, body } }])
    }

    fn while_stmt(&mut self) -> Option<Vec<BStmt>> {
        let sp = self.bump().span;
        let cond = self.expr()?;
        let body = self.block(&["WEND"])?;
        self.bump();
        Some(vec![BStmt { span: sp.to(self.prev_span()), kind: BStmtKind::While { cond, body } }])
    }

    fn select_stmt(&mut self) -> Option<Vec<BStmt>> {
        let sp = self.bump().span;
        if !self.eat_kw("CASE") {
            let s = self.span();
            self.err("PYB0034", s, "se esperaba CASE: SELECT CASE variable");
            return None;
        }
        let subject = self.expr()?;
        let mut cases = Vec::new();
        let mut default = Vec::new();
        self.skip_newlines();
        loop {
            self.skip_newlines();
            if self.close("ENDSELECT") {
                break;
            }
            if !self.eat_kw("CASE") {
                let s = self.span();
                self.err("PYB0034", s, "dentro de SELECT CASE se esperaba CASE o END SELECT");
                return None;
            }
            if self.eat_kw("ELSE") {
                default = self.block(&["CASE", "ENDSELECT"])?;
                continue;
            }
            let mut items = Vec::new();
            loop {
                let a = self.expr()?;
                if self.eat_kw("TO") {
                    let b = self.expr()?;
                    items.push(CaseItem::Range(a, b));
                } else {
                    items.push(CaseItem::Value(a));
                }
                if matches!(self.peek(), T::Comma) {
                    self.bump();
                } else {
                    break;
                }
            }
            let body = self.block(&["CASE", "ENDSELECT"])?;
            cases.push((items, body));
        }
        Some(vec![BStmt { span: sp.to(self.prev_span()), kind: BStmtKind::Select { subject, cases, default } }])
    }
}

/// Palabras reservadas: no pueden ser una etiqueta ni una variable.
fn is_keyword(upper: &str) -> bool {
    matches!(
        upper,
        "ON" | "ASM"
            | "DEVICE"
            | "BAUD"
            | "CLOCK"
            | "PLL"
            | "CLOCKDIVIDER"
            | "CLOCKOUTPUT"
            | "FAILSAFE"
            | "CLOCKSWITCHING"
            | "FROM"
            | "IMPORT"
            | "SYMBOL"
            | "DIM"
            | "AS"
            | "LET"
            | "GOTO"
            | "GOSUB"
            | "RETURN"
            | "EXIT"
            | "END"
            | "IF"
            | "THEN"
            | "ELSE"
            | "ELSEIF"
            | "ENDIF"
            | "FOR"
            | "TO"
            | "STEP"
            | "NEXT"
            | "DO"
            | "LOOP"
            | "WHILE"
            | "WEND"
            | "UNTIL"
            | "SELECT"
            | "CASE"
            | "ENDSELECT"
            | "AND"
            | "OR"
            | "XOR"
            | "NOT"
            | "MOD"
    )
}

/// Los comandos al estilo PICAXE (se escriben como `HIGH B.0`, sin parentesis).
pub fn is_command(name: &str) -> bool {
    matches!(
        name.to_ascii_uppercase().as_str(),
        "HIGH" | "LOW" | "TOGGLE" | "OUTPUT" | "INPUT" | "PAUSE" | "PAUSEUS" | "READADC" | "READADC10" | "READ" | "WRITE" | "PWMOUT" | "SERTXD" | "SERRXD" | "ENABLE" | "DISABLE"
    )
}
