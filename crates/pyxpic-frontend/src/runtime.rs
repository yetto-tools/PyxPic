//! Rutinas de runtime nativas (`*`, `//`, `%`): cuando hacen falta, su nombre y cuanta pila usan.
//!
//! Lo comparten el checker (aviso `PYP3011` y profundidad de pila) y el backend (que las genera). Una multiplicacion por una
//! constante potencia de 2, y una division o modulo sin signo por una constante potencia de 2, no la necesitan: son
//! un desplazamiento o una mascara.

use crate::ast::BinOp;
use crate::types::Type;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum RtKind {
    /// Multiplicacion modular (igual con y sin signo).
    Mul,
    /// Division y modulo sin signo.
    UDiv,
    /// Division y modulo con signo (envuelve a `UDiv`).
    SDiv,
}

impl RtKind {
    pub fn name(self, bits: u32) -> String {
        match self {
            RtKind::Mul => format!("mul{bits}"),
            RtKind::UDiv => format!("divmod{bits}"),
            RtKind::SDiv => format!("sdivmod{bits}"),
        }
    }

    /// Niveles de la pila de hardware que usa una llamada (la division con signo llama a la sin signo).
    pub fn stack_levels(self) -> u32 {
        if self == RtKind::SDiv {
            2
        } else {
            1
        }
    }
}

/// `Some(k)` si `v` es `2^k`.
pub fn pow2_exp(v: i128) -> Option<u32> {
    (v > 0 && v & (v - 1) == 0).then(|| v.trailing_zeros())
}

/// Rutina que necesita `lhs op rhs` sobre `ty` (con los operandos constantes que se conozcan), o `None` si se resuelve con
/// desplazamientos, mascaras o instrucciones.
pub fn routine_for(op: BinOp, ty: Type, lhs: Option<i128>, rhs: Option<i128>) -> Option<RtKind> {
    if !ty.is_int() {
        return None;
    }
    match op {
        BinOp::Mul => {
            let trivial = |c: Option<i128>| c.is_some_and(|v| v == 0 || pow2_exp(v).is_some());
            (!trivial(lhs) && !trivial(rhs)).then_some(RtKind::Mul)
        }
        BinOp::Div | BinOp::Mod => {
            if !ty.is_signed() && rhs.is_some_and(|v| pow2_exp(v).is_some()) {
                None
            } else if ty.is_signed() {
                Some(RtKind::SDiv)
            } else {
                Some(RtKind::UDiv)
            }
        }
        _ => None,
    }
}
