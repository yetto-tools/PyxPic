//! Ensamblador PIC14 (PIC16 clasica) con la sintaxis oficial de MPLAB XC8 PIC Assembler (`pic-as`).
//!
//! Fuente: MPLAB XC8 PIC Assembler User's Guide, DS50002974F, cap. 4 y 5. Lo que implementa:
//!   * sentencias `etiqueta: mnemonico operandos ; comentario`; mnemonicos y directivas sin distinguir
//!     mayusculas, simbolos si; comentarios `;`, `//` y `/* */`.
//!   * numeros decimales por defecto, `0x1F`, `1Fh`, `101B`, `17o`, `'A'`; expresiones evaluadas de
//!     izquierda a derecha (sin precedencia, como pic-as) con operadores simbolicos y en palabra.
//!   * destino `,w`/`,f` o `,0`/`,1`; si se omite es `f`.
//!   * directivas `PROCESSOR PSECT ORG DB DW DS EQU SET GLOBAL PUBLIC EXTRN END CONFIG ALIGN BANKSEL BANKISEL
//!     PAGESEL IF/ELSIF/ELSE/ENDIF`, mas `fcall` y `ljmp`; `#include <xc.inc>`, `#define`, `#ifdef`...
//!   * un mini-enlazador: los psects absolutos van en la direccion 0 + `ORG`, los demas se colocan por
//!     `-pNOMBRE=dir` o, sin eso, dentro del rango de su clase (`CODE`, `BANK0`...).
//!
//! Todo corre en el host; el resultado es codigo maquina nativo.

use std::collections::{BTreeMap, BTreeSet};

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
    /// Advertencias que no impiden generar la imagen.
    pub warnings: Vec<AsmError>,
    /// Etiquetas (direccion final) y constantes `EQU`/`SET`.
    pub symbols: BTreeMap<String, i64>,
    pub listing: String,
    /// (psect, direccion base, tamano en unidades de direccion) tras el enlace.
    pub psects: Vec<(String, u32, u32)>,
}

// ---------------------------------------------------------------- entorno (lo que da el dispositivo)

/// Un registro tal como lo expone `xc.inc`.
#[derive(Clone, Debug)]
pub struct EnvReg {
    pub name: String,
    /// Direccion completa de 9 bits.
    pub addr: u32,
    pub bits: Vec<Option<String>>,
}

/// Un campo de configuracion: valor numerico por nombre (`ON`, `OFF`...).
#[derive(Clone, Debug)]
pub struct EnvConfigField {
    pub name: String,
    pub lsb: u32,
    pub width: u32,
    pub names: BTreeMap<String, u64>,
}

#[derive(Clone, Debug)]
pub struct EnvConfigWord {
    pub name: String,
    pub address: u32,
    pub default: u16,
    pub fields: Vec<EnvConfigField>,
}

/// Lo que el ensamblador sabe del dispositivo (viene del Device Pack) y de la linea de ordenes.
#[derive(Clone, Debug)]
pub struct AsmEnv {
    pub device: Option<String>,
    pub regs: Vec<EnvReg>,
    pub config: Vec<EnvConfigWord>,
    pub id_locations: Vec<pyxpic_device::IdLocation>,
    /// Palabras de programa (los psects de la clase `CODE` se colocan dentro de ellas).
    pub program_words: u32,
    /// Clases de memoria de datos (`BANK0`.., `COMMON`, `RAM`): rangos inclusivos.
    pub data_classes: BTreeMap<String, Vec<(u32, u32)>>,
    /// Opciones `-pNOMBRE=dir[,siguiente...]` del enlazador: psect, direccion y psects que le siguen.
    pub placements: Vec<(String, u32, Vec<String>)>,
    /// `--fixupoverflow=ignore`: los operandos se truncan en vez de dar error.
    pub fixup_ignore: bool,
}

impl Default for AsmEnv {
    fn default() -> Self {
        AsmEnv { device: None, regs: Vec::new(), config: Vec::new(), id_locations: Vec::new(), program_words: 0x2000, data_classes: BTreeMap::new(), placements: Vec::new(), fixup_ignore: false }
    }
}

impl AsmEnv {
    pub fn from_pack(pack: &pyxpic_device::DevicePack) -> AsmEnv {
        let regs = pack.registers.registers.iter().filter(|r| r.physical).map(|r| EnvReg { name: r.name.clone(), addr: r.addresses[0].0, bits: r.bits.clone() }).collect();
        let config = pack
            .config
            .words
            .iter()
            .map(|w| EnvConfigWord {
                name: w.name.clone(),
                address: w.address.0,
                default: w.default.0 as u16,
                fields: w
                    .fields
                    .iter()
                    .map(|f| EnvConfigField { name: f.name.clone(), lsb: f.lsb, width: f.width, names: f.names.iter().filter_map(|(k, v)| v.parse().ok().map(|v| (k.clone(), v))).collect() })
                    .collect(),
            })
            .collect();
        // BANKn: la RAM de proposito general de cada banco sin la zona comun (que es la clase COMMON)
        let m = &pack.memory.data;
        let mut classes: BTreeMap<String, Vec<(u32, u32)>> = BTreeMap::new();
        let mut all = Vec::new();
        for (n, r) in m.gpr.iter().enumerate() {
            let (lo, mut hi) = (r.start.0, r.end.0);
            if (lo..=hi).contains(&m.common.start.0) {
                hi = m.common.start.0 - 1;
            }
            if lo <= hi {
                classes.insert(format!("BANK{n}"), vec![(lo, hi)]);
                all.push((lo, hi));
            }
        }
        classes.insert("COMMON".into(), vec![(m.common.start.0, m.common.end.0)]);
        all.push((m.common.start.0, m.common.end.0));
        all.sort();
        classes.insert("RAM".into(), all);
        AsmEnv {
            device: Some(pack.device.name.clone()),
            regs,
            config,
            id_locations: pack.config.id_locations.clone(),
            program_words: pack.memory.program.words,
            data_classes: classes,
            ..AsmEnv::default()
        }
    }

    /// Interpreta las opciones del enlazador (`-presetVec=0h`, `-pintentry=4h,init`...) y las anade a `placements`.
    pub fn add_link_options(&mut self, opts: &[String]) -> Result<(), String> {
        for o in opts {
            if o == "--fixupoverflow=ignore" || o == "--fixupoverflow=warn" || o == "--fixupoverflow=lstwarn" {
                self.fixup_ignore = true;
                continue;
            }
            let body = o.strip_prefix("-p").or_else(|| o.strip_prefix("-P")).ok_or_else(|| format!("opcion del enlazador no soportada: '{o}' (se admiten -pNOMBRE=dir y --fixupoverflow=ignore)"))?;
            let mut parts = body.split(',');
            let first = parts.next().unwrap_or("");
            let (name, addr) = first.split_once('=').ok_or_else(|| format!("'{o}': se esperaba -pNOMBRE=direccion"))?;
            let addr = parse_number(addr).map_err(|e| format!("'{o}': {e}"))?;
            self.placements.push((name.to_string(), addr as u32, parts.map(str::to_string).collect()));
        }
        Ok(())
    }
}

// ---------------------------------------------------------------- lexer

#[derive(Clone, Debug, PartialEq)]
enum Tok {
    Ident(String),
    Num(i64),
    Str(String),
    Comma,
    Colon,
    Plus,
    Minus,
    Star,
    Slash,
    Amp,
    Pipe,
    Caret,
    Tilde,
    Shl,
    Shr,
    Lt,
    Gt,
    Le,
    Ge,
    Ne,
    Eq,
    LParen,
    RParen,
    Dollar,
}

type T = (Tok, usize); // token y su columna (1-based)

const MNEMONICS: &[&str] = &[
    "ADDWF", "ANDWF", "CLRF", "CLRW", "COMF", "DECF", "DECFSZ", "INCF", "INCFSZ", "IORWF", "MOVF", "MOVWF", "NOP", "RLF", "RRF", "SUBWF", "SWAPF", "XORWF", "BCF", "BSF", "BTFSC", "BTFSS", "ADDLW",
    "ANDLW", "CALL", "CLRWDT", "GOTO", "IORLW", "MOVLW", "RETFIE", "RETLW", "RETURN", "SLEEP", "SUBLW", "XORLW",
];
/// Instrucciones que existen en otras familias (PIC16F1xxx, "enhanced mid-range") pero no en el PIC16 clasico.
const ENHANCED_ONLY: &[&str] = &["MOVLB", "MOVLP", "BRA", "BRW", "CALLW", "ADDWFC", "ASRF", "LSLF", "LSRF", "SUBWFB", "MOVIW", "MOVWI", "RESET"];
/// Pseudoinstrucciones de MPASM que PIC-AS no implementa.
const MPASM_PSEUDO: &[&str] = &["MOVFW", "CLRC", "SETC", "CLRZ", "SETZ", "SKPC", "SKPNC", "SKPZ", "SKPNZ", "TSTF", "BC", "BNC", "BZ", "BNZ", "NEGF", "ADDCF", "SUBCF", "LCALL", "LGOTO"];
/// Directivas de pic-as que no cambian el codigo y se aceptan sin efecto.
const IGNORED_DIRECTIVES: &[&str] = &[
    "LIST",
    "NOLIST",
    "TITLE",
    "SUBTITLE",
    "PAGELEN",
    "PAGEWIDTH",
    "EXPAND",
    "NOEXPAND",
    "COND",
    "NOCOND",
    "ASMOPT",
    "LINE",
    "FILE",
    "DEBUG_SOURCE",
    "CALLSTACK",
    "FNCALL",
    "FNROOT",
    "FNARG",
    "FNBREAK",
    "FNCONF",
    "FNINDIR",
    "FNSIZE",
    "FNADDR",
    "SIGNAT",
    "ERRORLEVEL",
    "RADIX_NOP",
];
/// Directivas de pic-as que el ensamblador todavia no implementa.
const UNSUPPORTED_DIRECTIVES: &[&str] = &["DABS", "DLABS", "DDW", "DQW"];
/// Directivas de MPASM que PIC-AS no tiene: (nombre, como se escribe en PIC-AS).
const MPASM_DIRECTIVES: &[(&str, &str)] = &[
    ("__CONFIG", "CONFIG nombre = valor (p. ej. CONFIG WDTE = OFF)"),
    ("CBLOCK", "un PSECT de datos con etiquetas y DS (p. ej. PSECT udata_bank0 / x: DS 1)"),
    ("ENDC", "un PSECT de datos con etiquetas y DS"),
    ("RES", "DS n dentro de un PSECT de datos"),
    ("UDATA", "PSECT udata_bank0 (o udata, udata_shr...)"),
    ("UDATA_SHR", "PSECT udata_shr"),
    ("IDATA", "PSECT con DB/DW en la memoria de programa"),
    ("CODE", "PSECT code,class=CODE,delta=2"),
    ("DATA", "DW o DB dentro de un PSECT de programa"),
    ("DE", "PSECT edata (EEPROM de datos)"),
    ("DT", "una serie de RETLW"),
    ("ORG", "ORG"),
    ("BANKSEL", "BANKSEL"),
];

fn err<R>(line: usize, col: usize, code: &'static str, message: impl Into<String>) -> Result<R, AsmError> {
    Err(AsmError { line, col, code, message: message.into() })
}

/// Numero de pic-as: decimal por defecto; prefijo `0x` o sufijo `h` (hexadecimal), `B` (binario, en mayuscula),
/// `o`/`q` (octal), `d` (decimal explicito).
pub fn parse_number(text: &str) -> Result<i64, String> {
    let bad = || format!("numero invalido '{text}' (decimal, 0x1F o 1Fh, 101B, 17o)");
    let lower = text.to_ascii_lowercase();
    let r = if let Some(h) = lower.strip_prefix("0x") {
        i64::from_str_radix(h, 16)
    } else if let Some(h) = lower.strip_suffix('h') {
        i64::from_str_radix(h, 16)
    } else if let Some(b) = text.strip_suffix('B') {
        i64::from_str_radix(b, 2)
    } else if let Some(o) = lower.strip_suffix('o').or_else(|| lower.strip_suffix('q')) {
        i64::from_str_radix(o, 8)
    } else if let Some(d) = lower.strip_suffix('d') {
        d.parse::<i64>()
    } else {
        text.parse::<i64>()
    };
    match r {
        Ok(v) if v <= u32::MAX as i64 => Ok(v),
        Ok(_) => Err(format!("el numero '{text}' no cabe en 32 bits")),
        Err(_) if text.len() > 2 && text[..2].eq_ignore_ascii_case("0b") => Err(format!("{} (el binario se escribe con sufijo B: {}B)", bad(), &text[2..])),
        Err(_) => Err(bad()),
    }
}

