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

pub use check::{PinDecl, Program};
pub use diag::{Diagnostic, Diagnostics, Severity, Span};
pub use target::{PinInfo, TargetProfile};
pub use types::Type;

pub struct Analysis {
    /// `Some` solo si no hubo errores.
    pub program: Option<Program>,
    pub diagnostics: Diagnostics,
}

pub fn analyze(source: &str, target: &TargetProfile) -> Analysis {
    let mut d = Diagnostics::default();
    let module = parser::parse(source, &mut d);
    // Si hubo errores de sintaxis/subconjunto no se continua: el AST puede estar incompleto.
    if d.has_errors() {
        return Analysis { program: None, diagnostics: d };
    }
    let c = check::check(&module, target, &mut d);
    let program = (!d.has_errors()).then_some(Program { module, info: c.info, globals: c.globals, functions: c.functions, pins: c.pins });
    Analysis { program, diagnostics: d }
}
