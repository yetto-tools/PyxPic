//! Temporizadores del SDK (`pypic.periph.timer`): tipos compartidos entre el checker, el IR y el backend,
//! y el solucionador exacto de periodos de Timer2.
//!
//! Los parametros son los de los registros (DS40001291H, cap. 5-7): `start(prescaler=4, period=249)` escribe
//! `T2CON` y `PR2`; `every_ms(1)` es azucar que se resuelve aqui, al compilar, a esos mismos parametros.

/// Los tres temporizadores del PIC16F887.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum TimerId {
    T0,
    T1,
    T2,
}

impl TimerId {
    pub fn name(self) -> &'static str {
        match self {
            TimerId::T0 => "TMR0",
            TimerId::T1 => "TMR1",
            TimerId::T2 => "TMR2",
        }
    }

    pub fn from_name(n: &str) -> Option<TimerId> {
        match n {
            "TMR0" => Some(TimerId::T0),
            "TMR1" => Some(TimerId::T1),
            "TMR2" => Some(TimerId::T2),
            _ => None,
        }
    }

    /// Nombre del periferico en `peripherals.json`.
    pub fn peripheral(self) -> &'static str {
        match self {
            TimerId::T0 => "TIMER0",
            TimerId::T1 => "TIMER1",
            TimerId::T2 => "TIMER2",
        }
    }

    /// Ancho del contador: 8 bits (Timer0, Timer2) o 16 (Timer1).
    pub fn bits(self) -> u32 {
        if self == TimerId::T1 { 16 } else { 8 }
    }
}

/// Fuente de reloj de un temporizador.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TimerSource {
    /// `FOSC/4`: el ciclo de instruccion.
    Internal,
    /// Pin `T0CKI` (RA4), Timer0 como contador.
    T0cki,
    /// Pin `T1CKI` (RC0), Timer1 como contador.
    T1cki,
    /// Oscilador propio de Timer1 en `T1OSI`/`T1OSO` (RC1/RC0).
    T1osc,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GateSource {
    /// Pin `T1G` (RB5).
    T1g,
    /// Salida del comparador 2 (sincronizada o no segun `C2SYNC`).
    C2out,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Gate {
    pub source: GateSource,
    /// `true`: cuenta mientras la compuerta esta en alto (`T1GINV = 1`).
    pub active_high: bool,
}

/// Configuracion completa de `start(...)`, con los valores de los registros ya resueltos y validados.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TimerStart {
    /// Divisor del preescalador (1..=256 en Timer0, 1/2/4/8 en Timer1, 1/4/16 en Timer2).
    pub prescaler: u32,
    /// Divisor del postescalador (1..=16, solo Timer2).
    pub postscaler: u32,
    /// Valor de `PR2` (0..=255, solo Timer2).
    pub period: u32,
    pub source: TimerSource,
    /// Timer0 como contador: flanco de bajada (`T0SE = 1`).
    pub falling: bool,
    /// Timer1 con reloj externo: sincronizado con el reloj del sistema (`T1SYNC = 0`).
    pub sync: bool,
    pub gate: Option<Gate>,
}

impl TimerStart {
    pub const fn new() -> TimerStart {
        TimerStart { prescaler: 1, postscaler: 1, period: 0, source: TimerSource::Internal, falling: false, sync: true, gate: None }
    }

    /// Ciclos de instruccion entre dos banderas de Timer2: `prescaler * (PR2 + 1) * postscaler`.
    pub fn t2_cycles(&self) -> u64 {
        self.prescaler as u64 * (self.period as u64 + 1) * self.postscaler as u64
    }
}

impl Default for TimerStart {
    fn default() -> Self {
        TimerStart::new()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TimerOp {
    Start(TimerStart),
    Stop,
    Value,
    Set,
    Expired,
    Clear,
    InterruptOn,
    InterruptOff,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TimerCall {
    pub timer: TimerId,
    pub op: TimerOp,
}

/// Constantes que exporta `pypic.periph.timer` y su codigo interno.
pub const CONSTANTS: &[(&str, i128)] = &[
    ("INTERNAL", 0),
    ("T0CKI", 1),
    ("T1CKI", 2),
    ("T1OSC", 3),
    ("RISING", 4),
    ("FALLING", 5),
    ("T1G_HIGH", 6),
    ("T1G_LOW", 7),
    ("C2_HIGH", 8),
    ("C2_LOW", 9),
];

pub fn constant(name: &str) -> Option<i128> {
    CONSTANTS.iter().find(|(n, _)| *n == name).map(|(_, v)| *v)
}

pub const T2_PRESCALERS: [u32; 3] = [1, 4, 16];

/// Mayor numero de ciclos entre banderas de Timer2: 16 * 256 * 16.
pub const T2_MAX_CYCLES: u64 = 16 * 256 * 16;

/// Descompone `cycles` en `prescaler * (PR2 + 1) * postscaler`. Si hay varias formas elige la de menor
/// preescalador y, a igualdad, la de menor postescalador (la mayor resolucion del contador).
pub fn solve_t2(cycles: u64) -> Option<TimerStart> {
    for &pre in &T2_PRESCALERS {
        for post in 1..=16u32 {
            let div = pre as u64 * post as u64;
            if cycles % div != 0 {
                continue;
            }
            let per = cycles / div;
            if (1..=256).contains(&per) {
                return Some(TimerStart { prescaler: pre, postscaler: post, period: per as u32 - 1, ..TimerStart::new() });
            }
        }
    }
    None
}

/// Periodos alcanzables mas cercanos por debajo y por encima de `cycles`.
pub fn nearest_t2(cycles: u64) -> (Option<u64>, Option<u64>) {
    let (mut below, mut above): (Option<u64>, Option<u64>) = (None, None);
    for &pre in &T2_PRESCALERS {
        for post in 1..=16u64 {
            for per in 1..=256u64 {
                let c = pre as u64 * post * per;
                if c <= cycles {
                    below = Some(below.map_or(c, |b| b.max(c)));
                } else {
                    above = Some(above.map_or(c, |a| a.min(c)));
                }
            }
        }
    }
    (below, above)
}