fn is_ident_start(c: char) -> bool {
    c.is_ascii_alphabetic() || c == '_' || c == '?'
}

fn is_ident_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_' || c == '?' || c == '$'
}

fn lex(src: &str, line: usize) -> Result<Vec<T>, AsmError> {
    lex_radix(src, line, 10)
}

/// Numero sin sufijo ni prefijo en la base de `RADIX` (hexadecimal u octal); con sufijo o prefijo manda el sufijo.
fn parse_number_radix(text: &str, radix: u32) -> Result<i64, String> {
    if radix != 10 && text.chars().all(|c| c.is_digit(radix)) {
        return i64::from_str_radix(text, radix).map_err(|_| format!("numero invalido '{text}' en base {radix}"));
    }
    parse_number(text)
}

fn lex_radix(src: &str, line: usize, radix: u32) -> Result<Vec<T>, AsmError> {
    let chars: Vec<(usize, char)> = src.char_indices().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let (pos, c) = chars[i];
        let col = src[..pos].chars().count() + 1;
        let next = chars.get(i + 1).map(|x| x.1);
        let simple = |t: Tok, n: usize, i: &mut usize, out: &mut Vec<T>| {
            out.push((t, col));
            *i += n;
        };
        match c {
            c if c.is_whitespace() => i += 1,
            ';' => break,
            ',' => simple(Tok::Comma, 1, &mut i, &mut out),
            ':' => simple(Tok::Colon, 1, &mut i, &mut out),
            '+' => simple(Tok::Plus, 1, &mut i, &mut out),
            '-' => simple(Tok::Minus, 1, &mut i, &mut out),
            '*' => simple(Tok::Star, 1, &mut i, &mut out),
            '/' => simple(Tok::Slash, 1, &mut i, &mut out),
            '&' => simple(Tok::Amp, 1, &mut i, &mut out),
            '|' => simple(Tok::Pipe, 1, &mut i, &mut out),
            '^' => simple(Tok::Caret, 1, &mut i, &mut out),
            '~' => simple(Tok::Tilde, 1, &mut i, &mut out),
            '(' => simple(Tok::LParen, 1, &mut i, &mut out),
            ')' => simple(Tok::RParen, 1, &mut i, &mut out),
            '=' => simple(Tok::Eq, 1, &mut i, &mut out),
            '<' if next == Some('<') => simple(Tok::Shl, 2, &mut i, &mut out),
            '<' if next == Some('=') => simple(Tok::Le, 2, &mut i, &mut out),
            '<' if next == Some('>') => simple(Tok::Ne, 2, &mut i, &mut out),
            '<' => simple(Tok::Lt, 1, &mut i, &mut out),
            '>' if next == Some('>') => simple(Tok::Shr, 2, &mut i, &mut out),
            '>' if next == Some('=') => simple(Tok::Ge, 2, &mut i, &mut out),
            '>' => simple(Tok::Gt, 1, &mut i, &mut out),
            '$' => simple(Tok::Dollar, 1, &mut i, &mut out),
            '\'' | '"' => {
                let q = c;
                let mut j = i + 1;
                let mut s = String::new();
                loop {
                    match chars.get(j) {
                        None => return err(line, col, "ASM001", "cadena sin cerrar"),
                        Some((_, ch)) if *ch == q => break,
                        Some((_, ch)) => s.push(*ch),
                    }
                    j += 1;
                }
                i = j + 1;
                // una constante de caracter es un solo caracter entre comillas simples
                if q == '\'' && s.chars().count() == 1 {
                    out.push((Tok::Num(s.chars().next().unwrap() as i64), col));
                } else {
                    out.push((Tok::Str(s), col));
                }
            }
            c if c.is_ascii_digit() => {
                let start = i;
                while i < chars.len() && chars[i].1.is_ascii_alphanumeric() {
                    i += 1;
                }
                let text: String = chars[start..i].iter().map(|(_, c)| *c).collect();
                match parse_number_radix(&text, radix) {
                    Ok(v) => out.push((Tok::Num(v), col)),
                    Err(m) => return err(line, col, "ASM001", m),
                }
            }
            c if is_ident_start(c) => {
                let start = i;
                while i < chars.len() && is_ident_char(chars[i].1) {
                    i += 1;
                }
                out.push((Tok::Ident(chars[start..i].iter().map(|(_, c)| *c).collect()), col));
            }
            other => return err(line, col, "ASM001", format!("caracter no valido '{other}'")),
        }
    }
    Ok(out)
}

// ---------------------------------------------------------------- expresiones

struct Ctx<'a> {
    syms: &'a BTreeMap<String, i64>,
    /// Valor de `$`; `None` donde la direccion aun no se conoce.
    pc: Option<u32>,
    line: usize,
}

struct ExprParser<'a, 'b> {
    toks: &'a [T],
    i: usize,
    ctx: &'a Ctx<'b>,
    at: usize,
}

impl ExprParser<'_, '_> {
    fn col(&self) -> usize {
        self.toks.get(self.i).map_or(self.at, |t| t.1)
    }

    fn fail<R>(&self, code: &'static str, msg: impl Into<String>) -> Result<R, AsmError> {
        err(self.ctx.line, self.col(), code, msg)
    }

    /// pic-as evalua de izquierda a derecha: `5 + 1 * 2` vale 12; los parentesis cambian el orden.
    fn expr(&mut self) -> Result<i64, AsmError> {
        let mut v = self.unary()?;
        loop {
            let Some((t, _)) = self.toks.get(self.i) else { return Ok(v) };
            let op = match t {
                Tok::Plus => "+",
                Tok::Minus => "-",
                Tok::Star => "*",
                Tok::Slash => "/",
                Tok::Amp => "&",
                Tok::Pipe => "|",
                Tok::Caret => "^",
                Tok::Shl => "<<",
                Tok::Shr => ">>",
                Tok::Lt => "<",
                Tok::Gt => ">",
                Tok::Le => "<=",
                Tok::Ge => ">=",
                Tok::Ne => "<>",
                Tok::Eq => "=",
                Tok::Ident(n) => match n.to_ascii_lowercase().as_str() {
                    "mod" => "mod",
                    "and" => "&",
                    "shl" => "<<",
                    "shr" => ">>",
                    "eq" => "=",
                    "ne" => "<>",
                    "gt" => ">",
                    "ge" => ">=",
                    "lt" => "<",
                    "le" => "<=",
                    _ => return Ok(v),
                },
                _ => return Ok(v),
            };
            let opcol = self.col();
            self.i += 1;
            let r = self.unary()?;
            v = match op {
                "+" => v.wrapping_add(r),
                "-" => v.wrapping_sub(r),
                "*" => v.wrapping_mul(r),
                "/" | "mod" if r == 0 => return err(self.ctx.line, opcol, "ASM004", "division por cero"),
                "/" => v / r,
                "mod" => v % r,
                "&" => v & r,
                "|" => v | r,
                "^" => v ^ r,
                "<<" => v.checked_shl(r as u32).unwrap_or(0),
                ">>" => v.checked_shr(r as u32).unwrap_or(0),
                "<" => (v < r) as i64,
                ">" => (v > r) as i64,
                "<=" => (v <= r) as i64,
                ">=" => (v >= r) as i64,
                "<>" => (v != r) as i64,
                _ => (v == r) as i64,
            };
        }
    }

    fn unary(&mut self) -> Result<i64, AsmError> {
        let Some((t, col)) = self.toks.get(self.i).cloned() else {
            return err(self.ctx.line, self.toks.last().map_or(self.at, |t| t.1), "ASM003", "la expresion termina en un operador");
        };
        self.i += 1;
        match t {
            Tok::Minus => Ok(self.unary()?.wrapping_neg()),
            Tok::Plus => self.unary(),
            Tok::Tilde => Ok(!self.unary()?),
            Tok::Num(n) => Ok(n),
            Tok::Dollar => match self.ctx.pc {
                Some(pc) => Ok(pc as i64),
                None => err(self.ctx.line, col, "ASM003", "'$' no se puede usar aqui: la direccion se conoce solo despues del enlace"),
            },
            Tok::LParen => {
                let v = self.expr()?;
                match self.toks.get(self.i) {
                    Some((Tok::RParen, _)) => {
                        self.i += 1;
                        Ok(v)
                    }
                    _ => self.fail("ASM003", "falta ')'"),
                }
            }
            Tok::Ident(name) => match name.to_ascii_lowercase().as_str() {
                "not" => Ok(!self.unary()?),
                "low" => Ok(self.unary()? & 0xFF),
                "high" => Ok((self.unary()? >> 8) & 0xFF),
                "highword" => Ok((self.unary()? >> 16) & 0xFFFF),
                // mascaras de direccion de xc.inc: dejan solo los bits que caben en el operando de la instruccion
                "bankmask" => Ok(self.unary()? & 0x7F),
                "pagemask" => Ok(self.unary()? & 0x7FF),
                _ => match self.ctx.syms.get(&name) {
                    Some(v) => Ok(*v),
                    None => err(self.ctx.line, col, "ASM005", format!("simbolo '{name}' no definido")),
                },
            },
            _ => err(self.ctx.line, col, "ASM003", "se esperaba un numero, un simbolo, '$' o '('"),
        }
    }
}

fn eval(toks: &[T], ctx: &Ctx, at: usize) -> Result<i64, AsmError> {
    if toks.is_empty() {
        return err(ctx.line, at, "ASM003", "falta un valor");
    }
    let mut p = ExprParser { toks, i: 0, ctx, at };
    let v = p.expr()?;
    if p.i < toks.len() {
        return err(ctx.line, toks[p.i].1, "ASM003", "se esperaba un operador o el fin del operando");
    }
    Ok(v)
}

