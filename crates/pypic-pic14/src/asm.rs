//! Ensamblador textual PIC14 (PIC16 clasica): texto -> palabras de 14 bits, en dos pasadas.
//!
//! Sintaxis (una sola forma por operacion):
//!   * comentarios con `;`, mnemonicos y directivas sin distinguir mayusculas, simbolos si.
//!   * etiquetas `nombre:`; constantes `NOMBRE EQU valor` (solo simbolos ya definidos).
//!   * `ORG direccion` y `DW valor[, valor...]` (palabras de 14 bits, p. ej. la configuracion en `ORG 0x2007`).
//!   * operandos: numeros decimales, `0x..`, `0b..`, simbolos y `$` (direccion actual) con `+` y `-`.
//!   * instrucciones de byte: `ADDWF f,W` o `ADDWF f,F` (el destino es siempre `W` o `F`, nunca 0/1).
//! Todo corre en el host; el resultado es codigo maquina nativo.

use std::collections::BTreeMap;

use crate::hex::{HexError, Image};
use crate::isa::{Dest, Inst};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AsmError {
    pub line: usize,
    pub col: usize,
    pub code: &'static str,
    pub message: String,
}

/// `archivo:linea:col: error[CODIGO]: mensaje`
pub fn render_errors(errors: &[AsmError], file: &str) -> String {
    errors.iter().map(|e| format!("{file}:{}:{}: error[{}]: {}\n", e.line, e.col, e.code, e.message)).collect()
}

#[derive(Debug)]
pub struct Assembled {
    pub image: Image,
    /// Etiquetas (direccion de palabra) y constantes `EQU`.
    pub symbols: BTreeMap<String, i64>,
    pub listing: String,
}

#[derive(Clone, Debug, PartialEq)]
enum Tok {
    Ident(String),
    Num(i64),
    Comma,
    Plus,
    Minus,
    Dollar,
    Colon,
}

type T = (Tok, usize); // token y su columna (1-based)

const MNEMONICS: &[&str] = &[
    "ADDWF", "ANDWF", "CLRF", "CLRW", "COMF", "DECF", "DECFSZ", "INCF", "INCFSZ", "IORWF", "MOVF", "MOVWF", "NOP", "RLF", "RRF", "SUBWF", "SWAPF", "XORWF", "BCF", "BSF",
    "BTFSC", "BTFSS", "ADDLW", "ANDLW", "CALL", "CLRWDT", "GOTO", "IORLW", "MOVLW", "RETFIE", "RETLW", "RETURN", "SLEEP", "SUBLW", "XORLW",
];
/// Instrucciones que existen en otras familias (PIC16F1xxx, "enhanced mid-range") pero no en el PIC16 clasico.
const ENHANCED_ONLY: &[&str] = &["MOVLB", "MOVLP", "BRA", "BRW", "CALLW", "ADDWFC", "ASRF", "LSLF", "LSRF", "SUBWFB", "MOVIW", "MOVWI", "RESET"];
/// Pseudoinstrucciones de MPASM: no existen; hay que escribir la instruccion real.
const MPASM_PSEUDO: &[&str] = &["MOVFW", "CLRC", "SETC", "CLRZ", "SETZ", "SKPC", "SKPNC", "SKPZ", "SKPNZ", "BANKSEL", "TSTF", "BC", "BNC", "BZ", "BNZ"];
const DIRECTIVES: &[&str] = &["ORG", "DW", "EQU"];

fn err<R>(line: usize, col: usize, code: &'static str, message: impl Into<String>) -> Result<R, AsmError> {
    Err(AsmError { line, col, code, message: message.into() })
}

