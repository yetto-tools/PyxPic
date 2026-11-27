//! Comparadores analogicos y referencia de tension CVREF (`pyxpic.periph.comparator` y `pyxpic.periph.vref`): tipos compartidos entre el
//! checker, el IR, el backend y los simuladores.
//!
//! Fuente: DS40001291H cap. 8 (pp. 85-98) y las tablas 17-7 y 17-8 (p.256), leidas de la pagina del PDF:
//!  * `CMxCON0` (registros 8-1 y 8-2): `CxON` (bit 7), `CxOUT` (6), `CxOE` (5), `CxPOL` (4), `CxR` (2: 1 = la entrada + es la referencia
//!    CxVREF, 0 = el pin `CxIN+`) y `CxCH<1:0>` (entrada - : `C12IN0-` a `C12IN3-`);
//!  * `CxOUT` = 1 cuando V(+) > V(-) con `CxPOL` = 0 (tabla 8-1); con `CxON` = 0 la salida que llega al XOR es 0, asi que `CxOUT` = `CxPOL`;
//!  * `CM2CON1` (registro 8-3): `C1RSEL`/`C2RSEL` (bits 5 y 4) eligen CxVREF: 1 = CVREF, 0 = la referencia fija de 0,6 V (que hay que
//!    encender con `FVREN`, `SRCON<0>`);
//!  * `VRCON` (registro 8-5): `VREN` (7), `VROE` (6), `VRR` (5: 1 = rango bajo), `VRSS` (4) y `VR<3:0>`; con `VRSS` = 0 la escalera va de VSS a
//!    VDD y CVREF = (VR/24) x VDD en el rango bajo (`VRR` = 1) y VDD/4 + (VR/32) x VDD en el alto (`VRR` = 0) (ecuacion 8-1);
//!  * tiempos: CM05 (cambio de modo del comparador hasta salida valida) 10 us maximo, CV04 (asentamiento de CVREF) 10 us maximo y VR02
//!    (asentamiento de la referencia fija) 100 us maximo (solo caracterizado, no probado); CM01 (offset de entrada) +-10 mV maximo,
//!    CM02 (modo comun) de 0 a VDD - 1,5 V.

/// Un comparador: `CMP1` es C1 (`CM1CON0`) y `CMP2` es C2 (`CM2CON0`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CmpId {
    Cmp1,
    Cmp2,
}

impl CmpId {
    pub fn name(self) -> &'static str {
        match self {
            CmpId::Cmp1 => "CMP1",
            CmpId::Cmp2 => "CMP2",
        }
    }

    /// Numero del comparador (sufijo de `CxON`, `CMxCON0`...).
    pub fn n(self) -> u8 {
        match self {
            CmpId::Cmp1 => 1,
            CmpId::Cmp2 => 2,
        }
    }

    /// Canal analogico del pin `CxIN+` (`C1IN+` = RA3/AN3, `C2IN+` = RA2/AN2).
    pub fn pos_pin_an(self) -> u8 {
        match self {
            CmpId::Cmp1 => 3,
            CmpId::Cmp2 => 2,
        }
    }

    pub fn from_name(n: &str) -> Option<CmpId> {
        match n {
            "CMP1" => Some(CmpId::Cmp1),
            "CMP2" => Some(CmpId::Cmp2),
            _ => None,
        }
    }
}

/// Las cuatro entradas inversoras: `C12IN0-` a `C12IN3-` son `CxCH<1:0>` = 0 a 3; el canal analogico (`ANSn`) de cada pin es
/// AN0 (RA0), AN1 (RA1), AN9 (RB3) y AN10 (RB1).
pub const NEG_INPUTS: [(&str, u8); 4] = [("C12IN0", 0), ("C12IN1", 1), ("C12IN2", 9), ("C12IN3", 10)];

/// La entrada no inversora.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CmpRef {
    /// El pin `CxIN+` (`CxR` = 0).
    Pin,
    /// CVREF (`CxR` = 1 y `CxRSEL` = 1): hay que iniciarlo con `CVREF.start(...)`.
    Cvref,
    /// La referencia fija de 0,6 V (`CxR` = 1 y `CxRSEL` = 0, con `FVREN` = 1).
    Fixed,
}