fn split_operands(toks: &[T]) -> Vec<&[T]> {
    if toks.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    let mut depth = 0i32;
    let mut start = 0;
    for (i, (t, _)) in toks.iter().enumerate() {
        match t {
            Tok::LParen => depth += 1,
            Tok::RParen => depth -= 1,
            Tok::Comma if depth == 0 => {
                out.push(&toks[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    out.push(&toks[start..]);
    out
}

fn is_reserved(name: &str) -> bool {
    let u = name.to_ascii_uppercase();
    MNEMONICS.contains(&u.as_str())
        || [
            "W",
            "F",
            "PSECT",
            "ORG",
            "DB",
            "DW",
            "DS",
            "EQU",
            "SET",
            "GLOBAL",
            "PUBLIC",
            "EXTRN",
            "END",
            "CONFIG",
            "BANKSEL",
            "BANKISEL",
            "PAGESEL",
            "ALIGN",
            "FCALL",
            "LJMP",
            "PROCESSOR",
            "INCLUDE",
            "MACRO",
            "ENDM",
            "REPT",
            "IRP",
            "IRPC",
            "LOCAL",
            "RADIX",
            "IF",
            "ELSIF",
            "ELSE",
            "ENDIF",
            "NOT",
            "LOW",
            "HIGH",
            "HIGHWORD",
            "MOD",
            "AND",
            "SHL",
            "SHR",
            "EQ",
            "NE",
            "GT",
            "GE",
            "LT",
            "LE",
        ]
        .contains(&u.as_str())
}

// ---------------------------------------------------------------- preprocesador

/// Quita comentarios `//`, `/* */` (con estado entre lineas) y `;`, respetando las comillas.
fn strip_comments(raw: &str, in_block: &mut bool) -> String {
    let b: Vec<char> = raw.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    let mut quote: Option<char> = None;
    while i < b.len() {
        let c = b[i];
        if *in_block {
            if c == '*' && b.get(i + 1) == Some(&'/') {
                *in_block = false;
                i += 2;
                out.push(' ');
            } else {
                i += 1;
            }
            continue;
        }
        if let Some(q) = quote {
            out.push(c);
            if c == q {
                quote = None;
            }
            i += 1;
            continue;
        }
        match c {
            '\'' | '"' => {
                quote = Some(c);
                out.push(c);
            }
            ';' => break,
            '/' if b.get(i + 1) == Some(&'/') => break,
            '/' if b.get(i + 1) == Some(&'*') => {
                *in_block = true;
                i += 1;
            }
            _ => out.push(c),
        }
        i += 1;
    }
    out
}

/// Lo que define `<xc.inc>`: simbolos (cada registro vale su direccion completa y cada campo tiene
/// `REG_CAMPO_POSN/POSITION/SIZE/LENGTH/MASK`) y macros de campo (`RB5` -> `PORTB,5`; solo si el campo es unico,
/// de mas de una letra y no coincide con un registro).
pub fn xc_inc(regs: &[EnvReg]) -> (BTreeMap<String, i64>, BTreeMap<String, String>) {
    let mut syms = BTreeMap::new();
    let mut macros = BTreeMap::new();
    let mut count: BTreeMap<&str, usize> = BTreeMap::new();
    for r in regs {
        for b in r.bits.iter().flatten() {
            *count.entry(b.as_str()).or_default() += 1;
        }
    }
    let regnames: BTreeSet<&str> = regs.iter().map(|r| r.name.as_str()).collect();
    for r in regs {
        syms.insert(r.name.clone(), r.addr as i64);
        for (i, b) in r.bits.iter().enumerate() {
            let Some(b) = b else { continue };
            for (suffix, v) in [("POSN", i as i64), ("POSITION", i as i64), ("SIZE", 1), ("LENGTH", 1), ("MASK", 1 << i)] {
                syms.insert(format!("{}_{b}_{suffix}", r.name), v);
            }
            if count[b.as_str()] == 1 && b.len() > 1 && !regnames.contains(b.as_str()) && !is_reserved(b) {
                macros.insert(b.clone(), format!("{},{i}", r.name));
            }
        }
    }
    (syms, macros)
}

/// Sustituye las macros en una linea (no dentro de comillas).
fn expand_macros(macros: &BTreeMap<String, String>, line: &str) -> String {
    if macros.is_empty() {
        return line.to_string();
    }
    let mut cur = line.to_string();
    for _ in 0..4 {
        let chars: Vec<char> = cur.chars().collect();
        let mut out = String::new();
        let mut i = 0;
        let mut changed = false;
        let mut quote: Option<char> = None;
        while i < chars.len() {
            let c = chars[i];
            if let Some(q) = quote {
                out.push(c);
                if c == q {
                    quote = None;
                }
                i += 1;
            } else if c == '\'' || c == '"' {
                quote = Some(c);
                out.push(c);
                i += 1;
            } else if is_ident_start(c) {
                let s = i;
                while i < chars.len() && is_ident_char(chars[i]) {
                    i += 1;
                }
                let w: String = chars[s..i].iter().collect();
                match macros.get(&w) {
                    Some(r) if *r != w => {
                        out.push_str(r);
                        changed = true;
                    }
                    _ => out.push_str(&w),
                }
            } else if c.is_ascii_digit() {
                while i < chars.len() && chars[i].is_ascii_alphanumeric() {
                    out.push(chars[i]);
                    i += 1;
                }
            } else {
                out.push(c);
                i += 1;
            }
        }
        cur = out;
        if !changed {
            break;
        }
    }
    cur
}

struct Pre<'a> {
    env: &'a AsmEnv,
    macros: BTreeMap<String, String>,
    defined: BTreeSet<String>,
    xc_included: bool,
    errors: Vec<AsmError>,
}

impl Pre<'_> {
    fn include_xc(&mut self) {
        if self.xc_included {
            return;
        }
        self.xc_included = true;
        let (_, macros) = xc_inc(&self.env.regs);
        self.defined.extend(self.env.regs.iter().map(|r| r.name.clone()));
        self.defined.extend(macros.keys().cloned());
        self.macros.extend(macros);
    }

    /// Sustituye las macros en una linea (no dentro de comillas).
    fn expand(&self, line: &str) -> String {
        expand_macros(&self.macros, line)
    }

    fn run(&mut self, src: &str) -> Vec<(usize, String)> {
        let mut out = Vec::new();
        let mut in_block = false;
        // pila de condiciones: (activa, ya_tomada, padre_activo)
        let mut cond: Vec<(bool, bool, bool)> = Vec::new();
        for (n, raw) in src.lines().enumerate() {
            let line = n + 1;
            let text = strip_comments(raw, &mut in_block);
            let t = text.trim();
            let active = cond.last().is_none_or(|c| c.0);
            if let Some(rest) = t.strip_prefix('#') {
                let rest = rest.trim();
                let (word, arg) = rest.split_once(char::is_whitespace).map_or((rest, ""), |(w, a)| (w, a.trim()));
                match word.to_ascii_lowercase().as_str() {
                    "ifdef" | "ifndef" => {
                        let d = self.defined.contains(arg);
                        let take = active && (d == (word.eq_ignore_ascii_case("ifdef")));
                        cond.push((take, take, active));
                    }
                    "if" => {
                        let v = self.cond_value(arg, line);
                        cond.push((active && v, active && v, active));
                    }
                    "elif" => match cond.last_mut() {
                        Some(c) => {
                            let v = c.2 && !c.1 && self.cond_value_static(arg);
                            c.0 = v;
                            c.1 |= v;
                        }
                        None => self.errors.push(AsmError { line, col: 1, code: "ASM008", message: "#elif sin #if".into() }),
                    },
                    "else" => match cond.last_mut() {
                        Some(c) => {
                            let v = c.2 && !c.1;
                            c.0 = v;
                            c.1 |= v;
                        }
                        None => self.errors.push(AsmError { line, col: 1, code: "ASM008", message: "#else sin #if".into() }),
                    },
                    "endif" => {
                        if cond.pop().is_none() {
                            self.errors.push(AsmError { line, col: 1, code: "ASM008", message: "#endif sin #if".into() });
                        }
                    }
                    "define" if active => {
                        let (name, repl) = arg.split_once(char::is_whitespace).map_or((arg, ""), |(a, b)| (a, b.trim()));
                        if name.is_empty() || name.contains('(') {
                            self.errors.push(AsmError { line, col: 1, code: "ASM008", message: "solo se admite #define NOMBRE [valor] (sin parametros)".into() });
                        } else {
                            self.defined.insert(name.to_string());
                            if !repl.is_empty() {
                                self.macros.insert(name.to_string(), self.expand(repl));
                            } else {
                                self.macros.remove(name);
                            }
                        }
                    }
                    "undef" if active => {
                        self.defined.remove(arg);
                        self.macros.remove(arg);
                    }
                    "include" if active => self.include(arg, line),
                    "define" | "undef" | "include" => {}
                    other => {
                        if active {
                            self.errors.push(AsmError {
                                line,
                                col: 1,
                                code: "ASM008",
                                message: format!("directiva del preprocesador '#{other}' no soportada (se admiten #include, #define, #undef, #if, #ifdef, #ifndef, #elif, #else, #endif)"),
                            });
                        }
                    }
                }
                continue;
            }
            if !active {
                continue;
            }
            // INCLUDE <xc.inc> como directiva del ensamblador
            let up = t.to_ascii_uppercase();
            if up.starts_with("INCLUDE ") || up.starts_with("INCLUDE\t") {
                self.include(t[7..].trim(), line);
                continue;
            }
            out.push((line, self.expand(&text)));
        }
        if !cond.is_empty() {
            self.errors.push(AsmError { line: src.lines().count().max(1), col: 1, code: "ASM008", message: "falta #endif".into() });
        }
        out
    }

    fn include(&mut self, arg: &str, line: usize) {
        let name = arg.trim_matches(|c| c == '<' || c == '>' || c == '"');
        if !name.eq_ignore_ascii_case("xc.inc") {
            self.errors.push(AsmError { line, col: 1, code: "ASM008", message: format!("solo se puede incluir <xc.inc> ('{name}' no se encuentra)") });
        } else if self.env.regs.is_empty() {
            self.errors.push(AsmError { line, col: 1, code: "ASM008", message: "xc.inc depende del dispositivo: indica uno con -mcpu=16F887, --device o la directiva PROCESSOR".into() });
        } else {
            self.include_xc();
        }
    }

    fn cond_value(&mut self, arg: &str, line: usize) -> bool {
        let a = self.expand(arg);
        let a = a.trim();
        if let Some(inner) = a.strip_prefix("defined") {
            return self.defined.contains(inner.trim().trim_matches(|c| c == '(' || c == ')').trim());
        }
        match parse_number(a) {
            Ok(v) => v != 0,
            Err(_) => {
                self.errors.push(AsmError { line, col: 1, code: "ASM008", message: "#if solo admite 'defined(NOMBRE)' o un numero".into() });
                false
            }
        }
    }

    fn cond_value_static(&self, arg: &str) -> bool {
        let a = self.expand(arg);
        let a = a.trim();
        match a.strip_prefix("defined") {
            Some(inner) => self.defined.contains(inner.trim().trim_matches(|c| c == '(' || c == ')').trim()),
            None => parse_number(a).map(|v| v != 0).unwrap_or(false),
        }
    }
}

/// Dispositivo de la directiva `PROCESSOR` (`PROCESSOR 16F887`), para elegir el Device Pack antes de ensamblar.
pub fn declared_processor(src: &str) -> Option<String> {
    let mut in_block = false;
    for raw in src.lines() {
        let t = strip_comments(raw, &mut in_block);
        let mut it = t.split_whitespace();
        if it.next().is_some_and(|w| w.eq_ignore_ascii_case("PROCESSOR")) {
            return it.next().map(str::to_string);
        }
    }
    None
}

// ---------------------------------------------------------------- psects y enlace

#[derive(Clone, Debug)]
struct Psect {
    name: String,
    delta: u32,
    class: Option<String>,
    abs: bool,
    space: u32,
    reloc: u32,
    /// Contador de ubicacion actual (en unidades de direccion), relativo al comienzo del psect.
    loc: u32,
}

impl Psect {
    fn new(name: &str) -> Psect {
        Psect { name: name.to_string(), delta: 1, class: None, abs: false, space: 0, reloc: 1, loc: 0 }
    }
}

/// Los psects que define `xc.inc`: (nombre, clase, delta, space).
fn predefined_psect(name: &str, env: &AsmEnv) -> Option<Psect> {
    let (class, delta, space): (String, u32, u32) = match name {
        "code" => ("CODE".into(), 2, 0),
        "data" => ("STRCODE".into(), 2, 0),
        "udata" => ("RAM".into(), 1, 1),
        "udata_shr" => ("COMMON".into(), 1, 1),
        n if n.starts_with("udata_bank") => (format!("BANK{}", &n["udata_bank".len()..]), 1, 1),
        _ => return None,
    };
    if space == 1 && !env.data_classes.contains_key(&class) {
        return None;
    }
    let mut p = Psect::new(name);
    p.class = Some(class);
    p.delta = delta;
    p.space = space;
    Some(p)
}

enum Kind {
    Inst(String, Vec<Vec<T>>),
    /// DB (`bytes`) o DW: una palabra por valor y por caracter de cada cadena.
    Data {
        ops: Vec<Vec<T>>,
        bytes: bool,
    },
    /// BANKSEL, BANKISEL, PAGESEL, FCALL, LJMP.
    Special(String, Vec<Vec<T>>),
    /// `nombre SET valor` evaluado en orden.
    Set(String, Vec<T>),
}

struct Item {
    line: usize,
    col: usize,
    psect: usize,
    off: u32,
    kind: Kind,
}

struct ConfigSet {
    line: usize,
    col: usize,
    name: String,
    value: Vec<T>,
}

fn special_words(name: &str) -> u32 {
    match name {
        "BANKSEL" | "PAGESEL" => 2,
        "BANKISEL" => 1,
        "FCALL" => 5,
        _ => 3, // LJMP
    }
}

fn data_words(ops: &[Vec<T>]) -> u32 {
    ops.iter().map(|o| if let [(Tok::Str(s), _)] = o.as_slice() { s.chars().count() as u32 } else { 1 }).sum()
}

struct Asm1<'a> {
    env: &'a AsmEnv,
    xc: bool,
    errors: Vec<AsmError>,
    psects: Vec<Psect>,
    pidx: BTreeMap<String, usize>,
    cur: Option<usize>,
    items: Vec<Item>,
    labels: BTreeMap<String, (usize, u32)>,
    consts: BTreeMap<String, i64>,
    deferred: Vec<(String, Vec<T>, usize, usize)>,
    configs: Vec<ConfigSet>,
    /// (activa, ya_tomada, padre_activo) de los IF anidados
    cond: Vec<(bool, bool, bool)>,
}

impl Asm1<'_> {
    fn e(&mut self, line: usize, col: usize, code: &'static str, message: impl Into<String>) {
        self.errors.push(AsmError { line, col, code, message: message.into() });
    }

    fn define_const(&mut self, name: &str, toks: &[T], line: usize, col: usize, is_set: bool) {
        if is_reserved(name) {
            self.e(line, col, "ASM005", format!("'{name}' es una palabra reservada y no puede ser un simbolo"));
            return;
        }
        if !is_set && (self.consts.contains_key(name) || self.labels.contains_key(name) || self.deferred.iter().any(|d| d.0 == name)) {
            self.e(line, col, "ASM005", format!("el simbolo '{name}' ya esta definido (EQU no se puede redefinir; usa SET)"));
            return;
        }
        if is_set {
            self.items.push(Item { line, col, psect: self.cur.unwrap_or(0), off: 0, kind: Kind::Set(name.to_string(), toks.to_vec()) });
            // el valor inmediato sirve a ORG y DS que lo usen despues
            let ctx = Ctx { syms: &self.consts, pc: None, line };
            if let Ok(v) = eval(toks, &ctx, col) {
                self.consts.insert(name.to_string(), v);
            }
            return;
        }
        let ctx = Ctx { syms: &self.consts, pc: None, line };
        match eval(toks, &ctx, col) {
            Ok(v) => {
                self.consts.insert(name.to_string(), v);
            }
            // depende de una etiqueta (se conoce tras el enlace) o de un simbolo posterior
            Err(e) if e.code == "ASM005" || e.message.starts_with("'$'") => self.deferred.push((name.to_string(), toks.to_vec(), line, col)),
            Err(e) => self.errors.push(e),
        }
    }

    fn const_eval(&mut self, toks: &[T], line: usize, col: usize, what: &str) -> Option<i64> {
        let ctx = Ctx { syms: &self.consts, pc: None, line };
        match eval(toks, &ctx, col) {
            Ok(v) => Some(v),
            Err(e) if e.code == "ASM005" => {
                self.errors.push(AsmError { line, col: e.col, code: "ASM005", message: format!("{} (en {what} el valor debe conocerse al ensamblar: usa una constante definida antes)", e.message) });
                None
            }
            Err(e) => {
                self.errors.push(e);
                None
            }
        }
    }

    fn cur_psect(&mut self, line: usize, col: usize) -> Option<usize> {
        if self.cur.is_none() {
            self.e(line, col, "ASM009", "codigo o datos fuera de un PSECT: pic-as no sabria donde enlazarlos; empieza con 'PSECT code' (tras #include <xc.inc>) o con tu propio PSECT");
        }
        self.cur
    }

    fn put(&mut self, line: usize, col: usize, words: u32, kind: Kind) {
        let Some(p) = self.cur_psect(line, col) else { return };
        let off = self.psects[p].loc;
        self.psects[p].loc += words;
        self.items.push(Item { line, col, psect: p, off, kind });
    }

    fn psect_directive(&mut self, ops: &[Vec<T>], line: usize, col: usize) {
        let Some(first) = ops.first() else {
            return self.e(line, col, "ASM003", "PSECT necesita un nombre");
        };
        let [(Tok::Ident(name), _)] = first.as_slice() else {
            return self.e(line, col, "ASM003", "el nombre de un PSECT es un identificador");
        };
        let idx = match self.pidx.get(name) {
            Some(&i) => i,
            None => {
                let p = if self.xc { predefined_psect(name, self.env).unwrap_or_else(|| Psect::new(name)) } else { Psect::new(name) };
                self.psects.push(p);
                self.pidx.insert(name.clone(), self.psects.len() - 1);
                self.psects.len() - 1
            }
        };
        for f in &ops[1..] {
            let Some((Tok::Ident(flag), fcol)) = f.first() else {
                self.e(line, col, "ASM003", "un flag de PSECT es un identificador (delta=2, class=CODE, abs...)");
                continue;
            };
            let value = f.get(2..).unwrap_or(&[]);
            let has_eq = matches!(f.get(1), Some((Tok::Eq, _)));
            let text: String = value
                .iter()
                .map(|(t, _)| match t {
                    Tok::Ident(s) => s.clone(),
                    Tok::Num(n) => n.to_string(),
                    Tok::Colon => ":".into(),
                    _ => String::new(),
                })
                .collect();
            let num = |this: &mut Self| -> Option<i64> {
                let ctx = Ctx { syms: &this.consts, pc: None, line };
                match eval(value, &ctx, *fcol) {
                    Ok(v) => Some(v),
                    Err(e) => {
                        this.errors.push(e);
                        None
                    }
                }
            };
            match (flag.to_ascii_lowercase().as_str(), has_eq) {
                ("abs", false) => self.psects[idx].abs = true,
                ("class", true) => {
                    let known = text == "CODE" || text == "STRCODE" || text == "ENTRY" || text == "CONST" || text == "EEDATA" || self.env.data_classes.contains_key(&text);
                    if !known {
                        self.e(line, *fcol, "ASM010", format!("clase '{text}' desconocida para este dispositivo (CODE, STRCODE, BANK0..., COMMON, RAM)"));
                    }
                    self.psects[idx].class = Some(text);
                }
                ("delta", true) => {
                    if let Some(v) = num(self) {
                        if v != 1 && v != 2 {
                            self.e(line, *fcol, "ASM010", "delta es 1 (memoria de datos) o 2 (memoria de programa)");
                        } else {
                            self.psects[idx].delta = v as u32;
                        }
                    }
                }
                ("space", true) => {
                    if let Some(v) = num(self) {
                        self.psects[idx].space = v as u32;
                    }
                }
                ("reloc", true) => {
                    if let Some(v) = num(self) {
                        self.psects[idx].reloc = v.max(1) as u32;
                    }
                }
                ("ovrld" | "global" | "local" | "pure" | "noexec" | "note" | "keep" | "inline", false) => {}
                ("size" | "limit" | "with" | "optim" | "merge" | "split" | "lowdata", _) => {}
                ("bit", false) => self.e(line, *fcol, "ASM008", "los psects de bits (flag bit) no estan soportados todavia"),
                (other, _) => self.e(line, *fcol, "ASM010", format!("flag de PSECT desconocido '{other}'")),
            }
        }
        // un psect de datos en una clase de datos usa direcciones de datos
        if let Some(c) = self.psects[idx].class.clone() {
            if self.env.data_classes.contains_key(&c) {
                self.psects[idx].space = 1;
            }
        }
        self.cur = Some(idx);
    }

    fn org(&mut self, ops: &[Vec<T>], line: usize, col: usize) {
        let Some(p) = self.cur_psect(line, col) else { return };
        if ops.len() != 1 {
            return self.e(line, col, "ASM003", "ORG recibe un valor");
        }
        let Some(v) = self.const_eval(&ops[0], line, col, "ORG") else { return };
        if v < 0 {
            return self.e(line, col, "ASM006", format!("ORG {v}: el valor no puede ser negativo"));
        }
        let loc = self.psects[p].loc;
        if (v as u32) < loc {
            return self.e(line, col, "ASM006", format!("ORG {v:#x} retrocede: el contador de ubicacion de '{}' ya esta en {loc:#x} y no puede moverse hacia atras", self.psects[p].name));
        }
        self.psects[p].loc = v as u32;
    }

    fn config(&mut self, ops: &[Vec<T>], line: usize, col: usize) {
        if ops.is_empty() {
            return self.e(line, col, "ASM003", "CONFIG necesita 'ajuste = valor'");
        }
        for g in ops {
            // CONFIG "WDT = ON"
            if let [(Tok::Str(s), c)] = g.as_slice() {
                match s.split_once('=') {
                    Some((n, v)) => {
                        let value = lex(v.trim(), line).unwrap_or_default();
                        self.configs.push(ConfigSet { line, col: *c, name: n.trim().trim_matches(|c| c == '"' || c == '\'').to_string(), value });
                    }
                    None => self.e(line, *c, "ASM003", "CONFIG \"ajuste = valor\": falta el '='"),
                }
                continue;
            }
            let name = match g.first() {
                Some((Tok::Ident(n) | Tok::Str(n), _)) => n.clone(),
                _ => {
                    self.e(line, g.first().map_or(col, |t| t.1), "ASM003", "CONFIG: se esperaba el nombre de un ajuste");
                    continue;
                }
            };
            if !matches!(g.get(1), Some((Tok::Eq, _))) || g.len() < 3 {
                self.e(line, g.first().map_or(col, |t| t.1), "ASM003", format!("CONFIG {name}: se escribe 'CONFIG {name} = valor'"));
                continue;
            }
            self.configs.push(ConfigSet { line, col: g[0].1, name, value: g[2..].to_vec() });
        }
    }

    fn statement(&mut self, line: usize, toks: Vec<T>) {
        let mut i = 0;
        // etiqueta
        if let (Some((Tok::Ident(name), col)), Some((Tok::Colon, _))) = (toks.first(), toks.get(1)) {
            i = 2;
            if self.cond.last().is_none_or(|c| c.0) {
                if is_reserved(name) {
                    self.e(line, *col, "ASM005", format!("'{name}' es una palabra reservada y no puede ser una etiqueta"));
                } else if self.labels.contains_key(name) || self.consts.contains_key(name) {
                    self.e(line, *col, "ASM005", format!("el simbolo '{name}' ya esta definido"));
                } else if let Some(p) = self.cur_psect(line, *col) {
                    self.labels.insert(name.clone(), (p, self.psects[p].loc));
                }
            }
        }
        let rest = &toks[i..];
        let Some((Tok::Ident(head), hcol)) = rest.first() else {
            if !rest.is_empty() {
                self.e(line, rest[0].1, "ASM001", "se esperaba una instruccion, una directiva o una etiqueta");
            }
            return;
        };
        let upper = head.to_ascii_uppercase();
        let ops: Vec<Vec<T>> = split_operands(&rest[1..]).into_iter().map(|s| s.to_vec()).collect();
        // IF/ELSIF/ELSE/ENDIF se procesan aunque la rama no este activa
        match upper.as_str() {
            "IF" | "ELSIF" => {
                let active = self.cond.last().is_none_or(|c| c.0);
                let v = if upper == "IF" {
                    if active {
                        self.const_eval(&rest[1..], line, *hcol, "IF").unwrap_or(0) != 0
                    } else {
                        false
                    }
                } else {
                    match self.cond.last() {
                        Some(c) if c.2 && !c.1 => self.const_eval(&rest[1..], line, *hcol, "ELSIF").unwrap_or(0) != 0,
                        Some(_) => false,
                        None => {
                            self.e(line, *hcol, "ASM008", "ELSIF sin IF");
                            false
                        }
                    }
                };
                if upper == "IF" {
                    self.cond.push((active && v, active && v, active));
                } else if let Some(c) = self.cond.last_mut() {
                    c.0 = v;
                    c.1 |= v;
                }
                return;
            }
            "ELSE" => {
                match self.cond.last_mut() {
                    Some(c) => {
                        let v = c.2 && !c.1;
                        c.0 = v;
                        c.1 |= v;
                    }
                    None => self.e(line, *hcol, "ASM008", "ELSE sin IF"),
                }
                return;
            }
            "ENDIF" => {
                if self.cond.pop().is_none() {
                    self.e(line, *hcol, "ASM008", "ENDIF sin IF");
                }
                return;
            }
            _ => {}
        }
        if !self.cond.last().is_none_or(|c| c.0) {
            return;
        }
        // nombre EQU valor / nombre SET valor
        if let Some((Tok::Ident(d), _)) = rest.get(1) {
            let du = d.to_ascii_uppercase();
            if du == "EQU" || du == "SET" {
                return self.define_const(head, &rest[2..], line, *hcol, du == "SET");
            }
        }
        match upper.as_str() {
            "GLOBAL" | "PUBLIC" => {}
            "EXTRN" => self.e(line, *hcol, "ASM008", "EXTRN enlaza con otros modulos y el ensamblador trabaja con un solo archivo"),
            "END" => {}
            "PSECT" => self.psect_directive(&ops, line, *hcol),
            "ORG" => self.org(&ops, line, *hcol),
            "ALIGN" => {
                let Some(p) = self.cur_psect(line, *hcol) else { return };
                if ops.len() != 1 {
                    return self.e(line, *hcol, "ASM003", "ALIGN recibe un valor");
                }
                if let Some(v) = self.const_eval(&ops[0], line, *hcol, "ALIGN") {
                    if v <= 0 {
                        return self.e(line, *hcol, "ASM004", "ALIGN necesita un valor positivo");
                    }
                    let a = v as u32;
                    self.psects[p].loc = self.psects[p].loc.div_ceil(a) * a;
                }
            }
            "DS" => {
                let Some(p) = self.cur_psect(line, *hcol) else { return };
                if ops.len() != 1 {
                    return self.e(line, *hcol, "ASM003", "DS recibe la cantidad de unidades a reservar");
                }
                if let Some(v) = self.const_eval(&ops[0], line, *hcol, "DS") {
                    if v < 0 {
                        return self.e(line, *hcol, "ASM004", "DS necesita una cantidad no negativa");
                    }
                    self.psects[p].loc += v as u32;
                }
            }
            "DB" | "DW" => {
                if ops.is_empty() {
                    return self.e(line, *hcol, "ASM003", format!("{upper} necesita al menos un valor"));
                }
                let n = data_words(&ops);
                self.put(line, *hcol, n, Kind::Data { ops, bytes: upper == "DB" });
            }
            "CONFIG" => self.config(&ops, line, *hcol),
            "BANKSEL" | "BANKISEL" | "PAGESEL" | "FCALL" | "LJMP" => {
                if ops.len() != 1 {
                    return self.e(line, *hcol, "ASM003", format!("{upper} recibe un simbolo o una direccion"));
                }
                self.put(line, *hcol, special_words(&upper), Kind::Special(upper, ops));
            }
            m if MNEMONICS.contains(&m) => self.put(line, *hcol, 1, Kind::Inst(m.to_string(), ops)),
            "LOCAL" => self.e(line, *hcol, "ASM008", "LOCAL solo se usa dentro de un MACRO (define simbolos unicos en cada expansion)"),
            "EXITM" => self.e(line, *hcol, "ASM008", "EXITM no esta soportado: la expansion de macros es textual y no conoce las condiciones del ensamblado; usa IF/ELSE dentro de la macro"),
            m if IGNORED_DIRECTIVES.contains(&m) => {}
            m if UNSUPPORTED_DIRECTIVES.contains(&m) => self.e(line, *hcol, "ASM008", format!("la directiva {m} de pic-as no esta implementada: la guia no fija como se reparten 32 o 64 bits en palabras de 14 bits ni como se rellena el espacio absoluto (ver docs/asm-pic14.md)")),
            "ERROR" => self.e(line, *hcol, "ASM012", format!("ERROR: {}", ops.iter().flatten().filter_map(|t| if let Tok::Str(s) = &t.0 { Some(s.as_str()) } else { None }).collect::<Vec<_>>().join(" "))),
            m if ENHANCED_ONLY.contains(&m) => self.e(
                line,
                *hcol,
                "ASM002",
                format!("'{m}' no existe en el PIC16 clasico (PIC14): es de otra familia (PIC16F1xxx, 'enhanced mid-range'); las 35 instrucciones de este dispositivo estan en docs/asm-pic14.md"),
            ),
            m if MPASM_PSEUDO.contains(&m) => self.e(
                line,
                *hcol,
                "ASM002",
                format!("'{m}' es una pseudoinstruccion de MPASM y pic-as no la implementa: escribe la instruccion real (p. ej. 'movf f,w' en vez de MOVFW, 'bsf STATUS,0' en vez de SETC)"),
            ),
            m => {
                if let Some((_, hint)) = MPASM_DIRECTIVES.iter().find(|(n, _)| *n == m) {
                    self.e(line, *hcol, "ASM002", format!("'{head}' es una directiva de MPASM; en PIC-AS se escribe {hint}"));
                } else if rest.len() == 1 {
                    self.e(line, *hcol, "ASM002", format!("'{head}' no es una instruccion ni una directiva; si es una etiqueta, en PIC-AS termina en ':' ({head}:)"));
                } else {
                    self.e(line, *hcol, "ASM002", format!("instruccion o directiva desconocida '{head}'"));
                }
            }
        }
    }
}

// ---------------------------------------------------------------- macros (MACRO, REPT, IRP, IRPC, LOCAL)

struct MacroDef {
    params: Vec<String>,
    body: Vec<(usize, String)>,
}

/// Primera palabra de una linea y el resto, tras una etiqueta `nombre:` opcional (que se devuelve aparte).
fn split_line(text: &str) -> (Option<String>, String, String) {
    let t = text.trim();
    let (label, rest) = match t.find(':') {
        Some(i) if i > 0 && t[..i].chars().all(is_ident_char) && is_ident_start(t.chars().next().unwrap()) => (Some(t[..i].to_string()), t[i + 1..].trim()),
        _ => (None, t),
    };
    let end = rest.find(char::is_whitespace).unwrap_or(rest.len());
    (label, rest[..end].to_string(), rest[end..].trim().to_string())
}

/// Argumentos separados por comas; `<a,b>` agrupa una coma dentro de un argumento (se quitan los `<>`).
fn split_macro_args(s: &str) -> Vec<String> {
    if s.trim().is_empty() {
        return Vec::new();
    }
    let (mut out, mut cur, mut depth) = (Vec::new(), String::new(), 0i32);
    for c in s.chars() {
        match c {
            '<' => {
                depth += 1;
                if depth > 1 {
                    cur.push(c);
                }
            }
            '>' if depth > 0 => {
                depth -= 1;
                if depth > 0 {
                    cur.push(c);
                }
            }
            ',' if depth == 0 => out.push(std::mem::take(&mut cur).trim().to_string()),
            _ => cur.push(c),
        }
    }
    out.push(cur.trim().to_string());
    out
}

/// Sustituye los parametros formales (palabras completas, tambien dentro de comillas) y aplica `&`: `PORT&p` con p=A es `PORTA`.
fn substitute(text: &str, map: &BTreeMap<String, String>) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if is_ident_start(c) {
            let s = i;
            while i < chars.len() && is_ident_char(chars[i]) {
                i += 1;
            }
            let w: String = chars[s..i].iter().collect();
            out.push_str(map.get(&w).unwrap_or(&w));
        } else if c.is_ascii_digit() {
            while i < chars.len() && chars[i].is_ascii_alphanumeric() {
                out.push(chars[i]);
                i += 1;
            }
        } else {
            out.push(c);
            i += 1;
        }
    }
    // `&&` es un AND de bits; un `&` suelto solo concatena y desaparece
    out.replace("&&", "\u{1}").replace('&', "").replace('\u{1}', "&")
}

