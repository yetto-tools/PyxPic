//! Conjunto de instrucciones PIC14 (PIC16 clasica): 35 instrucciones de 14 bits.
//!
//! Fuente: DS40001291H, tabla 15-2 (p.227). Los campos `x` de la tabla son bits sin
//! importancia: el decodificador acepta todas las palabras posibles y el
//! codificador emite siempre la forma canonica (x = 0).

use std::fmt;

/// Destino de una operacion orientada a bytes: bit 7 del opcode (`d`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dest {
    /// `d = 0`: el resultado va a W.
    W,
    /// `d = 1`: el resultado va al registro `f`.
    F,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Inst {
    // orientadas a bytes (f, d)
    Addwf(u8, Dest),
    Andwf(u8, Dest),
    Comf(u8, Dest),
    Decf(u8, Dest),
    Decfsz(u8, Dest),
    Incf(u8, Dest),
    Incfsz(u8, Dest),
    Iorwf(u8, Dest),
    Movf(u8, Dest),
    Rlf(u8, Dest),
    Rrf(u8, Dest),
    Subwf(u8, Dest),
    Swapf(u8, Dest),
    Xorwf(u8, Dest),
    // orientadas a bytes (solo f)
    Clrf(u8),
    Movwf(u8),
    // orientadas a bits (f, b)
    Bcf(u8, u8),
    Bsf(u8, u8),
    Btfsc(u8, u8),
    Btfss(u8, u8),
    // literales (k de 8 bits)
    Addlw(u8),
    Andlw(u8),
    Iorlw(u8),
    Movlw(u8),
    Retlw(u8),
    Sublw(u8),
    Xorlw(u8),
    // saltos (k de 11 bits)
    Call(u16),
    Goto(u16),
    // sin operandos
    Clrw,
    Clrwdt,
    Nop,
    Retfie,
    Return,
    Sleep,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EncodeError {
    /// `f` debe caber en 7 bits (0..=127).
    FileOutOfRange(u8),
    /// `b` debe estar en 0..=7.
    BitOutOfRange(u8),
    /// Direccion de CALL/GOTO: 11 bits (0..=0x7FF).
    AddressOutOfRange(u16),
}

impl fmt::Display for EncodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EncodeError::FileOutOfRange(v) => write!(f, "registro {v:#x} fuera de rango (0..=0x7F)"),
            EncodeError::BitOutOfRange(v) => write!(f, "bit {v} fuera de rango (0..=7)"),
            EncodeError::AddressOutOfRange(v) => write!(f, "direccion {v:#x} fuera de rango (0..=0x7FF)"),
        }
    }
}

impl std::error::Error for EncodeError {}

/// Bits de STATUS que una instruccion puede modificar.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Flag {
    C,
    DC,
    Z,
    TO,
    PD,
}

/// Ciclos de una instruccion (nota 3 de la tabla 15-2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cycles {
    One,
    /// 1 ciclo, o 2 si la prueba se cumple (DECFSZ, INCFSZ, BTFSC, BTFSS).
    OneOrTwo,
    Two,
}

fn file(f: u8) -> Result<u16, EncodeError> {
    if f > 0x7F {
        Err(EncodeError::FileOutOfRange(f))
    } else {
        Ok(f as u16)
    }
}

fn fd(base: u16, f: u8, d: Dest) -> Result<u16, EncodeError> {
    Ok(base | ((d == Dest::F) as u16) << 7 | file(f)?)
}

fn bit(base: u16, f: u8, b: u8) -> Result<u16, EncodeError> {
    if b > 7 {
        return Err(EncodeError::BitOutOfRange(b));
    }
    Ok(base | (b as u16) << 7 | file(f)?)
}

fn addr(base: u16, k: u16) -> Result<u16, EncodeError> {
    if k > 0x7FF {
        Err(EncodeError::AddressOutOfRange(k))
    } else {
        Ok(base | k)
    }
}

