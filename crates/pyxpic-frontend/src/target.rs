//! Perfil de destino visto por el frontend.
//!
//! PyxPic no es solo para PIC de 8 bits: cada familia/MCU tiene su ancho de
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

/// Registro de funciones especiales (lo llena `registers.json`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RegInfo {
    /// Primera direccion de 9 bits (banco incluido).
    pub addr: u16,
    /// Nombre de cada bit (del 0 al 7), `None` si no tiene.
    pub bits: Vec<Option<String>>,
    /// `false` en INDF y EECON2: no son registros fisicos.
    pub physical: bool,
    /// Comparte direccion con otro registro (SSPMSK/SSPADD): el nombre no basta para saber cual se toca.
    pub shared: bool,
}

/// Lo que un periferico toma al encender un bit: la funcion, el periferico y los pines (`RA4`) donde esta.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PinClaim {
    pub function: String,
    pub peripheral: String,
    pub pins: Vec<String>,
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
    /// Perifericos del dispositivo (`TIMER0`, `ADC`... de `peripherals.json`). `None` = sin validar.
    pub peripherals: Option<std::collections::BTreeSet<String>>,
    /// Registros por nombre (`STATUS`, `OPTION_REG`...). `None` = sin validar.
    pub registers: Option<BTreeMap<String, RegInfo>>,
    /// Bits que reclaman pines (`SPEN`, `C1OE`...), de `peripherals.json`. `None` = sin validar.
    pub pin_claims: Option<BTreeMap<String, Vec<PinClaim>>>,
    /// El reloj que pide el perfil de proyecto (`pyxpic.toml`): solo vale si el programa no declara el suyo (el fuente manda).
    pub project_clock: Option<crate::clock::ClockSource>,
}

impl TargetProfile {
    pub fn new(name: impl Into<String>, native_bits: u32, max_int_bits: u32) -> Self {
        TargetProfile {
            name: name.into(),
            native_bits,
            max_int_bits,
            stack_levels: None,
            pins: None,
            device: None,
            max_clock_hz: None,
            clocks_per_cycle: None,
            default_clock_hz: None,
            peripherals: None,
            registers: None,
            pin_claims: None,
            project_clock: None,
        }
    }

    pub fn with_pin_claims(mut self, c: BTreeMap<String, Vec<PinClaim>>) -> Self {
        self.pin_claims = Some(c);
        self
    }

    pub fn with_registers(mut self, regs: BTreeMap<String, RegInfo>) -> Self {
        self.registers = Some(regs);
        self
    }

    pub fn with_peripherals(mut self, names: impl IntoIterator<Item = String>) -> Self {
        self.peripherals = Some(names.into_iter().collect());
        self
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
