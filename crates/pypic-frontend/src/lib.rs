//! Frontend del compilador PyPIC (solo corre en el host).
//!
//! Lee fuente con sintaxis Python, valida el subconjunto estatico y tipado de
//! PyPIC y produce un `Program` tipado para el IR. No hay ni habra interprete
//! ni VM de Python en el MCU (ver docs/spec/00_PRINCIPIOS_NO_INTERPRETE.md).

pub mod adc;
pub mod ast;
pub mod clock;
pub mod bits;
pub mod check;
pub mod diag;
pub mod lexer;
pub mod parser;
pub mod regs;
pub mod runtime;
pub mod target;
pub mod timer;
pub mod types;
pub mod uart;

pub use check::{time_unit, DelayUnit, PinDecl, Program};
pub use diag::{Diagnostic, Diagnostics, Severity, Span};
pub use adc::{AdcCall, AdcClock, AdcConfig, AdcOp};
pub use bits::BitsOp;
pub use clock::{ClockKind, ClockSource};
pub use uart::{format_uint, FmtPart, UartBrg, UartCall, UartOp, UartStart};
pub use regs::{RegCall, RegOp};
pub use runtime::{pow2_exp, routine_for, RtKind};
pub use target::{PinClaim, PinInfo, RegInfo, TargetProfile};
pub use timer::{TimerCall, TimerId, TimerOp, TimerStart};
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

/// `true` si el programa declara su reloj (`CLOCK: ...[...]` o `CLOCK_HZ`): en ese caso el perfil de proyecto no lo cambia.
pub fn declares_clock(source: &str) -> bool {
    let mut d = Diagnostics::default();
    let module = parser::parse(source, &mut d);
    module.items.iter().any(|it| matches!(it, ast::Item::Var(v) if v.name.name == "CLOCK" || v.name.name == "CLOCK_HZ"))
}

/// Valida la declaracion de reloj de un perfil de proyecto (`Crystal[20 * MHz]`) con las mismas reglas que el fuente. El error trae
/// los diagnosticos ya formateados.
pub fn parse_clock(text: &str, target: &TargetProfile) -> Result<ClockSource, String> {
    let line = format!("CLOCK: {text}\n");
    let mut t = target.clone();
    t.project_clock = None;
    let res = analyze(&line, &t);
    match res.program.and_then(|p| p.clock_source) {
        Some(c) => Ok(c),
        None => {
            let shown = res.diagnostics.render(&line, "pypic.toml (clock)");
            Err(if shown.trim().is_empty() { format!("'{text}' no es una declaracion de reloj: Internal[4 * MHz], Crystal[20 * MHz] o ExternalClock[20 * MHz]") } else { shown })
        }
    }
}

pub fn analyze(source: &str, target: &TargetProfile) -> Analysis {
    let mut d = Diagnostics::default();
    let mut module = parser::parse(source, &mut d);
    // Si hubo errores de sintaxis/subconjunto no se continua: el AST puede estar incompleto.
    if d.has_errors() {
        return Analysis { program: None, diagnostics: d };
    }
    for it in &mut module.items {
        if let ast::Item::Func(f) = it {
            if let Some(a) = &mut f.asm {
                a.first_line = 1 + source[..a.offset.min(source.len())].matches('\n').count();
            }
        }
    }
    let c = check::check(&module, target, &mut d);
    let program = (!d.has_errors()).then_some(Program { module, info: c.info, globals: c.globals, functions: c.functions, pins: c.pins, device: c.device, clock_hz: c.clock_hz, clock_defaulted: c.clock_defaulted, clock_source: c.clock_source, timer_calls: c.timer_calls, reg_calls: c.reg_calls, bits_calls: c.bits_calls, roms: c.roms, rom_gets: c.rom_gets, arrays: c.arrays, array_gets: c.array_gets, adc_calls: c.adc_calls, adc_config: c.adc_config, uart_calls: c.uart_calls, uart_fmt: c.uart_fmt });
    Analysis { program, diagnostics: d }
}