/// Cuerpo de una macro con los argumentos puestos y los `LOCAL` con nombres unicos (`??0001`).
fn instantiate(body: &[(usize, String)], params: &BTreeMap<String, String>, counter: &mut u32) -> Vec<(usize, String)> {
    let mut map = params.clone();
    let mut out = Vec::new();
    for (line, text) in body {
        let (label, head, rest) = split_line(text);
        if label.is_none() && head.eq_ignore_ascii_case("LOCAL") {
            for name in rest.split(',').map(str::trim).filter(|n| !n.is_empty()) {
                *counter += 1;
                map.insert(name.to_string(), format!("??{:04}", *counter));
            }
            continue;
        }
        out.push((*line, substitute(text, &map)));
    }
    out
}

fn starts_block(text: &str) -> bool {
    let (label, head, rest) = split_line(text);
    let second = rest.split_whitespace().next().unwrap_or("");
    let u = head.to_ascii_uppercase();
    let _ = label;
    matches!(u.as_str(), "REPT" | "IRP" | "IRPC") || second.eq_ignore_ascii_case("MACRO")
}

fn expand_macro_stage(lines: Vec<(usize, String)>, errors: &mut Vec<AsmError>) -> Vec<(usize, String)> {
    let mut defs: BTreeMap<String, MacroDef> = BTreeMap::new();
    let mut counter = 0u32;
    let mut out = Vec::new();
    expand_block(&lines, &mut defs, &mut counter, errors, 0, &mut out);
    out
}

