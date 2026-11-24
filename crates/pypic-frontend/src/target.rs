//! Perfil de destino visto por el frontend.
//!
//! PyPIC no es solo para PIC de 8 bits: cada familia/MCU tiene su ancho de
//! palabra y capacidades. El frontend solo consulta este perfil; mas adelante
//! lo construira el Device Pack del dispositivo elegido.

use std::collections::BTreeMap;

use crate::types::Type;

/// Pin de E/S de un dispositivo (lo llena el Device Pack).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PinInfo {
    pub port: char,
    pub bit: u8,
    /// Solo entrada (p. ej. RE3/MCLR en el PIC16F887).
    pub input_only: bool,
    /// Canal analogico (`ANn`) del pin, si lo tiene: hay que pasarlo a digital para usarlo como E/S.
    pub analog: Option<u8>,
}

#[derive(Clone, Debug)]
pub struct TargetProfile {
    pub name: String,
    /// Ancho de dato nativo de la CPU (8 en PIC16, 16 en PIC24/dsPIC, 32 en PIC32...).
    /// Los enteros mas anchos se emulan por software: se permiten con aviso de coste.
    pub native_bits: u32,
    /// Ancho maximo de entero que el destino admite (nativo o emulado). Mas alla es error.
    pub max_int_bits: u32,
    /// Niveles de la pila de retorno de hardware (8 en PIC16). `None` = sin limite conocido.
    pub stack_levels: Option<u32>,
    /// Pines de E/S por nombre (`RD0`...). `None` = sin validar (solo se comprueba la sintaxis del nombre).
    pub pins: Option<BTreeMap<String, PinInfo>>,
    /// Nombre del dispositivo (`PIC16F887`) cuando el perfil viene de un Device Pack.
    pub device: Option<String>,
    /// Reloj maximo del dispositivo, en Hz.
    pub max_clock_hz: Option<u32>,
    /// Relojes por ciclo de instruccion (4 en PIC16): necesario para convertir tiempo en ciclos.
    pub clocks_per_cycle: Option<u32>,
    /// Reloj del dispositivo tras el reset con la configuracion por defecto (p. ej. 4 MHz con el oscilador interno).
    pub default_clock_hz: Option<u32>,
}

impl TargetProfile {
    pub fn new(name: impl Into<String>, native_bits: u32, max_int_bits: u32) -> Self {
        TargetProfile { name: name.into(), native_bits, max_int_bits, stack_levels: None, pins: None, device: None, max_clock_hz: None, clocks_per_cycle: None, default_clock_hz: None }
    }

    pub fn with_device(mut self, name: impl Into<String>, max_clock_hz: u32) -> Self {
        self.device = Some(name.into());
        self.max_clock_hz = Some(max_clock_hz);
        self
    }

    pub fn with_clocks_per_cycle(mut self, n: u32) -> Self {
        self.clocks_per_cycle = Some(n);
        self
    }

    pub fn with_default_clock(mut self, hz: u32) -> Self {
        self.default_clock_hz = Some(hz);
        self
    }

    pub fn with_pins(mut self, pins: BTreeMap<String, PinInfo>) -> Self {
        self.pins = Some(pins);
        self
    }

    pub fn with_stack_levels(mut self, levels: u32) -> Self {
        self.stack_levels = Some(levels);
        self
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
