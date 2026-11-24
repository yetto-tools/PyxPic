//! Acceso a registros (`pypic.regs`): tipos compartidos entre el checker, el IR y el backend.
//!
//! `STATUS.read()`, `OPTION_REG.write(v)` y, sobre un bit con nombre, `STATUS.RP0.set()`: cada llamada se
//! traduce a las instrucciones del ensamblador (`MOVF`, `MOVWF`, `BSF`, `BCF`, `XORWF`) con el banco que
//! pida la direccion del registro.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RegOp {
    /// `REG.read()`: el byte entero (u8).
    Read,
    /// `REG.write(v)`: escribe el byte entero.
    Write,
    /// `REG.BIT.set()`: `BSF`.
    Set,
    /// `REG.BIT.clear()`: `BCF`.
    Clear,
    /// `REG.BIT.toggle()`: `XORWF` con la mascara del bit.
    Toggle,
    /// `REG.BIT.value()`: el bit como u8 (0 o 1).
    Value,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RegCall {
    pub reg: String,
    /// Direccion de 9 bits (banco incluido) de la primera aparicion del registro.
    pub addr: u16,
    /// Numero de bit en las operaciones sobre un bit.
    pub bit: Option<u8>,
    pub op: RegOp,
}

/// Bits que el compilador gestiona (selector de banco): tocarlos rompe la convencion de que el codigo
/// de usuario corre en el banco 0.
pub const BANK_BITS: [&str; 3] = ["RP0", "RP1", "IRP"];

/// Registros que no se escriben enteros: los usa el compilador (`STATUS`, `PCLATH`) o mueven el PC (`PCL`).
pub const WRITE_PROTECTED: [&str; 4] = ["STATUS", "PCL", "PCLATH", "INDF"];