fn expand_block(lines: &[(usize, String)], defs: &mut BTreeMap<String, MacroDef>, counter: &mut u32, errors: &mut Vec<AsmError>, depth: u32, out: &mut Vec<(usize, String)>) {
    let err = |errors: &mut Vec<AsmError>, line: usize, msg: String| errors.push(AsmError { line, col: 1, code: "ASM008", message: msg });
    let mut i = 0;
    while i < lines.len() {
        let (line, text) = (lines[i].0, lines[i].1.clone());
        let (label, head, rest) = split_line(&text);
        let hu = head.to_ascii_uppercase();
        let second = rest.split_whitespace().next().unwrap_or("").to_string();
        let is_def = second.eq_ignore_ascii_case("MACRO") && label.is_none();
        let is_loop = matches!(hu.as_str(), "REPT" | "IRP" | "IRPC");
        if hu == "ENDM" {
            err(errors, line, "ENDM sin un MACRO, REPT, IRP o IRPC abierto".into());
            i += 1;
            continue;
        }
        if is_def || is_loop {
            // recoge el cuerpo hasta su ENDM, contando los bloques anidados
            let mut nest = 1;
            let mut j = i + 1;
            while j < lines.len() {
                if starts_block(&lines[j].1) {
                    nest += 1;
                } else if split_line(&lines[j].1).1.eq_ignore_ascii_case("ENDM") {
                    nest -= 1;
                    if nest == 0 {
                        break;
                    }
                }
                j += 1;
            }
            if j >= lines.len() {
                err(errors, line, format!("falta el ENDM de este {}", if is_def { "MACRO" } else { hu.as_str() }));
                return;
            }
            let body: Vec<(usize, String)> = lines[i + 1..j].to_vec();
            if is_def {
                let name = head.clone();
                let args = rest[second.len()..].trim();
                if is_reserved(&name) {
                    err(errors, line, format!("'{name}' es una palabra reservada y no puede ser el nombre de una macro"));
                } else {
                    match defs.entry(name.clone()) {
                        std::collections::btree_map::Entry::Occupied(_) => err(errors, line, format!("la macro '{name}' ya esta definida")),
                        std::collections::btree_map::Entry::Vacant(slot) => {
                            let params: Vec<String> = args.split(',').map(str::trim).filter(|p| !p.is_empty()).map(String::from).collect();
                            slot.insert(MacroDef { params, body });
                        }
                    }
                }
            } else {
                let args = split_macro_args(&rest);
                let mut passes: Vec<BTreeMap<String, String>> = Vec::new();
                match hu.as_str() {
                    "REPT" => {
                        let n = lex(&rest, line).ok().and_then(|t| if let [(Tok::Num(n), _)] = t.as_slice() { Some(*n) } else { None });
                        match n {
                            Some(n) if (0..=4096).contains(&n) => passes = vec![BTreeMap::new(); n as usize],
                            Some(n) => err(errors, line, format!("REPT {n}: de 0 a 4096 repeticiones")),
                            None => err(errors, line, "REPT necesita un numero constante de repeticiones (p. ej. REPT 3)".into()),
                        }
                    }
                    "IRP" if args.len() >= 2 => {
                        passes = args[1..].iter().map(|a| BTreeMap::from([(args[0].clone(), a.clone())])).collect();
                    }
                    "IRPC" if args.len() == 2 => {
                        passes = args[1].chars().filter(|c| !c.is_whitespace()).map(|c| BTreeMap::from([(args[0].clone(), c.to_string())])).collect();
                    }
                    "IRP" => err(errors, line, "IRP necesita un nombre y al menos un valor: IRP nombre,v1,v2".into()),
                    _ => err(errors, line, "IRPC necesita un nombre y una cadena: IRPC nombre,ABC".into()),
                }
                for p in passes {
                    let expanded = instantiate(&body, &p, counter);
                    expand_block(&expanded, defs, counter, errors, depth + 1, out);
                }
            }
            i = j + 1;
            continue;
        }
        // invocacion de una macro definida
        if let Some(def) = defs.get(&head) {
            if depth > 32 {
                err(errors, line, format!("la macro '{head}' se llama a si misma sin fin (mas de 32 niveles)"));
                return;
            }
            let args = split_macro_args(&rest);
            if args.len() > def.params.len() {
                err(errors, line, format!("la macro '{head}' recibe {} argumento(s) y se le pasaron {}", def.params.len(), args.len()));
            } else {
                let params: BTreeMap<String, String> = def.params.iter().enumerate().map(|(k, p)| (p.clone(), args.get(k).cloned().unwrap_or_default())).collect();
                let body = def.body.clone();
                let expanded = instantiate(&body, &params, counter);
                if let Some(l) = label {
                    out.push((line, format!("{l}:")));
                }
                // las lineas de la expansion llevan la linea de la invocacion
                let expanded: Vec<(usize, String)> = expanded.into_iter().map(|(_, t)| (line, t)).collect();
                expand_block(&expanded, defs, counter, errors, depth + 1, out);
            }
            i += 1;
            continue;
        }
        out.push((line, text));
        i += 1;
    }
}