fn lex(src: &str, line: usize) -> Result<Vec<T>, AsmError> {
    let chars: Vec<(usize, char)> = src.char_indices().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let (pos, c) = chars[i];
        let col = src[..pos].chars().count() + 1;
        match c {
            c if c.is_whitespace() => i += 1,
            ',' => {
                out.push((Tok::Comma, col));
                i += 1;
            }
            '+' => {
                out.push((Tok::Plus, col));
                i += 1;
            }
            '-' => {
                out.push((Tok::Minus, col));
                i += 1;
            }
            '$' => {
                out.push((Tok::Dollar, col));
                i += 1;
            }
            ':' => {
                out.push((Tok::Colon, col));
                i += 1;
            }
            c if c.is_ascii_alphanumeric() || c == '_' => {
                let start = i;
                while i < chars.len() && (chars[i].1.is_ascii_alphanumeric() || chars[i].1 == '_') {
                    i += 1;
                }
                let text: String = chars[start..i].iter().map(|(_, c)| *c).collect();
                if text.chars().next().unwrap().is_ascii_digit() {
                    let v = if let Some(h) = text.strip_prefix("0x").or_else(|| text.strip_prefix("0X")) {
                        i64::from_str_radix(h, 16)
                    } else if let Some(b) = text.strip_prefix("0b").or_else(|| text.strip_prefix("0B")) {
                        i64::from_str_radix(b, 2)
                    } else {
                        text.parse::<i64>()
                    };
                    match v {
                        Ok(v) => out.push((Tok::Num(v), col)),
                        Err(_) => return err(line, col, "ASM001", format!("numero invalido '{text}' (usa decimal, 0x.. o 0b..)")),
                    }
                } else {
                    out.push((Tok::Ident(text), col));
                }
            }
            other => return err(line, col, "ASM001", format!("caracter no valido '{other}'")),
        }
    }
    Ok(out)
}

struct Ctx<'a> {
    syms: &'a BTreeMap<String, i64>,
    pc: u32,
    line: usize,
}

/// Expresion: `[-] termino {(+|-) termino}` con terminos numero, simbolo o `$`.
fn eval(toks: &[T], ctx: &Ctx, at: usize) -> Result<i64, AsmError> {
    if toks.is_empty() {
        return err(ctx.line, at, "ASM003", "falta un valor");
    }
    let mut total: i64 = 0;
    let mut sign = 1i64;
    let mut i = 0;
    if matches!(toks[0].0, Tok::Minus) {
        sign = -1;
        i = 1;
    } else if matches!(toks[0].0, Tok::Plus) {
        i = 1;
    }
    loop {
        let Some((t, col)) = toks.get(i) else {
            return err(ctx.line, toks.last().map_or(at, |t| t.1), "ASM003", "la expresion termina en un operador");
        };
        let v = match t {
            Tok::Num(n) => *n,
            Tok::Dollar => ctx.pc as i64,
            Tok::Ident(name) => match ctx.syms.get(name) {
                Some(v) => *v,
                None => return err(ctx.line, *col, "ASM005", format!("simbolo '{name}' no definido")),
            },
            _ => return err(ctx.line, *col, "ASM003", "se esperaba un numero, un simbolo o '$'"),
        };
        total += sign * v;
        i += 1;
        match toks.get(i) {
            None => return Ok(total),
            Some((Tok::Plus, _)) => sign = 1,
            Some((Tok::Minus, _)) => sign = -1,
            Some((_, col)) => return err(ctx.line, *col, "ASM003", "se esperaba '+', '-' o el fin del operando"),
        }
        i += 1;
    }
}

fn split_operands(toks: &[T]) -> Vec<&[T]> {
    if toks.is_empty() {
        return Vec::new();
    }
    toks.split(|(t, _)| matches!(t, Tok::Comma)).collect()
}

enum Stmt {
    Dw { line: usize, col: usize, pc: u32, ops: Vec<Vec<T>> },
    Inst { line: usize, col: usize, pc: u32, mnemonic: String, ops: Vec<Vec<T>> },
}

fn is_reserved(name: &str) -> bool {
    let u = name.to_ascii_uppercase();
    MNEMONICS.contains(&u.as_str()) || DIRECTIVES.contains(&u.as_str()) || u == "W" || u == "F"
}

