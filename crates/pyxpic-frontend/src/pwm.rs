//! PWM de los modulos CCP (`pyxpic.periph.pwm`): tipos compartidos entre el checker, el IR y el backend.
//!
//! Fuente: DS40001291H cap. 11 (PWM Mode, p.126-129; CCP1CON y CCP2CON, p.122-123), leida de la pagina del PDF:
//!  * periodo = (PR2 + 1) x 4 x TOSC x prescaler de Timer2 (ecuacion 11-1); el postscaler **no** cuenta;
//!  * ancho de pulso = (CCPRxL:CCPxCON<5:4>) x TOSC x prescaler (ecuacion 11-2), razon = valor / (4 x (PR2 + 1)) (ecuacion 11-3);
//!  * con un valor de 0 el pin nunca se sube; con un ancho mayor que el periodo el pin no cambia (queda en alto);
//!  * CCPRxL y DCxB se pueden escribir en cualquier momento, pero el valor pasa a CCPRxH al terminar el periodo (doble bufer);
//!  * orden de arranque (11.5.7): pin como entrada, PR2, CCPxCON, ciclo de trabajo, T2CON (con TMR2IF en 0 y TMR2ON), esperar
//!    TMR2IF y solo entonces poner el pin como salida (para no sacar un primer pulso incompleto).
//!
//! Los dos modulos comparten Timer2, asi que el programa tiene **una sola** frecuencia.

/// Un modulo PWM: `PWM1` es CCP1 (pin P1A/CCP1 = RC2) y `PWM2` es CCP2 (pin CCP2 = RC1).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PwmId {
    Pwm1,
    Pwm2,
}

impl PwmId {
    pub fn name(self) -> &'static str {
        match self {
            PwmId::Pwm1 => "PWM1",
            PwmId::Pwm2 => "PWM2",
        }
    }

    /// Numero del modulo CCP.
    pub fn ccp(self) -> u8 {
        match self {
            PwmId::Pwm1 => 1,
            PwmId::Pwm2 => 2,
        }
    }

    /// El pin de salida segun `pins.json` del PIC16F887.
    pub fn pin(self) -> &'static str {
        match self {
            PwmId::Pwm1 => "RC2",
            PwmId::Pwm2 => "RC1",
        }
    }

    /// Nombre del periferico en `peripherals.json`.
    pub fn peripheral(self) -> &'static str {
        match self {
            PwmId::Pwm1 => "ECCP",
            PwmId::Pwm2 => "CCP2",
        }
    }

    /// La funcion del pin (`pins.json`).
    pub fn function(self) -> &'static str {
        match self {
            PwmId::Pwm1 => "CCP1",
            PwmId::Pwm2 => "CCP2",
        }
    }

    pub fn from_name(n: &str) -> Option<PwmId> {
        match n {
            "PWM1" => Some(PwmId::Pwm1),
            "PWM2" => Some(PwmId::Pwm2),
            _ => None,
        }
    }
}

/// Temporizacion de Timer2 para el PWM: prescaler (1, 4 o 16) y `PR2`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PwmTiming {
    pub prescaler: u8,
    pub pr2: u8,
}

impl PwmTiming {
    /// El periodo en ciclos de instruccion: `(PR2 + 1) x prescaler` (4 TOSC por ciclo).
    pub fn period_cycles(&self) -> u32 {
        (self.pr2 as u32 + 1) * self.prescaler as u32
    }

    /// Cuentas de ciclo de trabajo en un periodo: `4 x (PR2 + 1)`. Un valor igual o mayor es 100 %.
    pub fn full_scale(&self) -> u32 {
        4 * (self.pr2 as u32 + 1)
    }

    /// Valor de `T2CKPS<1:0>`.
    pub fn t2ckps(&self) -> u8 {
        match self.prescaler {
            1 => 0,
            4 => 1,
            _ => 2,
        }
    }