impl Inst {
    /// Palabra de 14 bits en su forma canonica.
    pub fn encode(&self) -> Result<u16, EncodeError> {
        use Inst::*;
        match *self {
            Addwf(f, d) => fd(0x0700, f, d),
            Andwf(f, d) => fd(0x0500, f, d),
            Comf(f, d) => fd(0x0900, f, d),
            Decf(f, d) => fd(0x0300, f, d),
            Decfsz(f, d) => fd(0x0B00, f, d),
            Incf(f, d) => fd(0x0A00, f, d),
            Incfsz(f, d) => fd(0x0F00, f, d),
            Iorwf(f, d) => fd(0x0400, f, d),
            Movf(f, d) => fd(0x0800, f, d),
            Rlf(f, d) => fd(0x0D00, f, d),
            Rrf(f, d) => fd(0x0C00, f, d),
            Subwf(f, d) => fd(0x0200, f, d),
            Swapf(f, d) => fd(0x0E00, f, d),
            Xorwf(f, d) => fd(0x0600, f, d),
            Clrf(f) => Ok(0x0180 | file(f)?),
            Movwf(f) => Ok(0x0080 | file(f)?),
            Bcf(f, b) => bit(0x1000, f, b),
            Bsf(f, b) => bit(0x1400, f, b),
            Btfsc(f, b) => bit(0x1800, f, b),
            Btfss(f, b) => bit(0x1C00, f, b),
            Addlw(k) => Ok(0x3E00 | k as u16),
            Andlw(k) => Ok(0x3900 | k as u16),
            Iorlw(k) => Ok(0x3800 | k as u16),
            Movlw(k) => Ok(0x3000 | k as u16),
            Retlw(k) => Ok(0x3400 | k as u16),
            Sublw(k) => Ok(0x3C00 | k as u16),
            Xorlw(k) => Ok(0x3A00 | k as u16),
            Call(k) => addr(0x2000, k),
            Goto(k) => addr(0x2800, k),
            Clrw => Ok(0x0100),
            Clrwdt => Ok(0x0064),
            Nop => Ok(0x0000),
            Retfie => Ok(0x0009),
            Return => Ok(0x0008),
            Sleep => Ok(0x0063),
        }
    }

    /// Decodifica una palabra de 14 bits; `None` si no es una instruccion valida.
    pub fn decode(w: u16) -> Option<Inst> {
        use Inst::*;
        if w > 0x3FFF {
            return None;
        }
        let f = (w & 0x7F) as u8;
        let d = if w & 0x80 != 0 { Dest::F } else { Dest::W };
        let k = (w & 0xFF) as u8;
        Some(match w >> 12 {
            0 => match w {
                0x0008 => Return,
                0x0009 => Retfie,
                0x0063 => Sleep,
                0x0064 => Clrwdt,
                _ if w & 0x3F9F == 0 => Nop, // 00 0000 0xx0 0000
                _ if w & 0x3F80 == 0x0100 => Clrw,
                _ if w & 0x3F80 == 0x0180 => Clrf(f),
                _ if w & 0x3F80 == 0x0080 => Movwf(f),
                _ => match (w >> 8) & 0xF {
                    0x2 => Subwf(f, d),
                    0x3 => Decf(f, d),
                    0x4 => Iorwf(f, d),
                    0x5 => Andwf(f, d),
                    0x6 => Xorwf(f, d),
                    0x7 => Addwf(f, d),
                    0x8 => Movf(f, d),
                    0x9 => Comf(f, d),
                    0xA => Incf(f, d),
                    0xB => Decfsz(f, d),
                    0xC => Rrf(f, d),
                    0xD => Rlf(f, d),
                    0xE => Swapf(f, d),
                    0xF => Incfsz(f, d),
                    _ => return None,
                },
            },
            1 => {
                let b = ((w >> 7) & 7) as u8;
                match (w >> 10) & 3 {
                    0 => Bcf(f, b),
                    1 => Bsf(f, b),
                    2 => Btfsc(f, b),
                    _ => Btfss(f, b),
                }
            }
            2 => {
                if w & 0x0800 == 0 {
                    Call(w & 0x7FF)
                } else {
                    Goto(w & 0x7FF)
                }
            }
            _ => match (w >> 8) & 0xF {
                0x0..=0x3 => Movlw(k),
                0x4..=0x7 => Retlw(k),
                0x8 => Iorlw(k),
                0x9 => Andlw(k),
                0xA => Xorlw(k),
                0xB => return None,
                0xC | 0xD => Sublw(k),
                _ => Addlw(k),
            },
        })
    }