/// Ensambla `src`. Devuelve todos los errores encontrados, no solo el primero.
pub fn assemble(src: &str) -> Result<Assembled, Vec<AsmError>> {
    let mut errors: Vec<AsmError> = Vec::new();
    let mut syms: BTreeMap<String, i64> = BTreeMap::new();
    let mut stmts: Vec<Stmt> = Vec::new();
    let lines: Vec<&str> = src.lines().collect();
    let mut pc: u32 = 0;

    // ---- pasada 1: direcciones, etiquetas y constantes -------------------------------------------
    for (n, raw) in lines.iter().enumerate() {
        let line = n + 1;
        let code = raw.split(';').next().unwrap_or("");
        let toks = match lex(code, line) {
            Ok(t) => t,
            Err(e) => {
                errors.push(e);
                continue;
            }
        };
        if toks.is_empty() {
            continue;
        }
        let mut i = 0;
        // etiqueta
        if let (Some((Tok::Ident(name), col)), Some((Tok::Colon, _))) = (toks.first(), toks.get(1)) {
            if is_reserved(name) {
                errors.push(AsmError { line, col: *col, code: "ASM005", message: format!("'{name}' es una palabra reservada y no puede ser una etiqueta") });
            } else if syms.contains_key(name) {
                errors.push(AsmError { line, col: *col, code: "ASM005", message: format!("el simbolo '{name}' ya esta definido") });
            } else {
                syms.insert(name.clone(), pc as i64);
            }
            i = 2;
        }
        let rest = &toks[i..];
        let Some((Tok::Ident(head), hcol)) = rest.first() else {
            if !rest.is_empty() {
                errors.push(AsmError { line, col: rest[0].1, code: "ASM001", message: "se esperaba una instruccion, una directiva o una etiqueta".into() });
            }
            continue;
        };
        // NOMBRE EQU valor
        if matches!(rest.get(1), Some((Tok::Ident(e), _)) if e.eq_ignore_ascii_case("EQU")) {
            let ctx = Ctx { syms: &syms, pc, line };
            if is_reserved(head) {
                errors.push(AsmError { line, col: *hcol, code: "ASM005", message: format!("'{head}' es una palabra reservada y no puede ser una constante") });
            } else if syms.contains_key(head) {
                errors.push(AsmError { line, col: *hcol, code: "ASM005", message: format!("el simbolo '{head}' ya esta definido") });
            } else {
                match eval(&rest[2..], &ctx, rest[1].1) {
                    Ok(v) => {
                        syms.insert(head.clone(), v);
                    }
                    Err(e) => errors.push(e),
                }
            }
            continue;
        }
        let upper = head.to_ascii_uppercase();
        let ops: Vec<Vec<T>> = split_operands(&rest[1..]).into_iter().map(|s| s.to_vec()).collect();
        match upper.as_str() {
            "ORG" => {
                let ctx = Ctx { syms: &syms, pc, line };
                if ops.len() != 1 {
                    errors.push(AsmError { line, col: *hcol, code: "ASM003", message: "ORG recibe una direccion".into() });
                    continue;
                }
                match eval(&ops[0], &ctx, *hcol) {
                    Ok(v) if (0..0x4000).contains(&v) => {
                        pc = v as u32;
                    }
                    Ok(v) => errors.push(AsmError { line, col: *hcol, code: "ASM006", message: format!("ORG {v:#x} fuera del espacio de direcciones (0..=0x3FFF)") }),
                    Err(e) => errors.push(e),
                }
            }
            "DW" => {
                if ops.is_empty() {
                    errors.push(AsmError { line, col: *hcol, code: "ASM003", message: "DW necesita al menos un valor".into() });
                    continue;
                }
                let n = ops.len() as u32;
                stmts.push(Stmt::Dw { line, col: *hcol, pc, ops });
                pc += n;
            }
            m if MNEMONICS.contains(&m) => {
                stmts.push(Stmt::Inst { line, col: *hcol, pc, mnemonic: m.to_string(), ops });
                pc += 1;
            }
            m if ENHANCED_ONLY.contains(&m) => errors.push(AsmError {
                line,
                col: *hcol,
                code: "ASM002",
                message: format!("'{m}' no existe en el PIC16 clasico (PIC14): es de otra familia (PIC16F1xxx, 'enhanced mid-range'); las 35 instrucciones de este dispositivo estan en docs/asm-pic14.md"),
            }),
            m if MPASM_PSEUDO.contains(&m) => errors.push(AsmError {
                line,
                col: *hcol,
                code: "ASM002",
                message: format!("'{m}' es una pseudoinstruccion de MPASM y no existe: escribe la instruccion real (p. ej. BSF STATUS,C en vez de SETC; el banco se cambia con BSF/BCF sobre RP0/RP1)"),
            }),
            _ => errors.push(AsmError { line, col: *hcol, code: "ASM002", message: format!("instruccion o directiva desconocida '{head}'") }),
        }
    }

    // ---- pasada 2: codificacion ---------------------------------------------------------------
    let mut image = Image::new();
    let mut emitted: BTreeMap<usize, Vec<(u32, u16)>> = BTreeMap::new();
    for st in &stmts {
        match st {
            Stmt::Dw { line, col, pc, ops } => {
                for (k, op) in ops.iter().enumerate() {
                    let addr = pc + k as u32;
                    let ctx = Ctx { syms: &syms, pc: addr, line: *line };
                    match eval(op, &ctx, *col) {
                        Ok(v) if (0..=0x3FFF).contains(&v) => place(&mut image, &mut emitted, &mut errors, *line, *col, addr, v as u16),
                        Ok(v) => errors.push(AsmError { line: *line, col: op.first().map_or(*col, |t| t.1), code: "ASM004", message: format!("DW {v}: una palabra tiene 14 bits (0..=0x3FFF)") }),
                        Err(e) => errors.push(e),
                    }
                }
            }
            Stmt::Inst { line, col, pc, mnemonic, ops } => {
                let ctx = Ctx { syms: &syms, pc: *pc, line: *line };
                match build(mnemonic, ops, &ctx, *col) {
                    Ok(inst) => match inst.encode() {
                        Ok(w) => place(&mut image, &mut emitted, &mut errors, *line, *col, *pc, w),
                        Err(e) => errors.push(AsmError { line: *line, col: *col, code: "ASM004", message: e.to_string() }),
                    },
                    Err(e) => errors.push(e),
                }
            }
        }
    }
    if !errors.is_empty() {
        errors.sort_by_key(|e| (e.line, e.col));
        return Err(errors);
    }

    let mut listing = String::new();
    for (n, raw) in lines.iter().enumerate() {
        match emitted.get(&(n + 1)) {
            Some(words) => {
                for (k, (a, w)) in words.iter().enumerate() {
                    if k == 0 {
                        listing.push_str(&format!("{a:04X} {w:04X}  {raw}\n"));
                    } else {
                        listing.push_str(&format!("{a:04X} {w:04X}\n"));
                    }
                }
            }
            None => listing.push_str(&format!("           {raw}\n")),
        }
    }
    Ok(Assembled { image, symbols: syms, listing })
}

