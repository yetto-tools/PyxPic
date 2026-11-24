//! Frontend del compilador PyPIC (solo corre en el host).
//!
//! Lee fuente con sintaxis Python, valida el subconjunto estatico y tipado de
//! PyPIC y produce un `Program` tipado para el IR. No hay ni habra interprete
//! ni VM de Python en el MCU (ver docs/spec/00_PRINCIPIOS_NO_INTERPRETE.md).

pub mod ast;
pub mod check;
pub mod diag;
pub mod lexer;
pub mod parser;
pub mod target;
pub mod types;

pub use check::{time_unit, DelayUnit, PinDecl, Program};
pub use diag::{Diagnostic, Diagnostics, Severity, Span};
pub use target::{PinInfo, TargetProfile};
pub use types::Type;

pub struct Analysis {
    /// `Some` solo si no hubo errores.
    pub program: Option<Program>,
    pub diagnostics: Diagnostics,
}

/// Dispositivo declarado con `from pypic.device import PIC16F887`, sin analizar el resto del programa.
/// Sirve para elegir el Device Pack antes de la comprobacion completa.
pub fn declared_device(source: &str) -> Option<String> {
    let mut d = Diagnostics::default();
    let module = parser::parse(source, &mut d);
    module.items.iter().find_map(|it| match it {
        ast::Item::Import { module, names, .. } if module == "pypic.device" => names.first().map(|n| n.name.clone()),
        _ => None,
    })
}

pub fn analyze(source: &str, target: &TargetProfile) -> Analysis {
    let mut d = Diagnostics::default();
    let module = parser::parse(source, &mut d);
    // Si hubo errores de sintaxis/subconjunto no se continua: el AST puede estar incompleto.
    if d.has_errors() {
        return Analysis { program: None, diagnostics: d };
    }
    let c = check::check(&module, target, &mut d);
    let program = (!d.has_errors()).then_some(Program { module, info: c.info, globals: c.globals, functions: c.functions, pins: c.pins, device: c.device, clock_hz: c.clock_hz, clock_defaulted: c.clock_defaulted });
    Analysis { program, diagnostics: d }
}
