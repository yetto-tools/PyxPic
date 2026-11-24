//! Convertidor analogico-digital (`pypic.periph.adc`): tipos compartidos entre el checker, el IR y el backend.
//!
//! Fuente: DS40001291H cap. 9 (p.99-108) y 17 (parametros AD130-AD132): reloj de conversion FOSC/2, FOSC/8, FOSC/32 o RC interno;
//! TAD entre 1,6 y 6,0 us; adquisicion de 5 us como minimo; 11 TAD por conversion de 10 bits.

/// Reloj de conversion (`ADCS<1:0>`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AdcClock {
    Fosc2 = 0,
    Fosc8 = 1,
    Fosc32 = 2,
    /// Oscilador RC propio del ADC: TAD tipico de 4 us.
    Frc = 3,
}

impl AdcClock {
    pub fn divisor(self) -> Option<u32> {
        match self {
            AdcClock::Fosc2 => Some(2),
            AdcClock::Fosc8 => Some(8),
            AdcClock::Fosc32 => Some(32),
            AdcClock::Frc => None,
        }
    }
}

/// TAD minimo y maximo para un reloj derivado de FOSC (AD130, p.15244): 1,6 a 6,0 us.
pub const TAD_MIN_NS: u64 = 1600;
pub const TAD_MAX_NS: u64 = 6000;
/// Tiempo de adquisicion minimo tras cambiar de canal (AD132 y ecuacion 9-1, p.107): 5 us.
pub const TACQ_NS: u64 = 5000;

/// TAD en nanosegundos de `clock` a `fosc_hz`.
pub fn tad_ns(clock: AdcClock, fosc_hz: u32) -> f64 {
    match clock.divisor() {
        Some(d) => d as f64 * 1e9 / fosc_hz as f64,
        None => 4000.0,
    }
}

/// El reloj mas rapido cuyo TAD cae en 1,6..6,0 us; si ninguno (FOSC muy bajo o muy alto), el RC interno.
pub fn auto_clock(fosc_hz: u32) -> AdcClock {
    [AdcClock::Fosc2, AdcClock::Fosc8, AdcClock::Fosc32]
        .into_iter()
        .find(|c| {
            let t = tad_ns(*c, fosc_hz);
            t >= TAD_MIN_NS as f64 && t <= TAD_MAX_NS as f64
        })
        .unwrap_or(AdcClock::Frc)
}

/// Ciclos de instruccion que cubren el tiempo de adquisicion: ceil(5 us / TCY).
pub fn acquisition_cycles(fosc_hz: u32, clocks_per_cycle: u32) -> u64 {
    let cycle_ns = clocks_per_cycle as f64 * 1e9 / fosc_hz as f64;
    ((TACQ_NS as f64 / cycle_ns) - 1e-9).ceil().max(1.0) as u64
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AdcOp {
    /// `ADC.start()`: ADCON1 (a la derecha, VDD/VSS) y ADCON0 (reloj, canal 0, ADON). `None` hasta resolver el reloj automatico.
    Start(Option<AdcClock>),
    /// `ADC.select(ANn)`: canal en CHS, pin como entrada analogica.
    Select(u8),
    /// `ADC.go()`: `GO/DONE = 1`.
    Go,
    /// `ADC.done()`: la conversion termino.
    Done,
    /// `ADC.result()`: `ADRESH:ADRESL` justificado a la derecha.
    Result,
    /// `ADC.read(ANn)`: select + adquisicion + go + espera + result.
    Read(u8),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AdcCall {
    pub op: AdcOp,
}

/// Canales de entrada (`AN0`..`AN13`).
pub const CHANNELS: u8 = 14;

/// Constantes de reloj que exporta `pypic.periph.adc`.
pub const CLOCKS: &[(&str, AdcClock)] = &[("FOSC_2", AdcClock::Fosc2), ("FOSC_8", AdcClock::Fosc8), ("FOSC_32", AdcClock::Fosc32), ("FRC", AdcClock::Frc)];

pub fn channel(name: &str) -> Option<u8> {
    name.strip_prefix("AN").and_then(|n| n.parse::<u8>().ok()).filter(|n| *n < CHANNELS && name == format!("AN{n}"))
}