/// Ensambla `src` sin informacion del dispositivo (sin `xc.inc` ni nombres de CONFIG).
pub fn assemble(src: &str) -> Result<Assembled, Vec<AsmError>> {
    assemble_with(src, &AsmEnv::default())
}

/// Rangos de una clase de memoria de programa: paginas de 0x800 palabras (los psects de codigo no las cruzan).
fn class_ranges(class: &str, env: &AsmEnv) -> Option<Vec<(u32, u32)>> {
    match class {
        "CODE" | "ENTRY" | "STRING" | "CONST" => {
            let mut v = Vec::new();
            let mut s = 0;
            while s < env.program_words {
                v.push((s, (s + 0x7FF).min(env.program_words - 1)));
                s += 0x800;
            }
            Some(v)
        }
        "STRCODE" => Some(vec![(0, env.program_words.saturating_sub(1))]),
        c => env.data_classes.get(c).cloned(),
    }
}

fn overlaps(used: &[(u32, u32)], s: u32, e: u32) -> bool {
    used.iter().any(|&(a, b)| s <= b && a <= e)
}

/// Coloca los psects: `abs` en 0, los de `-p` donde se pida y el resto en el primer hueco de su clase.
fn link(psects: &[Psect], env: &AsmEnv, errors: &mut Vec<AsmError>) -> Vec<u32> {
    let mut base: Vec<Option<u32>> = vec![None; psects.len()];
    let mut used: [Vec<(u32, u32)>; 2] = [Vec::new(), Vec::new()];
    let err_at = |errors: &mut Vec<AsmError>, m: String| errors.push(AsmError { line: 1, col: 1, code: "ASM006", message: m });
    let size_of = |p: &Psect| p.loc;
    let claim = |used: &mut [Vec<(u32, u32)>; 2], errors: &mut Vec<AsmError>, p: &Psect, at: u32| {
        let sz = size_of(p);
        if sz == 0 {
            return;
        }
        let sp = (p.space.min(1)) as usize;
        if overlaps(&used[sp], at, at + sz - 1) {
            err_at(errors, format!("el psect '{}' ({at:#x}..={:#x}) se solapa con otro psect ya colocado", p.name, at + sz - 1));
        }
        used[sp].push((at, at + sz - 1));
    };
    // 1) absolutos
    for (i, p) in psects.iter().enumerate() {
        if p.abs {
            base[i] = Some(0);
            claim(&mut used, errors, p, 0);
        }
    }
    // 2) -pNOMBRE=dir[,siguiente...], y los psects de arranque por omision (-preset_vec=00h,intentry,init,end_init)
    let mut placements: Vec<(String, u32, Vec<String>)> = env.placements.clone();
    if !placements.iter().any(|p| p.0 == "reset_vec") && psects.iter().any(|p| p.name == "reset_vec") {
        placements.push(("reset_vec".into(), 0, vec!["intentry".into(), "init".into(), "end_init".into()]));
    }
    for (name, addr, followers) in &placements {
        let mut at = *addr;
        for n in std::iter::once(name).chain(followers.iter()) {
            let Some(i) = psects.iter().position(|p| &p.name == n) else {
                if n == name && psects.iter().all(|p| p.name != *n) && !env.placements.is_empty() && env.placements.iter().any(|q| q.0 == *n) {
                    err_at(errors, format!("-p{n}: el programa no tiene ningun psect llamado '{n}'"));
                }
                continue;
            };
            if base[i].is_some() {
                err_at(errors, format!("el psect '{n}' ya esta colocado (es abs o se coloco dos veces)"));
                continue;
            }
            base[i] = Some(at);
            claim(&mut used, errors, &psects[i], at);
            at += size_of(&psects[i]);
        }
    }
    // 3) el resto, en orden de aparicion, en el primer hueco de su clase
    for (i, p) in psects.iter().enumerate() {
        if base[i].is_some() {
            continue;
        }
        if p.loc == 0 {
            base[i] = Some(0);
            continue;
        }
        let Some(class) = &p.class else {
            err_at(errors, format!("el psect '{}' no tiene clase (class=...) ni posicion (-p{}=dir): el enlazador no sabe donde ponerlo", p.name, p.name));
            continue;
        };
        let Some(ranges) = class_ranges(class, env) else {
            err_at(errors, format!("el psect '{}' usa la clase '{class}', que este dispositivo no define", p.name));
            continue;
        };
        let sp = (p.space.min(1)) as usize;
        let mut placed = None;
        'outer: for &(lo, hi) in &ranges {
            let mut at = lo.div_ceil(p.reloc.max(1)) * p.reloc.max(1);
            while at + p.loc - 1 <= hi {
                if !overlaps(&used[sp], at, at + p.loc - 1) {
                    placed = Some(at);
                    break 'outer;
                }
                // salta al final del psect que estorba
                let end = used[sp].iter().filter(|&&(a, b)| at <= b && a < at + p.loc).map(|&(_, b)| b).max().unwrap_or(at);
                at = (end + 1).div_ceil(p.reloc.max(1)) * p.reloc.max(1);
            }
        }
        match placed {
            Some(at) => {
                base[i] = Some(at);
                claim(&mut used, errors, p, at);
            }
            None => err_at(
                errors,
                format!("el psect '{}' ({} unidades) no cabe en la clase {class}: no hay un hueco libre en un solo rango (los psects de codigo no cruzan paginas de 0x800 palabras)", p.name, p.loc),
            ),
        }
    }
    base.into_iter().map(|b| b.unwrap_or(0)).collect()
}

/// Ensambla `src` con la informacion del dispositivo y las opciones del enlazador de `env`.
pub fn assemble_with(src: &str, env: &AsmEnv) -> Result<Assembled, Vec<AsmError>> {
    let mut pre = Pre { env, macros: BTreeMap::new(), defined: BTreeSet::new(), xc_included: false, errors: Vec::new() };
    let lines = pre.run(src);
    let mut macro_errors = Vec::new();
    let lines = expand_macro_stage(lines, &mut macro_errors);
    pre.errors.extend(macro_errors);
    let xc = pre.xc_included;
    let mut a = Asm1 {
        env,
        xc,
        errors: std::mem::take(&mut pre.errors),
        psects: Vec::new(),
        pidx: BTreeMap::new(),
        cur: None,
        items: Vec::new(),
        labels: BTreeMap::new(),
        consts: BTreeMap::new(),
        deferred: Vec::new(),
        configs: Vec::new(),
        cond: Vec::new(),
    };
    // xc.inc: los registros valen su direccion y cada campo tiene REG_CAMPO_POSN/POSITION/SIZE/LENGTH/MASK
    if xc {
        a.consts.extend(xc_inc(&env.regs).0);
    }
    let mut ended = false;
    let mut radix = 10u32;
    for (line, text) in &lines {
        if ended {
            break;
        }
        // PROCESSOR 16F887: el nombre del dispositivo empieza por un digito y no es un numero
        if let Some(arg) = text.trim().get(..9).filter(|h| h.eq_ignore_ascii_case("processor")).and_then(|_| text.trim()[9..].strip_prefix(|c: char| c.is_whitespace())) {
            let w = arg.trim();
            if let Some(dev) = &env.device {
                let norm = |s: &str| s.to_ascii_uppercase().trim_start_matches("PIC").to_string();
                if norm(dev) != norm(w) {
                    a.e(*line, text.find(w).map_or(1, |c| c + 1), "ASM011", format!("el programa dice PROCESSOR {w} pero se ensambla para {dev}"));
                }
            }
            continue;
        }
        // RADIX dec|hex|oct: la base de los numeros sin sufijo
        if let Some(arg) = text.trim().get(..5).filter(|h| h.eq_ignore_ascii_case("radix")).and_then(|_| text.trim()[5..].strip_prefix(|c: char| c.is_whitespace())) {
            match arg.trim().to_ascii_lowercase().as_str() {
                "dec" => radix = 10,
                "hex" => radix = 16,
                "oct" => radix = 8,
                other => a.e(*line, 1, "ASM003", format!("RADIX '{other}': las bases son dec, hex y oct")),
            }
            continue;
        }
        let mut toks = match lex_radix(text, *line, radix) {
            Ok(t) => t,
            Err(e) => {
                a.errors.push(e);
                continue;
            }
        };
        if toks.is_empty() {
            continue;
        }
        // CONFIG puede tener nombres de valor que consisten solo en dígitos
        // (p. ej. WRT=256); distinguir el texto original de 0x100.
        if matches!(toks.first(), Some((Tok::Ident(w), _)) if w.eq_ignore_ascii_case("CONFIG")) {
            for idx in 3..toks.len() {
                let (left, right) = toks.split_at_mut(idx);
                if let (Some((Tok::Ident(field), _)), Some((Tok::Eq, _)), Some((Tok::Num(_), col))) = (left.get(idx - 2), left.get(idx - 1), right.first()) {
                    let raw = text.get(col.saturating_sub(1)..).unwrap_or("").split(|c: char| !c.is_ascii_alphanumeric()).next().unwrap_or("");
                    if env.config.iter().flat_map(|w| &w.fields).any(|f| f.name == *field && f.names.contains_key(raw)) {
                        right[0].0 = Tok::Str(raw.to_string());
                    }
                }
            }
        }
        if matches!(toks.first(), Some((Tok::Ident(w), _)) if w.eq_ignore_ascii_case("END")) && a.cond.last().is_none_or(|c| c.0) {
            ended = true;
        }
        a.statement(*line, toks);
    }
    if !a.cond.is_empty() {
        let line = lines.last().map_or(1, |l| l.0);
        a.e(line, 1, "ASM008", "falta ENDIF");
    }

    // ---- enlace ----------------------------------------------------------------------------------
    let mut errors = std::mem::take(&mut a.errors);
    let base = link(&a.psects, env, &mut errors);
    let mut syms: BTreeMap<String, i64> = a.consts.clone();
    for (name, (p, off)) in &a.labels {
        syms.insert(name.clone(), (base[*p] + *off) as i64);
    }
    // EQU que dependian de etiquetas o de otros simbolos: hasta que no avancen
    let mut pending = std::mem::take(&mut a.deferred);
    loop {
        let before = pending.len();
        let mut next = Vec::new();
        for d in pending {
            let ctx = Ctx { syms: &syms, pc: None, line: d.2 };
            match eval(&d.1, &ctx, d.3) {
                Ok(v) => {
                    syms.insert(d.0.clone(), v);
                }
                Err(e) if e.code == "ASM005" => next.push(d),
                Err(e) => errors.push(e),
            }
        }
        pending = next;
        if pending.is_empty() || pending.len() == before {
            break;
        }
    }
    for d in &pending {
        let ctx = Ctx { syms: &syms, pc: None, line: d.2 };
        if let Err(e) = eval(&d.1, &ctx, d.3) {
            errors.push(e);
        }
    }

    // ---- emision ---------------------------------------------------------------------------------
    let mut image = Image::new();
    let mut emitted: BTreeMap<usize, Vec<(u32, u16)>> = BTreeMap::new();
    for it in &a.items {
        let p = &a.psects[it.psect];
        let addr0 = base[it.psect] + it.off;
        match &it.kind {
            Kind::Set(name, toks) => {
                let ctx = Ctx { syms: &syms, pc: None, line: it.line };
                match eval(toks, &ctx, it.col) {
                    Ok(v) => {
                        syms.insert(name.clone(), v);
                    }
                    Err(e) => errors.push(e),
                }
                continue;
            }
            _ if p.space != 0 || p.delta != 2 => {
                let why = if p.space != 0 {
                    "es de datos (usa DS para reservar memoria; una instruccion o DB/DW van en la memoria de programa)"
                } else {
                    "la memoria de programa de este dispositivo exige delta=2"
                };
                errors.push(AsmError { line: it.line, col: it.col, code: "ASM010", message: format!("el psect '{}' {why}", p.name) });
                continue;
            }
            _ => {}
        }
        let mut words: Vec<u16> = Vec::new();
        match &it.kind {
            Kind::Set(..) => {}
            Kind::Inst(m, ops) => {
                let ctx = Ctx { syms: &syms, pc: Some(addr0), line: it.line };
                match build(m, ops, &ctx, it.col, env.fixup_ignore).and_then(|i| i.encode().map_err(|e| AsmError { line: it.line, col: it.col, code: "ASM004", message: e.to_string() })) {
                    Ok(w) => words.push(w),
                    Err(e) => errors.push(e),
                }
            }
            Kind::Data { ops, bytes } => {
                for (k, op) in ops.iter().enumerate() {
                    if let [(Tok::Str(s), _)] = op.as_slice() {
                        words.extend(s.chars().map(|c| c as u16 & if *bytes { 0xFF } else { 0x3FFF }));
                        continue;
                    }
                    let ctx = Ctx { syms: &syms, pc: Some(addr0 + k as u32), line: it.line };
                    match eval(op, &ctx, it.col) {
                        Ok(v) if *bytes && (-128..=255).contains(&v) => words.push(v as u8 as u16),
                        Ok(v) if !*bytes && (0..=0x3FFF).contains(&v) => words.push(v as u16),
                        Ok(v) => errors.push(AsmError {
                            line: it.line,
                            col: op.first().map_or(it.col, |t| t.1),
                            code: "ASM004",
                            message: if *bytes { format!("DB {v}: un byte va de 0 a 255") } else { format!("DW {v}: una palabra de programa tiene 14 bits (0..=0x3FFF)") },
                        }),
                        Err(e) => errors.push(e),
                    }
                }
            }
            Kind::Special(name, ops) => {
                let ctx = Ctx { syms: &syms, pc: Some(addr0), line: it.line };
                let v = match eval(&ops[0], &ctx, it.col) {
                    Ok(v) => v,
                    Err(e) => {
                        errors.push(e);
                        continue;
                    }
                };
                let bit = |on: bool, f: u8, b: u8| if on { Inst::Bsf(f, b) } else { Inst::Bcf(f, b) };
                let insts: Vec<Inst> = match name.as_str() {
                    "BANKSEL" => vec![bit(v >> 7 & 1 != 0, 0x03, 5), bit(v >> 8 & 1 != 0, 0x03, 6)],
                    "BANKISEL" => vec![bit(v >> 8 & 1 != 0, 0x03, 7)],
                    "PAGESEL" => vec![bit(v >> 11 & 1 != 0, 0x0A, 3), bit(v >> 12 & 1 != 0, 0x0A, 4)],
                    _ => {
                        let target = (v as u32) & 0x7FF;
                        let cur = addr0 >> 11;
                        let mut w = vec![bit(v >> 12 & 1 != 0, 0x0A, 4), bit(v >> 11 & 1 != 0, 0x0A, 3)];
                        w.push(if name == "FCALL" { Inst::Call(target as u16) } else { Inst::Goto(target as u16) });
                        if name == "FCALL" {
                            w.push(bit(cur >> 1 & 1 != 0, 0x0A, 4));
                            w.push(bit(cur & 1 != 0, 0x0A, 3));
                        }
                        w
                    }
                };
                for i in insts {
                    match i.encode() {
                        Ok(w) => words.push(w),
                        Err(e) => errors.push(AsmError { line: it.line, col: it.col, code: "ASM004", message: e.to_string() }),
                    }
                }
            }
        }
        for (k, w) in words.into_iter().enumerate() {
            place(&mut image, &mut emitted, &mut errors, it.line, it.col, addr0 + k as u32, w);
        }
    }

    // ---- CONFIG ----------------------------------------------------------------------------------
    let mut warnings = Vec::new();
    apply_config(&a.configs, env, &syms, &mut image, &mut errors, &mut warnings);

    if !errors.is_empty() {
        errors.sort_by_key(|e| (e.line, e.col));
        errors.dedup();
        return Err(errors);
    }

    let mut listing = String::new();
    for (n, raw) in src.lines().enumerate() {
        match emitted.get(&(n + 1)) {
            Some(words) => {
                for (k, (ad, w)) in words.iter().enumerate() {
                    if k == 0 {
                        listing.push_str(&format!("{ad:04X} {w:04X}  {raw}\n"));
                    } else {
                        listing.push_str(&format!("{ad:04X} {w:04X}\n"));
                    }
                }
            }
            None => listing.push_str(&format!("           {raw}\n")),
        }
    }
    let psects = a.psects.iter().enumerate().map(|(i, p)| (p.name.clone(), base[i], p.loc)).collect();
    // no se devuelven los simbolos de xc.inc, solo los del programa
    let user: BTreeMap<String, i64> = syms.into_iter().filter(|(k, _)| a.labels.contains_key(k) || !env.regs.iter().any(|r| r.name == *k || k.starts_with(&format!("{}_", r.name)))).collect();
    Ok(Assembled { image, warnings, symbols: user, listing, psects })
}

