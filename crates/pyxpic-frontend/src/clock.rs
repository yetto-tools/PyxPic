//! Fuente y frecuencia del reloj (`CLOCK: Internal`, `FREQUENCY: 8MHz`): lo que cada fuente significa en un dispositivo y sus limites.
//!
//! Los limites salen del perfil del dispositivo (`oscillator.json` del Device Pack, `TargetProfile::oscillator`); no hay una tabla
//! global para toda la familia PIC. Sin Device Pack (pruebas del frontend con un `TargetProfile` suelto) se usa la tabla del PIC16F887,
//! que es la del pack `devices/pic16f887`.
//!
//! PIC16F887, DS40001291H, cap. 4 y tabla 17-1 (p.249, leida de la imagen de la pagina):
//!
//! | Modo | Con cristal o resonador | Con reloj externo en CLKIN |
//! |---|---|---|
//! | LP | 32,768 kHz (tipico) | - |
//! | XT | 0,1 a 4 MHz | - |
//! | HS | 1 a 20 MHz | - |
//! | EC | - | DC a 20 MHz |
//!
//! El oscilador interno (OSCCON, registro 4-1, p.64) da 8 MHz, 4 MHz (el valor tras el reset), 2 MHz, 1 MHz, 500 kHz, 250 kHz y
//! 125 kHz con HFINTOSC y 31 kHz con LFINTOSC, segun IRCF<2:0>.

/// De donde viene el reloj del sistema.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClockKind {
    /// Oscilador interno (HFINTOSC / LFINTOSC).
    Internal,
    /// Cristal de cuarzo entre OSC1 y OSC2.
    Crystal,
    /// Una senal de reloj generada por otro circuito.
    External,
    /// Resonador ceramico entre OSC1 y OSC2.
    Resonator,
    /// Resistencia y capacitor externos.
    Rc,
    /// Oscilador secundario (Timer1).
    Secondary,
}

impl ClockKind {
    pub const ALL: [ClockKind; 6] = [ClockKind::Internal, ClockKind::Crystal, ClockKind::External, ClockKind::Resonator, ClockKind::Rc, ClockKind::Secondary];

    /// El nombre como se escribe en `CLOCK: ...`.
    pub fn name(self) -> &'static str {
        match self {
            ClockKind::Internal => "Internal",
            ClockKind::Crystal => "Crystal",
            ClockKind::External => "External",
            ClockKind::Resonator => "Resonator",
            ClockKind::Rc => "RC",
            ClockKind::Secondary => "Secondary",
        }
    }

    pub fn parse(s: &str) -> Option<ClockKind> {
        ClockKind::ALL.into_iter().find(|k| k.name() == s)
    }

    pub fn names() -> String {
        ClockKind::ALL.iter().map(|k| k.name()).collect::<Vec<_>>().join(", ")
    }
}

/// Un modo del oscilador externo del dispositivo (`LP`, `XT`, `HS`, `EC`, `RC`...).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OscillatorMode {
    pub source: ClockKind,
    pub mode: String,
    /// Valor de `FOSC` en el Device Pack.
    pub fosc: String,
    pub min_hz: u32,
    pub max_hz: u32,
    /// (pin, funcion) que el oscilador toma.
    pub pins: Vec<(String, String)>,
}

/// Lo que un dispositivo admite como reloj.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OscillatorProfile {
    /// `(Hz, IRCF)` de las frecuencias que el oscilador interno puede configurar por registro.
    pub internal: Vec<(u32, u8)>,
    /// `FOSC` con el oscilador interno; `None` si el dispositivo no lo tiene.
    pub internal_fosc: Option<String>,
    /// Frecuencia e `IRCF` tras el reset.
    pub reset_hz: u32,
    pub reset_ircf: u8,
    pub modes: Vec<OscillatorMode>,
    /// PLL del dispositivo; `None` si no tiene.
    pub pll: Option<PllProfile>,
    /// Divisores de frecuencia admitidos, con los ajustes de configuracion que los producen.
    pub dividers: Vec<(u32, Vec<(String, String)>)>,
    /// Salida de reloj (CLKOUT) por modo.
    pub clock_output: Vec<ClockOutput>,
    /// Monitor de fallas del oscilador (`FCMEN`); `None` si no tiene.
    pub fail_safe: Option<SwitchProfile>,
    /// Cambio entre fuentes de reloj (`IESO`, arranque en dos velocidades); `None` si no tiene.
    pub switching: Option<SwitchProfile>,
}

/// La PLL de un dispositivo: multiplicadores con los ajustes que los producen y la fuente y el rango de entrada que admite.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PllProfile {
    pub multipliers: Vec<(u32, Vec<(String, String)>)>,
    pub sources: Vec<ClockKind>,
    pub min_input_hz: u32,
    pub max_input_hz: u32,
}