/// Constantes de referencia que exporta `pyxpic.periph.comparator`.
pub const REFS: &[(&str, CmpRef)] = &[("REF_PIN", CmpRef::Pin), ("REF_CVREF", CmpRef::Cvref), ("REF_FIXED", CmpRef::Fixed)];

/// Pasos de `VR<3:0>`: 0 a 15.
pub const CVREF_STEPS: u8 = 16;

/// TCM2COV (CM05), TCV04 y TSTABLE (VR02) en nanosegundos: lo que el programa espera tras encender cada modulo.
pub const COMPARATOR_SETTLE_NS: u32 = 10_000;
pub const CVREF_SETTLE_NS: u32 = 10_000;
pub const FIXED_REF_SETTLE_NS: u32 = 100_000;

/// Tension nominal de la referencia fija (VR01: 0,5 a 0,7 V, 0,6 V tipico).
pub const FIXED_REF_VOLTS: f64 = 0.6;
/// La alimentacion con la que los simuladores convierten la referencia fija a cuentas de 10 bits (las entradas se dan en cuentas de
/// 0 a 1023 = VSS a VDD, como las del ADC).
pub const SIM_VDD_VOLTS: f64 = 5.0;

/// CVREF en cuentas de 10 bits (1023 = VDD), con `VRSS` = 0: ecuacion 8-1.
pub fn cvref_counts(step: u8, low_range: bool) -> f64 {
    let s = (step & 0xF) as f64;
    let fraction = if low_range { s / 24.0 } else { 0.25 + s / 32.0 };
    fraction * 1023.0
}

/// Fraccion de VDD que da CVREF (para la documentacion y los avisos).
pub fn cvref_fraction(step: u8, low_range: bool) -> f64 {
    cvref_counts(step, low_range) / 1023.0
}

/// El nivel del comparador a partir de sus entradas en cuentas de 10 bits: `C1OUT` con `C1ON`, `C1POL` y los niveles de + y -.
pub fn output(on: bool, invert: bool, pos: f64, neg: f64) -> bool {
    (on && pos > neg) ^ invert
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CompOp {
    /// `CMP1.start(negative=C12IN0, positive=REF_PIN, invert=False)`: pines como entradas analogicas, la referencia, `CMxCON0` con `CxON`,
    /// la espera del cambio de modo (CM05) y la bandera `CxIF` en 0.
    Start { neg: u8, pos: CmpRef, invert: bool },
    /// `CMP1.stop()`: `CxON` = 0 (el resto de la configuracion queda).
    Stop,
    /// `CMP1.out()`: el nivel de `CxOUT`.
    Out,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CvrefOp {
    /// `CVREF.start(step=8, low_range=False)`: `VRCON` = `VREN` + rango + `VR<3:0>` y la espera de asentamiento (CV04).
    Start { step: u8, low_range: bool },
    /// `CVREF.set(step)`: cambia `VR<3:0>` sin tocar el rango y espera el asentamiento.
    Set(u8),
    /// `CVREF.stop()`: `VRCON` = 0.
    Stop,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CompCall {
    Cmp(CmpId, CompOp),
    Cvref(CvrefOp),
}

impl CompCall {
    /// Devuelve un valor (`CMPx.out()`).
    pub fn returns_value(&self) -> bool {
        matches!(self, CompCall::Cmp(_, CompOp::Out))
    }

    /// Espera tras escribir los registros (asentamiento de CVREF, de la referencia fija o cambio de modo del comparador), en ns.
    pub fn settle_ns(&self) -> u32 {
        match self {
            CompCall::Cmp(_, CompOp::Start { pos: CmpRef::Fixed, .. }) => FIXED_REF_SETTLE_NS,
            CompCall::Cmp(_, CompOp::Start { .. }) => COMPARATOR_SETTLE_NS,
            CompCall::Cvref(CvrefOp::Start { .. } | CvrefOp::Set(_)) => CVREF_SETTLE_NS,
            _ => 0,
        }
    }
}
