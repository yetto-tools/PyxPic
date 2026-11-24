//! Perfil de destino visto por el frontend.
//!
//! PyPIC no es solo para PIC de 8 bits: cada familia/MCU tiene su ancho de
//! palabra y capacidades. El frontend solo consulta este perfil; mas adelante
//! lo construira el Device Pack del dispositivo elegido.

use crate::types::Type;

#[derive(Clone, Debug)]
pub struct TargetProfile {
    pub name: String,
    /// Ancho de dato nativo de la CPU (8 en PIC16, 16 en PIC24/dsPIC, 32 en PIC32...).
    /// Los enteros mas anchos se emulan por software: se permiten con aviso de coste.
    pub native_bits: u32,
    /// Ancho maximo de entero que el destino admite (nativo o emulado). Mas alla es error.
    pub max_int_bits: u32,
}

impl TargetProfile {
    pub fn new(name: impl Into<String>, native_bits: u32, max_int_bits: u32) -> Self {
        TargetProfile { name: name.into(), native_bits, max_int_bits }
    }

    /// Destino sin restricciones (por defecto): todo es nativo hasta 32 bits.
    pub fn generic() -> Self {
        Self::new("generic", 32, 32)
    }

    /// El tipo existe en el destino (nativo o emulado).
    pub fn supports(&self, ty: Type) -> bool {
        !ty.is_int() || ty.bits() <= self.max_int_bits
    }

    /// El tipo existe pero se emula por software (mayor coste en flash y ciclos).
    pub fn is_emulated(&self, ty: Type) -> bool {
        ty.is_int() && self.supports(ty) && ty.bits() > self.native_bits
    }
}

impl Default for TargetProfile {
    fn default() -> Self {
        Self::generic()
    }
}
