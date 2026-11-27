//! PyxPic BASIC: el segundo frontend del compilador (estilo PICAXE, extension `.bas`). Produce el mismo AST que el parser de Python, asi
//! que el verificador, el IR y el backend son los mismos (docs/basic-picaxe.md).

mod lexer;
mod lower;
mod parse;
mod vars;

pub use vars::{variables, VarInfo};

use crate::ast::Module;
use crate::diag::Diagnostics;

/// Lee un programa BASIC y lo traduce al AST comun; los errores de sintaxis y de traduccion van en `d`.
/// `target` trae los pines del dispositivo (`pins.json`): solo existen los que el PIC tiene, con su tipo (digital, analogico, solo entrada).
pub fn to_module(source: &str, d: &mut Diagnostics, target: &crate::target::TargetProfile) -> Module {
    d.phase = crate::diag::Phase::Syntax;
    let stmts = parse::parse(source, d);
    d.phase = crate::diag::Phase::Semantic;
    lower::lower(&stmts, d, target)
}

/// El PIC que declara el programa (`DEVICE PIC16F887`), sin analizar nada mas.
pub fn declared_device(source: &str) -> Option<String> {
    let mut d = Diagnostics::default();
    parse::parse(source, &mut d).into_iter().find_map(|s| match s.kind {
        parse::BStmtKind::Device(n) => Some(n),
        _ => None,
    })
}

/// `true` si el programa declara su reloj (`CLOCK ...`).
pub fn declares_clock(source: &str) -> bool {
    let mut d = Diagnostics::default();
    parse::parse(source, &mut d).iter().any(|s| matches!(s.kind, parse::BStmtKind::Clock { .. }))
}