/// Salida de reloj de un modo: el `FOSC` sin ella, con ella y el pin que ocupa.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClockOutput {
    pub mode: String,
    pub off: String,
    pub on: String,
    pub pin: (String, String),
}

/// Un bit de configuracion que habilita una funcion del oscilador y los modos donde aplica.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SwitchProfile {
    pub setting: String,
    pub modes: Vec<String>,
}

/// `PLL: Off`, `PLL: On` o `PLL: 4`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Toggle {
    Off,
    On,
    Mult(u32),
}

/// Los ajustes complementarios del reloj declarados junto a `CLOCK` (todos opcionales).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ClockSettings {
    pub pll: Option<Toggle>,
    /// `CLOCK_DIVIDER: 2`: `FOSC = SOURCE_FREQ * PLL / CLOCK_DIVIDER`.
    pub divider: Option<u32>,
    pub output: Option<bool>,
    pub fail_safe: Option<bool>,
    pub switching: Option<bool>,
}

impl OscillatorProfile {
    /// La tabla del PIC16F887 (la de `devices/pic16f887/oscillator.json`), para el frontend sin Device Pack.
    pub fn pic16f887() -> OscillatorProfile {
        let pins2 = || vec![("RA7".to_string(), "OSC1".to_string()), ("RA6".to_string(), "OSC2".to_string())];
        let mut modes = Vec::new();
        for (mode, min, max) in [("LP", 30_000, 37_000), ("XT", 100_000, 4_000_000), ("HS", 1_000_000, 20_000_000)] {
            for source in [ClockKind::Crystal, ClockKind::Resonator] {
                modes.push(OscillatorMode { source, mode: mode.into(), fosc: mode.into(), min_hz: min, max_hz: max, pins: pins2() });
            }
        }
        modes.push(OscillatorMode { source: ClockKind::External, mode: "EC".into(), fosc: "EC".into(), min_hz: 1, max_hz: 20_000_000, pins: vec![("RA7".into(), "CLKIN".into())] });
        modes.push(OscillatorMode { source: ClockKind::Rc, mode: "RC".into(), fosc: "EXTRC_NOCLKOUT".into(), min_hz: 1, max_hz: 20_000_000, pins: vec![("RA7".into(), "OSC1".into())] });
        let out = |mode: &str, off: &str, on: &str| ClockOutput { mode: mode.into(), off: off.into(), on: on.into(), pin: ("RA6".into(), "CLKOUT".into()) };
        let modes_of = |l: &[&str]| l.iter().map(|m| m.to_string()).collect::<Vec<_>>();
        OscillatorProfile {
            internal: INTERNAL.to_vec(),
            internal_fosc: Some("INTRC_NOCLKOUT".into()),
            reset_hz: INTERNAL_RESET_HZ,
            reset_ircf: IRCF_RESET,
            modes,
            pll: None,
            dividers: Vec::new(),
            clock_output: vec![out("INTOSC", "INTRC_NOCLKOUT", "INTRC_CLKOUT"), out("RC", "EXTRC_NOCLKOUT", "EXTRC_CLKOUT")],
            fail_safe: Some(SwitchProfile { setting: "FCMEN".into(), modes: modes_of(&["LP", "XT", "HS", "EC", "RC"]) }),
            switching: Some(SwitchProfile { setting: "IESO".into(), modes: modes_of(&["LP", "XT", "HS"]) }),
        }
    }
}

/// El reloj resuelto: lo que el compilador pone en la palabra de configuracion y en OSCCON.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClockSource {
    pub kind: ClockKind,
    /// Frecuencia de la fuente (`SOURCE_FREQ`).
    pub hz: u32,
    /// Frecuencia del sistema (`FOSC`): la de la fuente por la PLL entre el divisor.
    pub system_hz: u32,
    /// Nombre del modo del oscilador (`LP`, `XT`, `HS`, `EC`, `RC`; `INTOSC` con el interno).
    pub mode: String,
    /// Nombre del valor de `FOSC` en el Device Pack (`XT`, `HS`, `INTRC_NOCLKOUT`...).
    pub fosc: String,
    /// `IRCF<2:0>` del oscilador interno; `None` si el reloj es externo.
    pub ircf: Option<u8>,
    /// Pines que el oscilador toma: (pin, funcion). No pueden ser E/S del programa.
    pub pins: Vec<(String, String)>,
    /// Bits de configuracion que piden `PLL`, `CLOCK_DIVIDER`, `FAIL_SAFE_CLOCK` y `CLOCK_SWITCHING`: (nombre, valor) como en `--config`.
    pub extra_config: Vec<(String, String)>,
}