fn apply_config(sets: &[ConfigSet], env: &AsmEnv, syms: &BTreeMap<String, i64>, image: &mut Image, errors: &mut Vec<AsmError>, warnings: &mut Vec<AsmError>) {
    let mut words: BTreeMap<u32, (u16, BTreeSet<String>)> = BTreeMap::new();
    for c in sets {
        if env.config.is_empty() && env.id_locations.is_empty() {
            errors.push(AsmError {
                line: c.line,
                col: c.col,
                code: "ASM013",
                message: "CONFIG necesita un dispositivo (-mcpu=16F887, --device o PROCESSOR) para conocer sus palabras de configuracion".into(),
            });
            return;
        }
        let ctx = Ctx { syms, pc: None, line: c.line };
        if let Some(id) = env.id_locations.iter().find(|id| id.name.eq_ignore_ascii_case(&c.name)) {
            match eval(&c.value, &ctx, c.col) {
                Ok(v) if v >= 0 => {
                    let truncated = v as u64 & id.mask.0 as u64;
                    if truncated != v as u64 {
                        warnings.push(AsmError { line: c.line, col: c.col, code: "ASMW001", message: format!("{} = {v} excede la mascara {:#x}: se usa {truncated}", id.name, id.mask.0) });
                    }
                    match words.entry(id.address.0) {
                        std::collections::btree_map::Entry::Occupied(_) => errors.push(AsmError { line: c.line, col: c.col, code: "ASM013", message: format!("{} se configura dos veces", id.name) }),
                        std::collections::btree_map::Entry::Vacant(slot) => {
                            slot.insert((truncated as u16, BTreeSet::new()));
                        }
                    }
                }
                Ok(v) => errors.push(AsmError { line: c.line, col: c.col, code: "ASM004", message: format!("{} = {v}: valor fuera de la mascara {:#x}", id.name, id.mask.0) }),
                Err(e) => errors.push(e),
            }
            continue;
        }
        // CONFIG CONFIG1 = 0x20D4: la palabra entera
        if let Some(w) = env.config.iter().find(|w| w.name.eq_ignore_ascii_case(&c.name)) {
            match eval(&c.value, &ctx, c.col) {
                Ok(v) if v >= 0 => {
                    let truncated = (v as u64) & 0x3fff;
                    if v as u64 != truncated {
                        warnings.push(AsmError { line: c.line, col: c.col, code: "ASMW001", message: format!("{} = {v} excede 14 bits: se usa {truncated}", w.name) });
                    }
                    let e = words.entry(w.address).or_insert((w.default, BTreeSet::new()));
                    if !e.1.insert(w.name.clone()) {
                        errors.push(AsmError { line: c.line, col: c.col, code: "ASM013", message: format!("{} se configura dos veces", w.name) });
                    }
                    e.0 = truncated as u16;
                }
                Ok(v) => errors.push(AsmError { line: c.line, col: c.col, code: "ASM004", message: format!("{} = {v}: la palabra de configuracion tiene 14 bits (0..=0x3FFF)", w.name) }),
                Err(e) => errors.push(e),
            }
            continue;
        }
        let Some((w, f)) = env.config.iter().find_map(|w| w.fields.iter().find(|f| f.name == c.name).map(|f| (w, f))) else {
            let all: Vec<&str> = env.config.iter().flat_map(|w| w.fields.iter().map(|f| f.name.as_str())).collect();
            errors.push(AsmError {
                line: c.line,
                col: c.col,
                code: "ASM013",
                message: format!("'{}' no es un ajuste de configuracion de {} (ajustes: {})", c.name, env.device.as_deref().unwrap_or("este dispositivo"), all.join(", ")),
            });
            continue;
        };
        // el valor es primero un nombre predefinido (ON, OFF, INTRC_NOCLKOUT...) y solo si no lo es, una constante
        let value: Option<u64> = match c.value.as_slice() {
            [(Tok::Ident(n) | Tok::Str(n), _)] if f.names.contains_key(n.as_str()) => Some(f.names[n.as_str()]),
            toks => match eval(toks, &ctx, c.col) {
                Ok(v) if v >= 0 => {
                    let mask = (1u64 << f.width) - 1;
                    let truncated = (v as u64) & mask;
                    if v as u64 > mask {
                        warnings.push(AsmError { line: c.line, col: c.col, code: "ASMW001", message: format!("{} = {v} excede {} bit(s): se usa {truncated}", f.name, f.width) });
                    }
                    Some(truncated)
                }
                Ok(v) => {
                    errors.push(AsmError { line: c.line, col: c.col, code: "ASM004", message: format!("{} = {v}: el campo tiene {} bit(s)", f.name, f.width) });
                    None
                }
                Err(e) => {
                    let names: Vec<&str> = f.names.keys().map(String::as_str).collect();
                    let hint = if names.is_empty() { format!("un numero 0..={}", (1u64 << f.width) - 1) } else { format!("{} o un numero", names.join(", ")) };
                    errors.push(AsmError { line: e.line, col: e.col, code: "ASM013", message: format!("valor no valido para {}: se esperaba {hint}", f.name) });
                    None
                }
            },
        };
        let Some(v) = value else { continue };
        let e = words.entry(w.address).or_insert((w.default, BTreeSet::new()));
        if !e.1.insert(f.name.clone()) {
            errors.push(AsmError { line: c.line, col: c.col, code: "ASM013", message: format!("{} se configura dos veces", f.name) });
            continue;
        }
        let mask = (((1u32 << f.width) - 1) << f.lsb) as u16;
        e.0 = (e.0 & !mask) | ((v as u16) << f.lsb);
    }
    for (addr, (w, _)) in words {
        if let Err(e) = image.set(addr, w) {
            errors.push(AsmError { line: 1, col: 1, code: "ASM006", message: format!("la palabra de configuracion {addr:#06x}: {e}") });
        }
    }
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

// ---------------------------------------------------------------- instrucciones

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

/// El operando `f` son 7 bits. Un simbolo de otro banco (TRISB = 0x86) se enmascara con BANKMASK() tras BANKSEL,
/// como en pic-as; con `--fixupoverflow=ignore` se trunca.
fn file_reg(v: i64, line: usize, col: usize, ignore: bool) -> Result<u8, AsmError> {
    if (0..=0x7F).contains(&v) {
        Ok(v as u8)
    } else if ignore {
        Ok((v & 0x7F) as u8)
    } else {
        err(line, col, "ASM004", format!("desbordamiento del operando: {v:#x} no cabe en 7 bits (el registro esta en otro banco): selecciona el banco con BANKSEL y escribe BANKMASK(simbolo)"))
    }
}

fn dest(op: &[T], line: usize, col: usize) -> Result<Dest, AsmError> {
    match op {
        [(Tok::Ident(n), _)] if n.eq_ignore_ascii_case("W") => Ok(Dest::W),
        [(Tok::Ident(n), _)] if n.eq_ignore_ascii_case("F") => Ok(Dest::F),
        [(Tok::Num(0), _)] => Ok(Dest::W),
        [(Tok::Num(1), _)] => Ok(Dest::F),
        _ => err(line, op.first().map_or(col, |t| t.1), "ASM003", "el destino debe ser w o f (tambien 0 o 1)"),
    }
}

fn build(m: &str, ops: &[Vec<T>], ctx: &Ctx, col: usize, ignore: bool) -> Result<Inst, AsmError> {
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
            let f = file_reg(val(0)?, line, opcol(0), ignore)?;
            if m == "CLRF" {
                Clrf(f)
            } else {
                Movwf(f)
            }
        }
        Shape::Fd => {
            if ops.is_empty() || ops.len() > 2 {
                return err(line, col, "ASM003", format!("{m} se escribe '{m} f,w' o '{m} f,f' ({} operando(s))", ops.len()));
            }
            let f = file_reg(val(0)?, line, opcol(0), ignore)?;
            // sin destino, pic-as usa el registro (f)
            let d = if ops.len() == 2 { dest(&ops[1], line, col)? } else { Dest::F };
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
            let f = file_reg(val(0)?, line, opcol(0), ignore)?;
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
            if (0..=0x7FF).contains(&a) {
                if m == "CALL" {
                    Call(a as u16)
                } else {
                    Goto(a as u16)
                }
            } else if ignore && a >= 0 {
                if m == "CALL" {
                    Call((a & 0x7FF) as u16)
                } else {
                    Goto((a & 0x7FF) as u16)
                }
            } else {
                return err(
                    line,
                    opcol(0),
                    "ASM004",
                    format!("desbordamiento del operando: {m} {a:#x} no cabe en 11 bits: usa PAGESEL y PAGEMASK(etiqueta), o 'fcall'/'ljmp', que seleccionan la pagina por ti"),
                );
            }
        }
    })
}