    /// Resolucion maxima en bits (ecuacion 11-4): `log2(4 x (PR2 + 1))`.
    pub fn resolution_bits(&self) -> f64 {
        (self.full_scale() as f64).log2()
    }
}

/// Prescalers de Timer2.
pub const PRESCALERS: [u8; 3] = [1, 4, 16];

/// Todos los periodos (en ciclos) que Timer2 puede dar con alguno de sus prescalers.
fn representable(cycles: u32) -> Option<PwmTiming> {
    PRESCALERS.iter().find_map(|&p| (cycles.is_multiple_of(p as u32) && (1..=256).contains(&(cycles / p as u32))).then(|| PwmTiming { prescaler: p, pr2: (cycles / p as u32 - 1) as u8 }))
}

/// Resuelve `frequency=` a `fosc_hz`: el periodo en ciclos debe ser **exacto** y salir de `prescaler x (PR2 + 1)`. Elige el
/// prescaler **mas chico** (la mejor resolucion). Si no se puede, devuelve las dos frecuencias exactas mas cercanas.
pub fn resolve_frequency(fosc_hz: u32, clocks_per_cycle: u32, freq_hz: u32) -> Result<PwmTiming, String> {
    let denom = clocks_per_cycle as u64 * freq_hz as u64;
    if denom == 0 {
        return Err("la frecuencia debe ser mayor que cero".into());
    }
    let cycles = fosc_hz as u64 / denom;
    if (fosc_hz as u64).is_multiple_of(denom) {
        if let Some(t) = u32::try_from(cycles).ok().and_then(representable) {
            return Ok(t);
        }
    }
    // la frecuencia exacta mas cercana por debajo y por encima entre los periodos que Timer2 sabe dar
    let target = fosc_hz as f64 / (clocks_per_cycle as f64 * freq_hz as f64);
    let (mut lo, mut hi) = (None, None);
    for n in 1..=4096u32 {
        if representable(n).is_some() {
            if (n as f64) <= target {
                lo = Some(n);
            } else if hi.is_none() {
                hi = Some(n);
            }
        }
    }
    let f = |n: u32| fosc_hz as f64 / (clocks_per_cycle as f64 * n as f64);
    let fmt = |n: u32| {
        let v = f(n);
        if (v - v.round()).abs() < 1e-9 {
            format!("{} Hz", v.round() as u64)
        } else {
            format!("{v:.2} Hz")
        }
    };
    let near = match (lo, hi) {
        (Some(a), Some(b)) => format!("las frecuencias exactas mas cercanas son {} (periodo de {a} ciclos) y {} ({b} ciclos)", fmt(a), fmt(b)),
        (Some(a), None) => format!("la frecuencia exacta mas cercana es {} ({a} ciclos)", fmt(a)),
        (None, Some(b)) => format!("la frecuencia exacta mas cercana es {} ({b} ciclos)", fmt(b)),
        (None, None) => String::new(),
    };
    Err(format!(
        "{freq_hz} Hz no cae en un periodo exacto de Timer2 a {fosc_hz} Hz (el periodo es prescaler x (PR2+1) ciclos de {} ns, con prescaler 1/4/16 y PR2+1 de 1 a 256: de 1 a 4096 ciclos); {near}; o usa start(prescaler=..., period=...)",
        clocks_per_cycle as f64 * 1e9 / fosc_hz as f64
    ))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PwmOp {
    /// `PWM1.start(...)`: sigue el orden de 11.5.7. La temporizacion es `None` hasta resolver `frequency=` (necesita el reloj);
    /// el segundo campo es el ciclo de trabajo inicial (`duty=`, 0 por defecto).
    Start(Option<PwmTiming>, u16),
    /// `PWM1.duty(v)`: `CCPRxL = v >> 2` y `DCxB = v & 3`.
    Duty,
    /// `PWM1.stop()`: `CCPxCON = 0` (el pin vuelve a ser del puerto y el modulo se reinicia).
    Stop,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PwmCall {
    pub pwm: PwmId,
    pub op: PwmOp,
}