fn place(image: &mut Image, emitted: &mut BTreeMap<usize, Vec<(u32, u16)>>, errors: &mut Vec<AsmError>, line: usize, col: usize, addr: u32, word: u16) {
    // dos sentencias en la misma direccion son siempre un error, aunque escriban el mismo valor
    if image.get(addr).is_some() {
        errors.push(AsmError { line, col, code: "ASM006", message: format!("la direccion {addr:#06x} ya esta ocupada por otra instruccion o dato") });
        return;
    }
    match image.set(addr, word) {
        Ok(()) => emitted.entry(line).or_default().push((addr, word)),
        Err(HexError::Conflict { .. }) => errors.push(AsmError { line, col, code: "ASM006", message: format!("la direccion {addr:#06x} ya esta ocupada por otra instruccion o dato") }),
        Err(e) => errors.push(AsmError { line, col, code: "ASM004", message: e.to_string() }),
    }
}

enum Shape {
    Fd,
    F,
    Fb,
    K,
    Addr,
    None,
}

fn shape(m: &str) -> Shape {
    match m {
        "ADDWF" | "ANDWF" | "COMF" | "DECF" | "DECFSZ" | "INCF" | "INCFSZ" | "IORWF" | "MOVF" | "RLF" | "RRF" | "SUBWF" | "SWAPF" | "XORWF" => Shape::Fd,
        "CLRF" | "MOVWF" => Shape::F,
        "BCF" | "BSF" | "BTFSC" | "BTFSS" => Shape::Fb,
        "ADDLW" | "ANDLW" | "IORLW" | "MOVLW" | "RETLW" | "SUBLW" | "XORLW" => Shape::K,
        "CALL" | "GOTO" => Shape::Addr,
        _ => Shape::None,
    }
}

fn file_reg(v: i64, line: usize, col: usize) -> Result<u8, AsmError> {
    if (0..=0x7F).contains(&v) {
        Ok(v as u8)
    } else {
        err(
            line,
            col,
            "ASM004",
            format!("registro {v:#x} fuera de 0..=0x7F: el operando f son 7 bits; para otro banco selecciona RP0/RP1 con BSF/BCF y usa el desplazamiento dentro del banco (p. ej. 0x188 -> 0x08 en el banco 3)"),
        )
    }
}