/// `IRCF<2:0>` y su frecuencia del PIC16F887 (registro 4-1). 31 kHz es el LFINTOSC nominal.
pub const INTERNAL: &[(u32, u8)] = &[(31_000, 0), (125_000, 1), (250_000, 2), (500_000, 3), (1_000_000, 4), (2_000_000, 5), (4_000_000, 6), (8_000_000, 7)];
/// El valor de `IRCF` tras el reset: 4 MHz.
pub const IRCF_RESET: u8 = 6;
/// La frecuencia del oscilador interno tras el reset: la de `CLOCK: Internal` sin `FREQUENCY`.
pub const INTERNAL_RESET_HZ: u32 = 4_000_000;

pub fn fmt_hz(hz: u32) -> String {
    if hz >= 1_000_000 && hz.is_multiple_of(100_000) {
        format!("{} MHz", hz as f64 / 1e6)
    } else if hz >= 1_000 && hz.is_multiple_of(100) {
        format!("{} kHz", hz as f64 / 1e3)
    } else {
        format!("{hz} Hz")
    }
}

/// Resuelve la fuente y la frecuencia contra el perfil del dispositivo; `mode` es `OSC_MODE` (`None` = automatico). El error dice
/// cuales valen.
pub fn resolve(profile: &OscillatorProfile, kind: ClockKind, hz: u32, mode: Option<&str>) -> Result<ClockSource, String> {
    if kind == ClockKind::Internal {
        let Some(fosc) = &profile.internal_fosc else {
            return Err("este dispositivo no tiene oscilador interno: declara 'CLOCK: Crystal' y 'FREQUENCY: ...'".into());
        };
        if let Some(m) = mode.filter(|m| *m != "Auto") {
            return Err(format!("OSC_MODE: {m} no se aplica al oscilador interno: su modo lo fija el dispositivo"));
        }
        return match profile.internal.iter().find(|(f, _)| *f == hz) {
            Some(&(_, ircf)) => Ok(ClockSource { kind, hz, system_hz: hz, mode: "INTOSC".into(), fosc: fosc.clone(), ircf: Some(ircf), pins: Vec::new(), extra_config: Vec::new() }),
            None => {
                let list: Vec<String> = profile.internal.iter().map(|(f, _)| fmt_hz(*f)).collect();
                Err(format!("el oscilador interno de este dispositivo da {} (los valores de IRCF de OSCCON), no {}; para otra frecuencia usa un cristal (CLOCK: Crystal)", list.join(", "), fmt_hz(hz)))
            }
        };
    }
    let of_kind: Vec<&OscillatorMode> = profile.modes.iter().filter(|m| m.source == kind).collect();
    if of_kind.is_empty() {
        return Err(format!("este dispositivo no admite '{}' como fuente de reloj", kind.name()));
    }
    let chosen = match mode {
        None | Some("Auto") => of_kind.iter().find(|m| (m.min_hz..=m.max_hz).contains(&hz)).copied(),
        Some(want) => match of_kind.iter().find(|m| m.mode.eq_ignore_ascii_case(want)) {
            Some(m) if (m.min_hz..=m.max_hz).contains(&hz) => Some(*m),
            Some(m) => return Err(format!("{} no cabe en el modo {}, que va de {} a {}", fmt_hz(hz), m.mode, fmt_hz(m.min_hz), fmt_hz(m.max_hz))),
            None => {
                let mut valid: Vec<&str> = of_kind.iter().map(|m| m.mode.as_str()).collect();
                valid.dedup();
                return Err(format!("OSC_MODE: {want} no existe para '{}' en este dispositivo; valen Auto, {}", kind.name(), valid.join(", ")));
            }
        },
    };
    match chosen {
        Some(m) => Ok(ClockSource { kind, hz, system_hz: hz, mode: m.mode.clone(), fosc: m.fosc.clone(), ircf: None, pins: m.pins.clone(), extra_config: Vec::new() }),
        None => {
            let ranges: Vec<String> = of_kind.iter().map(|m| format!("{} {} a {}", m.mode, fmt_hz(m.min_hz), fmt_hz(m.max_hz))).collect();
            Err(format!("{} no cabe en ningun modo de '{}' de este dispositivo: {}", fmt_hz(hz), kind.name(), ranges.join(", ")))
        }
    }
}

