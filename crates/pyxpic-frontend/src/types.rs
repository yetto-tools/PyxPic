//! Tipos del lenguaje PyxPic.
//!
//! Los enteros son de ancho fijo y portables. El frontend no asume el ancho de
//! palabra de ningun MCU (PIC16, PIC18, PIC24, dsPIC, PIC32...): que tipos
//! admite cada destino lo decide el `TargetProfile`.

use std::fmt;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Type {
    Int {
        bits: u8,
        signed: bool,
    },
    Bool,
    Void,
    /// Literal entero que aun no tomo tipo: lo toma del contexto si su valor cabe.
    Literal,
    /// Constante decimal no entera (`0.5`, `2.54`): un racional exacto que solo existe al compilar.
    Decimal,
    /// Punto fijo: un entero de `bits` bits (con o sin signo) que representa `raw / factor`; `factor` es 10^scale (`Fixed`) o 2^scale
    /// (`QFixed`, `binary`). Solo existe en el verificador: la IR y el PIC ven el entero (`to_ir`).
    Fixed { bits: u8, signed: bool, scale: u8, binary: bool },
    /// Marcador para suprimir errores en cascada.
    Error,
}

pub const U8: Type = Type::Int { bits: 8, signed: false };
pub const I8: Type = Type::Int { bits: 8, signed: true };
pub const U16: Type = Type::Int { bits: 16, signed: false };
pub const I16: Type = Type::Int { bits: 16, signed: true };
pub const U32: Type = Type::Int { bits: 32, signed: false };
pub const I32: Type = Type::Int { bits: 32, signed: true };

/// Tipos enteros en orden de preferencia para inferir el tipo de `range(literales)`.
pub const INFERENCE_ORDER: [Type; 6] = [U8, U16, I8, I16, U32, I32];

impl Type {
    pub fn from_name(name: &str) -> Option<Type> {
        Some(match name {
            "uint8" | "u8" => U8,
            "int8" | "i8" => I8,
            "uint16" | "u16" => U16,
            "int16" | "i16" => I16,
            "uint32" | "u32" => U32,
            "int32" | "i32" => I32,
            "bool" => Type::Bool,
            _ => return None,
        })
    }

    pub fn is_int(self) -> bool {
        matches!(self, Type::Int { .. })
    }

    pub fn is_fixed(self) -> bool {
        matches!(self, Type::Fixed { .. })
    }

    /// El entero que guarda el tipo: el mismo para un entero y el `raw` de un punto fijo; otro tipo, sin cambio.
    pub fn to_ir(self) -> Type {
        match self {
            Type::Fixed { bits, signed, .. } => Type::Int { bits, signed },
            t => t,
        }
    }

    /// El factor de escala de un punto fijo: 10^scale o 2^scale; 1 para los demas.
    pub fn factor(self) -> i128 {
        match self {
            Type::Fixed { scale, binary: true, .. } => 1i128 << scale,
            Type::Fixed { scale, binary: false, .. } => 10i128.pow(scale as u32),
            _ => 1,
        }
    }

    pub fn bits(self) -> u32 {
        match self.to_ir() {
            Type::Int { bits, .. } => bits as u32,
            _ => 0,
        }
    }

    pub fn is_signed(self) -> bool {
        matches!(self.to_ir(), Type::Int { signed: true, .. })
    }

    pub fn min(self) -> i128 {
        match self.to_ir() {
            Type::Int { bits, signed: true } => -(1i128 << (bits - 1)),
            _ => 0,
        }
    }

    pub fn max(self) -> i128 {
        match self.to_ir() {
            Type::Int { bits, signed: true } => (1i128 << (bits - 1)) - 1,
            Type::Int { bits, signed: false } => (1i128 << bits) - 1,
            _ => 0,
        }
    }

    pub fn contains(self, v: i128) -> bool {
        (self.is_int() || self.is_fixed()) && self.min() <= v && v <= self.max()
    }

    /// Reduce `v` al rango del tipo (aritmetica modular, como el hardware).
    pub fn wrap(self, v: i128) -> i128 {
        match self {
            Type::Int { bits, signed } => {
                let m = (1i128 << bits) - 1;
                let mut r = v & m;
                if signed && r >= 1i128 << (bits - 1) {
                    r -= 1i128 << bits;
                }
                r
            }
            _ => v,
        }
    }
}

impl fmt::Display for Type {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Type::Int { bits, signed } => write!(f, "{}{}", if *signed { "int" } else { "uint" }, bits),
            Type::Bool => f.write_str("bool"),
            Type::Void => f.write_str("None"),
            Type::Literal => f.write_str("int literal"),
            Type::Decimal => f.write_str("decimal"),
            Type::Fixed { bits, signed, scale, binary } => write!(f, "{}[{}{}, {}]", if *binary { "QFixed" } else { "Fixed" }, if *signed { "int" } else { "uint" }, bits, scale),
            Type::Error => f.write_str("<error>"),
        }
    }
}