    pub fn mnemonic(&self) -> &'static str {
        use Inst::*;
        match self {
            Addwf(..) => "ADDWF",
            Andwf(..) => "ANDWF",
            Comf(..) => "COMF",
            Decf(..) => "DECF",
            Decfsz(..) => "DECFSZ",
            Incf(..) => "INCF",
            Incfsz(..) => "INCFSZ",
            Iorwf(..) => "IORWF",
            Movf(..) => "MOVF",
            Rlf(..) => "RLF",
            Rrf(..) => "RRF",
            Subwf(..) => "SUBWF",
            Swapf(..) => "SWAPF",
            Xorwf(..) => "XORWF",
            Clrf(..) => "CLRF",
            Movwf(..) => "MOVWF",
            Bcf(..) => "BCF",
            Bsf(..) => "BSF",
            Btfsc(..) => "BTFSC",
            Btfss(..) => "BTFSS",
            Addlw(..) => "ADDLW",
            Andlw(..) => "ANDLW",
            Iorlw(..) => "IORLW",
            Movlw(..) => "MOVLW",
            Retlw(..) => "RETLW",
            Sublw(..) => "SUBLW",
            Xorlw(..) => "XORLW",
            Call(..) => "CALL",
            Goto(..) => "GOTO",
            Clrw => "CLRW",
            Clrwdt => "CLRWDT",
            Nop => "NOP",
            Retfie => "RETFIE",
            Return => "RETURN",
            Sleep => "SLEEP",
        }
    }

    pub fn cycles(&self) -> Cycles {
        use Inst::*;
        match self {
            Decfsz(..) | Incfsz(..) | Btfsc(..) | Btfss(..) => Cycles::OneOrTwo,
            Call(_) | Goto(_) | Retfie | Return | Retlw(_) => Cycles::Two,
            _ => Cycles::One,
        }
    }

    /// Bits de STATUS afectados (columna "Status Affected" de la tabla 15-2).
    /// Nota: una escritura directa a STATUS/PCL tiene efectos adicionales (ver ABI).
    pub fn status_affected(&self) -> &'static [Flag] {
        use Flag::*;
        use Inst::*;
        match self {
            Addwf(..) | Subwf(..) | Addlw(_) | Sublw(_) => &[C, DC, Z],
            Andwf(..) | Clrf(_) | Clrw | Comf(..) | Decf(..) | Incf(..) | Iorwf(..) | Movf(..) | Xorwf(..)
            | Andlw(_) | Iorlw(_) | Xorlw(_) => &[Z],
            Rlf(..) | Rrf(..) => &[C],
            Clrwdt | Sleep => &[TO, PD],
            _ => &[],
        }
    }
}

impl fmt::Display for Inst {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use Inst::*;
        let m = self.mnemonic();
        match *self {
            Addwf(r, d) | Andwf(r, d) | Comf(r, d) | Decf(r, d) | Decfsz(r, d) | Incf(r, d) | Incfsz(r, d)
            | Iorwf(r, d) | Movf(r, d) | Rlf(r, d) | Rrf(r, d) | Subwf(r, d) | Swapf(r, d) | Xorwf(r, d) => {
                write!(f, "{m} {r:#04x},{}", if d == Dest::W { "W" } else { "F" })
            }
            Clrf(r) | Movwf(r) => write!(f, "{m} {r:#04x}"),
            Bcf(r, b) | Bsf(r, b) | Btfsc(r, b) | Btfss(r, b) => write!(f, "{m} {r:#04x},{b}"),
            Addlw(k) | Andlw(k) | Iorlw(k) | Movlw(k) | Retlw(k) | Sublw(k) | Xorlw(k) => write!(f, "{m} {k:#04x}"),
            Call(k) | Goto(k) => write!(f, "{m} {k:#06x}"),
            Clrw | Clrwdt | Nop | Retfie | Return | Sleep => f.write_str(m),
        }
    }
}

/// Desensambla `words` empezando en la direccion `origin` (en palabras).
/// Las palabras que no son instruccion se muestran como `DW`.
pub fn disassemble(words: &[u16], origin: u32) -> String {
    let mut out = String::new();
    for (i, &w) in words.iter().enumerate() {
        let text = match Inst::decode(w) {
            Some(inst) => inst.to_string(),
            None => format!("DW {w:#06x}"),
        };
        out.push_str(&format!("{:04X}: {:04X}  {text}\n", origin + i as u32, w));
    }
    out
}

/// Expresa una palabra como instrucción PIC-AS sólo si su codificación es exacta.
/// Las codificaciones alternativas y palabras desconocidas se conservan con DW.
pub fn pic_as_word(word: u16) -> String {
    match Inst::decode(word) {
        Some(inst) if inst.encode().ok() == Some(word) => inst.to_string(),
        _ => format!("DW {word:#06x}"),
    }
}
