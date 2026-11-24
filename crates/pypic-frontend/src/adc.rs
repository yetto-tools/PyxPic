//! Convertidor analogico-digital (`pypic.periph.adc`): tipos compartidos entre el checker, el IR y el backend.
//!
//! Fuente: DS40001291H cap. 9 (p.99-108) y la tabla 17-11 (p.258, parametros AD130-AD134), leida de la pagina del PDF.
//!
//! **Condiciones electricas que asume PyPIC** (el compilador no conoce el circuito: estas son sus suposiciones y se pueden cambiar):
//!  * La referencia del ADC es VDD (no hay referencias externas) y **VDD > 3,0 V** (condiciones de las tablas 9-1 y 17-18).
//!    Con `low_voltage=True` se asume VDD < 3,0 V (VREF "rango completo" de la tabla 17-11).
//!  * **TAD** (AD130): 1,6 us minimo con VREF >= 3,0 V y 3,0 us con VREF de rango completo; **9,0 us maximo**. Las tablas 9-1/17-18
//!    recomiendan 1,6 a 6,0 us con VDD > 3,0 V: el reloj automatico prefiere esa ventana y solo sale de ella si no hay otra opcion.
//!  * **Adquisicion** (AD132): el datasheet no da un minimo universal, solo el valor tipico de 11,5 us (5 V, 25 C) y la ecuacion 9-1
//!    (4,67 us con 10 kOhm de fuente, 50 C y 5 V; usa TAMP, cuyo maximo es 5 us, AD133). Depende de la impedancia de la fuente, la
//!    tension y la temperatura, asi que se parte del valor tipico y se puede fijar con `acquisition_us=`.

/// Reloj de conversion (`ADCS<1:0>`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AdcClock {
    Fosc2 = 0,
    Fosc8 = 1,
    Fosc32 = 2,
    /// Oscilador RC propio del ADC (AD130: TAD tipico de 4 us a 5 V y 6 us a 2,5 V).
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

/// Condiciones que asume el programa: tiempo de adquisicion y si la alimentacion es menor de 3,0 V.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AdcConfig {
    pub acq_ns: u32,
    pub low_voltage: bool,
}

/// Valor tipico de TACQ (AD132, 5 V y 25 C): el que se usa si el programa no dice otro.
pub const DEFAULT_TACQ_NS: u32 = 11_500;
/// TAMP maximo (AD133): una adquisicion menor que esto no puede cumplir el amplificador en el peor caso.
pub const TAMP_MAX_NS: u32 = 5_000;
/// TAD maximo (AD130) con cualquier tension.
pub const TAD_MAX_NS: u64 = 9_000;

impl Default for AdcConfig {
    fn default() -> Self {
        AdcConfig { acq_ns: DEFAULT_TACQ_NS, low_voltage: false }
    }
}

impl AdcConfig {
    /// TAD minimo de AD130: 1,6 us con VREF >= 3,0 V y 3,0 us con VREF de rango completo.
    pub fn tad_min_ns(&self) -> u64 {
        if self.low_voltage { 3_000 } else { 1_600 }
    }

    /// Ventana recomendada (tablas 9-1 y 17-18, VDD > 3,0 V): hasta 6,0 us; con VDD baja, hasta el maximo de 9,0 us.
    pub fn tad_recommended_max_ns(&self) -> u64 {
        if self.low_voltage { TAD_MAX_NS } else { 6_000 }
    }
}

/// TAD en nanosegundos de `clock` a `fosc_hz`.
pub fn tad_ns(clock: AdcClock, fosc_hz: u32) -> f64 {
    match clock.divisor() {
        Some(d) => d as f64 * 1e9 / fosc_hz as f64,
        None => 4000.0,
    }
}

/// TAD de un reloj derivado de FOSC dentro de lo que garantiza AD130 (minimo y maximo de 9,0 us).
pub fn tad_in_spec(clock: AdcClock, fosc_hz: u32, cfg: &AdcConfig) -> bool {
    let t = tad_ns(clock, fosc_hz);
    clock.divisor().is_none() || (t >= cfg.tad_min_ns() as f64 && t <= TAD_MAX_NS as f64)
}

/// El reloj mas rapido con TAD en la ventana recomendada; si no hay, el mas rapido que cumple AD130; si tampoco, el RC interno.
pub fn auto_clock(fosc_hz: u32, cfg: &AdcConfig) -> AdcClock {
    let fosc_based = [AdcClock::Fosc2, AdcClock::Fosc8, AdcClock::Fosc32];
    let recommended = |c: &AdcClock| {
        let t = tad_ns(*c, fosc_hz);
        t >= cfg.tad_min_ns() as f64 && t <= cfg.tad_recommended_max_ns() as f64
    };
    fosc_based.into_iter().find(recommended).or_else(|| fosc_based.into_iter().find(|c| tad_in_spec(*c, fosc_hz, cfg))).unwrap_or(AdcClock::Frc)
}

/// Ciclos de instruccion que cubren `acq_ns`: ceil(acq / TCY), al menos uno.
pub fn acquisition_cycles(fosc_hz: u32, clocks_per_cycle: u32, acq_ns: u32) -> u64 {
    let cycle_ns = clocks_per_cycle as f64 * 1e9 / fosc_hz as f64;
    ((acq_ns as f64 / cycle_ns) - 1e-9).ceil().max(1.0) as u64
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