fn dest(op: &[T], line: usize, col: usize) -> Result<Dest, AsmError> {
    match op {
        [(Tok::Ident(n), _)] if n.eq_ignore_ascii_case("W") => Ok(Dest::W),
        [(Tok::Ident(n), _)] if n.eq_ignore_ascii_case("F") => Ok(Dest::F),
        [(Tok::Num(v @ (0 | 1)), c)] => err(line, *c, "ASM003", format!("el destino es W o F, no {v}: una sola forma (usa {})", if *v == 0 { "W" } else { "F" })),
        _ => err(line, op.first().map_or(col, |t| t.1), "ASM003", "el destino debe ser W o F"),
    }
}

fn build(m: &str, ops: &[Vec<T>], ctx: &Ctx, col: usize) -> Result<Inst, AsmError> {
    use Inst::*;
    let line = ctx.line;
    let want = |n: usize, form: &str| -> Result<(), AsmError> {
        if ops.len() == n {
            Ok(())
        } else {
            err(line, col, "ASM003", format!("{m} se escribe '{form}' ({} operando(s) en lugar de {n})", ops.len()))
        }
    };
    let val = |k: usize| eval(&ops[k], ctx, col);
    let opcol = |k: usize| ops[k].first().map_or(col, |t| t.1);
    Ok(match shape(m) {
        Shape::None => {
            want(0, m)?;
            match m {
                "CLRW" => Clrw,
                "CLRWDT" => Clrwdt,
                "NOP" => Nop,
                "RETFIE" => Retfie,
                "RETURN" => Return,
                _ => Sleep,
            }
        }
        Shape::F => {
            want(1, &format!("{m} f"))?;
            let f = file_reg(val(0)?, line, opcol(0))?;
            if m == "CLRF" { Clrf(f) } else { Movwf(f) }
        }
        Shape::Fd => {
            if ops.len() == 1 {
                return err(line, col, "ASM003", format!("falta el destino: escribe '{m} f,W' o '{m} f,F'"));
            }
            want(2, &format!("{m} f,W|F"))?;
            let f = file_reg(val(0)?, line, opcol(0))?;
            let d = dest(&ops[1], line, col)?;
            match m {
                "ADDWF" => Addwf(f, d),
                "ANDWF" => Andwf(f, d),
                "COMF" => Comf(f, d),
                "DECF" => Decf(f, d),
                "DECFSZ" => Decfsz(f, d),
                "INCF" => Incf(f, d),
                "INCFSZ" => Incfsz(f, d),
                "IORWF" => Iorwf(f, d),
                "MOVF" => Movf(f, d),
                "RLF" => Rlf(f, d),
                "RRF" => Rrf(f, d),
                "SUBWF" => Subwf(f, d),
                "SWAPF" => Swapf(f, d),
                _ => Xorwf(f, d),
            }
        }
        Shape::Fb => {
            want(2, &format!("{m} f,bit"))?;
            let f = file_reg(val(0)?, line, opcol(0))?;
            let b = val(1)?;
            if !(0..=7).contains(&b) {
                return err(line, opcol(1), "ASM004", format!("bit {b} fuera de 0..=7"));
            }
            let b = b as u8;
            match m {
                "BCF" => Bcf(f, b),
                "BSF" => Bsf(f, b),
                "BTFSC" => Btfsc(f, b),
                _ => Btfss(f, b),
            }
        }
        Shape::K => {
            want(1, &format!("{m} k"))?;
            let k = val(0)?;
            if !(0..=255).contains(&k) {
                return err(line, opcol(0), "ASM004", format!("literal {k} fuera de 0..=255"));
            }
            let k = k as u8;
            match m {
                "ADDLW" => Addlw(k),
                "ANDLW" => Andlw(k),
                "IORLW" => Iorlw(k),
                "MOVLW" => Movlw(k),
                "RETLW" => Retlw(k),
                "SUBLW" => Sublw(k),
                _ => Xorlw(k),
            }
        }
        Shape::Addr => {
            want(1, &format!("{m} etiqueta"))?;
            let a = val(0)?;
            if !(0..=0x7FF).contains(&a) {
                return err(
                    line,
                    opcol(0),
                    "ASM006",
                    format!("{m} {a:#x} fuera de la pagina 0 (0..=0x7FF): CALL/GOTO llevan 11 bits y el ensamblador no carga PCLATH para otras paginas (#31)"),
                );
            }
            if m == "CALL" { Call(a as u16) } else { Goto(a as u16) }
        }
    })
}
