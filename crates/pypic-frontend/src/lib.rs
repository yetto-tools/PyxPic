//! Frontend del compilador PyPIC (solo corre en el host).
//!
//! Lee fuente con sintaxis Python, valida el subconjunto estatico y tipado de
//! PyPIC y produce un `Program` tipado para el IR. No hay ni habra interprete
//! ni VM de Python en el MCU (ver docs/spec/00_PRINCIPIOS_NO_INTERPRETE.md).

pub mod adc;
pub mod ast;
pub mod basic;
pub mod bits;
pub mod check;
pub mod clock;
pub mod comparator;
pub mod diag;
pub mod eeprom;
pub mod lexer;
pub mod parser;
pub mod pwm;
pub mod regs;
pub mod runtime;
pub mod target;
pub mod text;
pub mod timer;
pub mod types;
pub mod uart;

pub use adc::{AdcCall, AdcClock, AdcConfig, AdcOp};
pub use bits::BitsOp;
pub use check::{time_unit, DelayUnit, PinDecl, Program};
pub use clock::{ClockKind, ClockSource};
pub use comparator::{CmpId, CmpRef, CompCall, CompOp, CvrefOp};
pub use diag::{Diagnostic, Diagnostics, Severity, Span};
pub use eeprom::{EepromCall, EepromOp};
pub use pwm::{PwmCall, PwmId, PwmOp, PwmTiming};
pub use regs::{RegCall, RegOp};
pub use runtime::{pow2_exp, routine_for, RtKind};
pub use target::{PinClaim, PinInfo, RegInfo, TargetProfile};
pub use text::{TextCall, TextOp};
pub use timer::{TimerCall, TimerId, TimerOp, TimerStart};
pub use types::Type;
pub use uart::{format_uint, FmtPart, UartBrg, UartCall, UartOp, UartStart};

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

/// Diagnosticos del verificador que, sobre un AST incompleto, pueden ser falsos porque dependen de una declaracion que el parser descarto: nombre o
/// funcion no definidos, asignar a lo no declarado, `global` de lo no declarado y el inicio (`start`) del ADC o la UART que estaba en codigo descartado.
const DEPEND_ON_DROPPED_CODE: &[&str] = &["PYP2043", "PYP2022", "PYP2020", "PYP2023", "PYP2171", "PYP2181"];

/// El lenguaje de un programa: Python (`.py`) o PyPIC BASIC (`.bas`). Los dos producen el mismo AST y comparten todo lo demas.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Language {
    Python,
    Basic,
}

impl Language {
    /// Por la extension del archivo: `.bas` es BASIC; cualquier otra cosa, Python.
    pub fn from_path(path: &str) -> Language {
        if std::path::Path::new(path).extension().is_some_and(|e| e.eq_ignore_ascii_case("bas")) {
            Language::Basic
        } else {
            Language::Python
        }
    }
}

pub fn declared_device_in(lang: Language, source: &str) -> Option<String> {
    match lang {
        Language::Python => declared_device(source),
        Language::Basic => basic::declared_device(source),
    }
}

pub fn declares_clock_in(lang: Language, source: &str) -> bool {
    match lang {
        Language::Python => declares_clock(source),
        Language::Basic => basic::declares_clock(source),
    }
}

pub fn analyze_in(lang: Language, source: &str, target: &TargetProfile) -> Analysis {
    match lang {
        Language::Python => analyze(source, target),
        Language::Basic => {
            let mut d = Diagnostics::default();
            let module = basic::to_module(source, &mut d);
            analyze_parsed(source, module, d, target)
        }
    }
}

pub fn analyze(source: &str, target: &TargetProfile) -> Analysis {
    let mut d = Diagnostics::default();
    let module = parser::parse(source, &mut d);
    analyze_parsed(source, module, d, target)
}

/// Lo que sigue al parser, comun a los dos lenguajes: la comprobacion de tipos y la construccion del `Program`.
fn analyze_parsed(source: &str, mut module: ast::Module, mut d: Diagnostics, target: &TargetProfile) -> Analysis {
    for it in &mut module.items {
        if let ast::Item::Func(f) = it {
            if let Some(a) = &mut f.asm {
                a.first_line = 1 + source[..a.offset.min(source.len())].matches('\n').count();
            }
        }
    }
    // Con errores de sintaxis no hay programa, pero se siguen comprobando los tipos de lo que el parser si leyo (la recuperacion descarta la
    // linea rota y su bloque): asi se ven los dos tipos de error a la vez. Los diagnosticos que dependen de lo que falta (un nombre o una
    // funcion cuya definicion era la linea rota) se descartan: serian falsos.
    if d.has_errors() {
        let mut partial = Diagnostics::default();
        check::check(&module, target, &mut partial);
        d.items.extend(partial.items.into_iter().filter(|x| x.severity == Severity::Error && !DEPEND_ON_DROPPED_CODE.contains(&x.code)));
        return Analysis { program: None, diagnostics: d };
    }
    let c = check::check(&module, target, &mut d);
    let program = (!d.has_errors()).then_some(Program {
        module,
        info: c.info,
        globals: c.globals,
        functions: c.functions,
        pins: c.pins,
        device: c.device,
        clock_hz: c.clock_hz,
        clock_defaulted: c.clock_defaulted,
        clock_source: c.clock_source,
        timer_calls: c.timer_calls,
        reg_calls: c.reg_calls,
        bits_calls: c.bits_calls,
        roms: c.roms,
        rom_gets: c.rom_gets,
        arrays: c.arrays,
        array_inits: c.array_inits,
        array_gets: c.array_gets,
        adc_calls: c.adc_calls,
        adc_config: c.adc_config,
        pwm_calls: c.pwm_calls,
        eeprom_calls: c.eeprom_calls,
        comp_calls: c.comp_calls,
        uart_calls: c.uart_calls,
        uart_fmt: c.uart_fmt,
        text_calls: c.text_calls,
    });
    Analysis { program, diagnostics: d }
}
