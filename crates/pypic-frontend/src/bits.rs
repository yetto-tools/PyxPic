//! Intrinsecos de bits (`pypic.bits`): tipos compartidos entre el checker, el IR y el backend.
//!
//! Cada uno mapea 1:1 a una instruccion del PIC16: `swap` = `SWAPF`, `rol_c` = `RLF`, `ror_c` = `RRF`,
//! `clear_carry`/`set_carry` = `BCF`/`BSF STATUS,C`, `bit_set`/`bit_clear` = `BSF`/`BCF` sobre la variable y
//! `bit_test` = `BTFSC` + guardar el resultado.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BitsOp {
    /// `swap(x)`: intercambia los nibbles (u8).
    Swap,
    /// `rol_c(x)`: rota a la izquierda por el acarreo; el bit 7 pasa a C y C entra por el bit 0.
    RolC,
    /// `ror_c(x)`: rota a la derecha por el acarreo; el bit 0 pasa a C y C entra por el bit 7.
    RorC,
    ClearCarry,
    SetCarry,
    /// `carry()`: el valor de C como bool.
    Carry,
    BitSet(u8),
    BitClear(u8),
    BitTest(u8),
}

/// Funciones que exporta `pypic.bits`.
pub const FUNCTIONS: &[&str] = &["swap", "rol_c", "ror_c", "clear_carry", "set_carry", "carry", "bit_set", "bit_clear", "bit_test"];

impl BitsOp {
    /// Deja el acarreo definido para la sentencia siguiente (`rol_c`, `ror_c`, `clear_carry`, `set_carry`).
    pub fn defines_carry(self) -> bool {
        matches!(self, BitsOp::RolC | BitsOp::RorC | BitsOp::ClearCarry | BitsOp::SetCarry)
    }

    /// Usa el acarreo que dejo la sentencia anterior.
    pub fn uses_carry(self) -> bool {
        matches!(self, BitsOp::RolC | BitsOp::RorC | BitsOp::Carry)
    }

    pub fn returns_value(self) -> bool {
        matches!(self, BitsOp::Swap | BitsOp::RolC | BitsOp::RorC | BitsOp::Carry | BitsOp::BitTest(_))
    }
}