/// `resolve` con los ajustes complementarios: PLL, divisor, salida de reloj, monitor de fallas y cambio de fuente. Todo se valida
/// contra el perfil del dispositivo; lo que el dispositivo no tiene es un error (apagarlo con `Off` siempre es valido).
pub fn resolve_with(profile: &OscillatorProfile, kind: ClockKind, hz: u32, mode: Option<&str>, st: &ClockSettings) -> Result<ClockSource, String> {
    let mut src = resolve(profile, kind, hz, mode)?;
    let mut sys = hz as u64;
    // PLL
    match st.pll {
        None | Some(Toggle::Off) => {}
        Some(t) => {
            let Some(pll) = &profile.pll else { return Err("este dispositivo no tiene PLL: quita 'PLL:' (o usa PLL: Off)".into()) };
            if !pll.sources.contains(&kind) {
                let names: Vec<&str> = pll.sources.iter().map(|k| k.name()).collect();
                return Err(format!("la PLL de este dispositivo solo trabaja con {}, no con '{}'", names.join(", "), kind.name()));
            }
            if !(pll.min_input_hz..=pll.max_input_hz).contains(&hz) {
                return Err(format!("la PLL admite una entrada de {} a {}, no {}", fmt_hz(pll.min_input_hz), fmt_hz(pll.max_input_hz), fmt_hz(hz)));
            }
            let valid: Vec<String> = pll.multipliers.iter().map(|m| m.0.to_string()).collect();
            let entry = match t {
                Toggle::Mult(m) => pll.multipliers.iter().find(|x| x.0 == m),
                _ if pll.multipliers.len() == 1 => pll.multipliers.first(),
                _ => return Err(format!("PLL: On necesita el multiplicador: PLL: {} (valen {})", valid[0], valid.join(", "))),
            };
            let Some((mult, cfg)) = entry else { return Err(format!("la PLL de este dispositivo no tiene multiplicador x{}; valen {}", if let Toggle::Mult(m) = t { m } else { 0 }, valid.join(", "))) };
            sys *= *mult as u64;
            src.extra_config.extend(cfg.iter().cloned());
        }
    }
    // divisor
    if let Some(d) = st.divider.filter(|d| *d != 1) {
        let Some((_, cfg)) = profile.dividers.iter().find(|x| x.0 == d) else {
            let valid: Vec<String> = profile.dividers.iter().map(|x| x.0.to_string()).collect();
            return Err(if valid.is_empty() { "este dispositivo no tiene divisor de reloj: quita 'CLOCK_DIVIDER:' (o usa 1)".into() } else { format!("este dispositivo no tiene el divisor {d}; valen {}", valid.join(", ")) });
        };
        if sys % d as u64 != 0 {
            return Err(format!("{} no se divide en forma exacta entre {d}", fmt_hz(sys as u32)));
        }
        sys /= d as u64;
        src.extra_config.extend(cfg.iter().cloned());
    }
    if sys > u32::MAX as u64 {
        return Err("la frecuencia resultante no cabe en 32 bits".into());
    }
    src.system_hz = sys as u32;
    // salida de reloj
    if let Some(on) = st.output {
        match profile.clock_output.iter().find(|o| o.mode == src.mode) {
            Some(o) => {
                src.fosc = if on { o.on.clone() } else { o.off.clone() };
                if on {
                    src.pins.push(o.pin.clone());
                }
            }
            None if on => {
                let valid: Vec<&str> = profile.clock_output.iter().map(|o| o.mode.as_str()).collect();
                return Err(format!("el modo {} de este dispositivo no tiene salida de reloj (la tienen: {})", src.mode, valid.join(", ")));
            }
            None => {}
        }
    }
    // monitor de fallas y cambio de fuente
    for (what, want, prof, name) in [("FAIL_SAFE_CLOCK", st.fail_safe, &profile.fail_safe, "monitor de fallas del oscilador"), ("CLOCK_SWITCHING", st.switching, &profile.switching, "cambio de fuente de reloj")] {
        let Some(on) = want else { continue };
        match prof {
            Some(p) => {
                if on && !p.modes.iter().any(|m| *m == src.mode) {
                    return Err(format!("{what} no aplica al modo {} de este dispositivo (aplica a {})", src.mode, p.modes.join(", ")));
                }
                src.extra_config.push((p.setting.clone(), if on { "ON" } else { "OFF" }.to_string()));
            }
            None if on => return Err(format!("este dispositivo no tiene {name}: quita '{what}:' (o usa Off)")),
            None => {}
        }
    }
    Ok(src)
}

impl ClockSource {
    /// `Internal 8 MHz (FOSC = INTRC_NOCLKOUT, OSCCON = 0x70)`.
    pub fn describe(&self) -> String {
        self.describe_for(true)
    }

    /// Como `describe`, pero sin `OSCCON` en los dispositivos que no tienen ese registro (PIC16F628A).
    pub fn describe_for(&self, has_osccon: bool) -> String {
        let extra = match self.ircf.filter(|_| has_osccon) {
            Some(i) => format!(", OSCCON = {:#04x}", i << 4),
            None => String::new(),
        };
        let sys = if self.system_hz != self.hz { format!(", sistema {}", fmt_hz(self.system_hz)) } else { String::new() };
        format!("{} {}{sys} (FOSC = {}{extra})", self.kind.name(), fmt_hz(self.hz), self.fosc)
    }
}
