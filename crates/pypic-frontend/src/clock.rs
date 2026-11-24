//! Fuente y frecuencia del reloj (`CLOCK: Internal[4 * MHz]`, `Crystal[20 * MHz]`, `ExternalClock[20 * MHz]`): lo que cada una
//! significa en un PIC16F887 y los limites del datasheet.
//!
//! Fuente: DS40001291H, cap. 4 y tabla 17-1 (p.249, leida de la imagen de la pagina):
//!
//! | Modo | Con cristal o resonador | Con reloj externo en CLKIN |
//! |---|---|---|
//! | LP | 32,768 kHz (tipico) | DC a 37 kHz |
//! | XT | 0,1 a 4 MHz | DC a 4 MHz |
//! | HS | 1 a 20 MHz | DC a 20 MHz |
//! | EC | - | DC a 20 MHz |
//!
//! El oscilador interno (OSCCON, registro 4-1, p.64) da 8 MHz, 4 MHz (el valor tras el reset), 2 MHz, 1 MHz, 500 kHz, 250 kHz y
//! 125 kHz con HFINTOSC y 31 kHz con LFINTOSC, segun IRCF<2:0>.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClockKind {
    /// Oscilador interno (HFINTOSC / LFINTOSC).
    Internal,
    /// Cristal o resonador entre OSC1 y OSC2.
    Crystal,
    /// Una senal de reloj externa en CLKIN.
    ExternalClock,
}

impl ClockKind {
    pub fn name(self) -> &'static str {
        match self {
            ClockKind::Internal => "Internal",
            ClockKind::Crystal => "Crystal",
            ClockKind::ExternalClock => "ExternalClock",
        }
    }
}

/// El reloj resuelto: lo que el compilador pone en la palabra de configuracion y en OSCCON.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClockSource {
    pub kind: ClockKind,
    pub hz: u32,
    /// Nombre del valor de `FOSC` en el Device Pack (`XT`, `HS`, `INTRC_NOCLKOUT`...).
    pub fosc: &'static str,
    /// `IRCF<2:0>` del oscilador interno; `None` si el reloj es externo.
    pub ircf: Option<u8>,
    /// Pines que el oscilador toma: (pin, funcion). No pueden ser E/S del programa.
    pub pins: &'static [(&'static str, &'static str)],
}

/// `IRCF<2:0>` y su frecuencia (registro 4-1). 31 kHz es el LFINTOSC nominal.
pub const INTERNAL: &[(u32, u8)] = &[(31_000, 0), (125_000, 1), (250_000, 2), (500_000, 3), (1_000_000, 4), (2_000_000, 5), (4_000_000, 6), (8_000_000, 7)];
/// El valor de `IRCF` tras el reset: 4 MHz.
pub const IRCF_RESET: u8 = 6;

const CRYSTAL_PINS: &[(&str, &str)] = &[("RA7", "OSC1"), ("RA6", "OSC2")];
const EC_PINS: &[(&str, &str)] = &[("RA7", "CLKIN")];

fn fmt_hz(hz: u32) -> String {
    if hz >= 1_000_000 && hz % 100_000 == 0 {
        format!("{} MHz", hz as f64 / 1e6)
    } else if hz >= 1_000 && hz % 100 == 0 {
        format!("{} kHz", hz as f64 / 1e3)
    } else {
        format!("{hz} Hz")
    }
}

/// Resuelve la fuente y la frecuencia; el error dice cuales valen.
pub fn resolve(kind: ClockKind, hz: u32) -> Result<ClockSource, String> {
    match kind {
        ClockKind::Internal => match INTERNAL.iter().find(|(f, _)| *f == hz) {
            Some(&(_, ircf)) => Ok(ClockSource { kind, hz, fosc: "INTRC_NOCLKOUT", ircf: Some(ircf), pins: &[] }),
            None => {
                let list: Vec<String> = INTERNAL.iter().map(|(f, _)| fmt_hz(*f)).collect();
                Err(format!("el oscilador interno del PIC16F887 da {} (IRCF de OSCCON, DS40001291H p.64), no {}; para otra frecuencia usa un cristal (Crystal[...]) o un reloj externo (ExternalClock[...])", list.join(", "), fmt_hz(hz)))
            }
        },
        ClockKind::Crystal => {
            let (fosc, ok) = match hz {
                30_000..=37_000 => ("LP", true),
                100_000..=4_000_000 => ("XT", true),
                4_000_001..=20_000_000 => ("HS", true),
                _ => ("", false),
            };
            if !ok {
                return Err(format!(
                    "un cristal de {} no cabe en ningun modo del PIC16F887 (tabla 17-1, p.249): LP es el cristal de 32,768 kHz, XT va de 0,1 a 4 MHz y HS de 1 a 20 MHz",
                    fmt_hz(hz)
                ));
            }
            Ok(ClockSource { kind, hz, fosc, ircf: None, pins: CRYSTAL_PINS })
        }
        ClockKind::ExternalClock => {
            if !(1..=20_000_000).contains(&hz) {
                return Err(format!("un reloj externo (modo EC) va de continua a 20 MHz (tabla 17-1, p.249), no {}", fmt_hz(hz)));
            }
            Ok(ClockSource { kind, hz, fosc: "EC", ircf: None, pins: EC_PINS })
        }
    }
}

impl ClockSource {
    /// `Internal 8 MHz (FOSC = INTRC_NOCLKOUT, OSCCON = 0x70)`.
    pub fn describe(&self) -> String {
        let extra = match self.ircf {
            Some(i) => format!(", OSCCON = {:#04x}", i << 4),
            None => String::new(),
        };
        format!("{} {} (FOSC = {}{extra})", self.kind.name(), fmt_hz(self.hz), self.fosc)
    }
}