// ---- fragmentos: el cuerpo de una funcion `@asm_function` -------------------------------------------

/// Una palabra de un fragmento; los saltos llevan el indice (dentro del fragmento) de su destino.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FragItem {
    Inst(Inst),
    Goto(u32),
    Call(u32),
}

#[derive(Clone, Debug)]
pub struct Fragment {
    pub items: Vec<FragItem>,
    /// Indices de `items` a los que salta algun `GOTO`/`CALL` (el generador coloca ahi una etiqueta).
    pub targets: BTreeSet<u32>,
    /// Linea del fuente de cada palabra, para ubicar mensajes y listados.
    pub lines: Vec<usize>,
}

enum FStmt {
    Inst { line: usize, col: usize, pc: u32, mnemonic: String, ops: Vec<Vec<T>> },
}

/// Ensambla el cuerpo de una funcion con la sintaxis de pic-as. `externals` son los simbolos de datos
/// (parametros, globales y los de `xc.inc`, ver `xc_inc`) y `macros` los campos de registro (`RB5`); las etiquetas (`nombre:`) son locales al fragmento y solo se usan
/// como destino de `CALL`/`GOTO`. `first_line` es la linea del fuente donde empieza `src`.
///
/// Ademas: no hay `PSECT`, `ORG`, `DB`/`DW` ni `CONFIG` (la funcion se coloca donde la ponga el compilador),
/// no se usa `RETFIE` y la funcion debe terminar en `RETURN`, `RETLW` o `GOTO`.
pub fn assemble_fragment(src: &str, first_line: usize, externals: &BTreeMap<String, i64>, macros: &BTreeMap<String, String>) -> Result<Fragment, Vec<AsmError>> {
    let mut errors: Vec<AsmError> = Vec::new();
    let mut syms = externals.clone();
    let mut labels: BTreeMap<String, i64> = BTreeMap::new();
    let mut stmts: Vec<FStmt> = Vec::new();
    let mut pc: u32 = 0;
    let mut in_block = false;

    for (n, raw) in src.lines().enumerate() {
        let line = first_line + n;
        let code = expand_macros(macros, &strip_comments(raw, &mut in_block));
        let toks = match lex(&code, line) {
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
        if let (Some((Tok::Ident(name), col)), Some((Tok::Colon, _))) = (toks.first(), toks.get(1)) {
            if is_reserved(name) {
                errors.push(AsmError { line, col: *col, code: "ASM005", message: format!("'{name}' es una palabra reservada y no puede ser una etiqueta") });
            } else if syms.contains_key(name) || labels.contains_key(name) {
                errors.push(AsmError { line, col: *col, code: "ASM005", message: format!("el simbolo '{name}' ya esta definido (parametro, global, registro o etiqueta)") });
            } else {
                labels.insert(name.clone(), pc as i64);
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
        if let Some((Tok::Ident(e), _)) = rest.get(1) {
            if e.eq_ignore_ascii_case("EQU") || e.eq_ignore_ascii_case("SET") {
                let ctx = Ctx { syms: &syms, pc: Some(pc), line };
                if is_reserved(head) {
                    errors.push(AsmError { line, col: *hcol, code: "ASM005", message: format!("'{head}' es una palabra reservada y no puede ser una constante") });
                } else if syms.contains_key(head) || labels.contains_key(head) {
                    errors.push(AsmError { line, col: *hcol, code: "ASM005", message: format!("el simbolo '{head}' ya esta definido (parametro, global, registro o etiqueta)") });
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
        }
        let upper = head.to_ascii_uppercase();
        let ops: Vec<Vec<T>> = split_operands(&rest[1..]).into_iter().map(|s| s.to_vec()).collect();
        match upper.as_str() {
            "PSECT" | "ORG" | "DB" | "DW" | "DS" | "CONFIG" | "END" | "GLOBAL" | "PUBLIC" | "EXTRN" | "PROCESSOR" | "ALIGN" => errors.push(AsmError {
                line,
                col: *hcol,
                code: "ASM007",
                message: format!("{upper} no se usa dentro de una funcion @asm_function: el compilador decide donde va (una tabla se escribe con RETLW)"),
            }),
            "BANKSEL" | "BANKISEL" | "PAGESEL" | "FCALL" | "LJMP" => errors.push(AsmError {
                line,
                col: *hcol,
                code: "ASM007",
                message: format!("{upper} aun no se admite dentro de una funcion @asm_function: el compilador gestiona el banco y la pagina; usa BSF/BCF sobre STATUS y restablece el banco 0"),
            }),
            "RETFIE" => errors.push(AsmError { line, col: *hcol, code: "ASM007", message: "RETFIE solo vuelve de una interrupcion y una funcion @asm_function no lo es: usa RETURN o RETLW".into() }),
            m if MNEMONICS.contains(&m) => {
                stmts.push(FStmt::Inst { line, col: *hcol, pc, mnemonic: m.to_string(), ops });
                pc += 1;
            }
            m if ENHANCED_ONLY.contains(&m) => errors.push(AsmError {
                line,
                col: *hcol,
                code: "ASM002",
                message: format!("'{m}' no existe en el PIC16 clasico (PIC14): es de otra familia (PIC16F1xxx); las 35 instrucciones de este dispositivo estan en docs/asm-pic14.md"),
            }),
            m if MPASM_PSEUDO.contains(&m) => errors.push(AsmError {
                line,
                col: *hcol,
                code: "ASM002",
                message: format!("'{m}' es una pseudoinstruccion de MPASM y pic-as no la implementa: escribe la instruccion real (p. ej. BSF STATUS,0 en vez de SETC)"),
            }),
            _ if rest.len() == 1 => {
                errors.push(AsmError { line, col: *hcol, code: "ASM002", message: format!("'{head}' no es una instruccion; si es una etiqueta, en PIC-AS termina en ':' ({head}:)") })
            }
            _ => errors.push(AsmError { line, col: *hcol, code: "ASM002", message: format!("instruccion o directiva desconocida '{head}'") }),
        }
    }
    let total = pc;

    // las etiquetas valen su posicion dentro de la funcion y solo se evaluan como destino de salto: en un dato
    // no tienen valor, porque su direccion final la decide el compilador
    let mut jump_syms = syms.clone();
    jump_syms.extend(labels.iter().map(|(k, v)| (k.clone(), *v)));
    let mut items = Vec::new();
    let mut lines = Vec::new();
    let mut targets = BTreeSet::new();
    // estado estatico de RP0/RP1 en orden de texto, para exigir que se vuelva al banco 0 (invariante de la ABI)
    let (mut rp0, mut rp1) = (false, false);
    for st in &stmts {
        let FStmt::Inst { line, col, pc, mnemonic, ops } = st;
        if mnemonic == "CALL" || mnemonic == "GOTO" {
            if ops.len() != 1 {
                errors.push(AsmError { line: *line, col: *col, code: "ASM003", message: format!("{mnemonic} se escribe '{mnemonic} etiqueta' ({} operando(s) en lugar de 1)", ops.len()) });
                continue;
            }
            if let [(Tok::Ident(name), c)] = ops[0].as_slice() {
                if syms.contains_key(name) && !labels.contains_key(name) {
                    errors.push(AsmError {
                        line: *line,
                        col: *c,
                        code: "ASM005",
                        message: format!(
                            "'{name}' es un dato, no una etiqueta de esta funcion: {mnemonic} solo salta a etiquetas locales (llamar a otras funciones desde ensamblador aun no esta soportado)"
                        ),
                    });
                    continue;
                }
            }
            let ctx = Ctx { syms: &jump_syms, pc: Some(*pc), line: *line };
            match eval(&ops[0], &ctx, *col) {
                Ok(t) if (0..total as i64).contains(&t) => {
                    targets.insert(t as u32);
                    items.push(if mnemonic == "CALL" { FragItem::Call(t as u32) } else { FragItem::Goto(t as u32) });
                    lines.push(*line);
                }
                Ok(t) => errors.push(AsmError {
                    line: *line,
                    col: ops[0].first().map_or(*col, |x| x.1),
                    code: "ASM006",
                    message: format!("{mnemonic} a la palabra {t} de la funcion, fuera de ella (tiene {total}): una funcion @asm_function solo salta dentro de si misma"),
                }),
                Err(e) => errors.push(e),
            }
            continue;
        }
        let ctx = Ctx { syms: &syms, pc: Some(*pc), line: *line };
        // dentro de una funcion el operando f son 7 bits y el banco lo gestiona el programa: sin truncado automatico
        match build(mnemonic, ops, &ctx, *col, false) {
            Ok(inst) => {
                match inst {
                    Inst::Bsf(0x03, b @ (5 | 6)) => {
                        if b == 5 {
                            rp0 = true
                        } else {
                            rp1 = true
                        }
                    }
                    Inst::Bcf(0x03, b @ (5 | 6)) => {
                        if b == 5 {
                            rp0 = false
                        } else {
                            rp1 = false
                        }
                    }
                    Inst::Return | Inst::Retlw(_) if rp0 || rp1 => errors.push(AsmError {
                        line: *line,
                        col: *col,
                        code: "ASM007",
                        message: "vuelve con RP0/RP1 puestos: todo el codigo generado asume el banco 0 al volver; restablece los bits con BCF STATUS,RP0 y BCF STATUS,RP1 antes de RETURN/RETLW".into(),
                    }),
                    _ => {}
                }
                items.push(FragItem::Inst(inst));
                lines.push(*line);
            }
            Err(e) => errors.push(e),
        }
    }
    if errors.is_empty() {
        match items.last() {
            None => errors.push(AsmError { line: first_line, col: 1, code: "ASM007", message: "la funcion @asm_function no tiene instrucciones".into() }),
            Some(FragItem::Goto(_)) | Some(FragItem::Inst(Inst::Return)) | Some(FragItem::Inst(Inst::Retlw(_))) => {}
            Some(_) => errors.push(AsmError {
                line: *lines.last().unwrap_or(&first_line),
                col: 1,
                code: "ASM007",
                message: "la funcion termina sin RETURN, RETLW ni GOTO: seguiria ejecutando el codigo de la funcion siguiente".into(),
            }),
        }
    }
    if !errors.is_empty() {
        errors.sort_by_key(|e| (e.line, e.col));
        return Err(errors);
    }
    Ok(Fragment { items, targets, lines })
}
