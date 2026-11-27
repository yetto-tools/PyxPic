//! Periodos de Timer2 mas largos que el hardware (`TMR2.every_ms(500)` a 8 MHz): el compilador usa un tick exacto de Timer2 y un contador de
//! software, de modo que BASIC no se queda corto frente a lo que se haria a mano en C o ensamblador. Se mide en el simulador cuanto dura
//! realmente cada periodo, y se comprueban el error con su causa y su solucion y los avisos.

use std::path::PathBuf;

use pyxpic_device::DevicePack;
use pyxpic_frontend::{analyze_in, Language};
use pyxpic_ir::{lower, verify};
use pyxpic_pic14::codegen::Compiled;
use pyxpic_pic14::sim::Sim;
use pyxpic_pic14::{compile, Options};

fn pack() -> DevicePack {
    DevicePack::load(&PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../devices/pic16f887")).unwrap()
}

fn analysis(lang: Language, src: &str) -> pyxpic_frontend::Analysis {
    analyze_in(lang, src, &pack().target_profile())
}

fn build(lang: Language, src: &str) -> (Compiled, u32) {
    let r = analysis(lang, src);
    let p = r.program.unwrap_or_else(|| panic!("{}\n{src}", r.diagnostics.render(src, "t")));
    let hz = p.clock_hz.unwrap();
    let m = lower(&p);
    verify(&m).unwrap();
    (compile(&m, &Options::default()).unwrap(), hz)
}

/// Milisegundos entre los cambios de RD0 (lo que dura cada periodo) en `ms` milisegundos de tiempo del PIC.
fn toggles_ms(lang: Language, src: &str, ms: u64) -> Vec<f64> {
    let (c, hz) = build(lang, src);
    let mut s = Sim::new(&c.image);
    s.fosc_hz = hz;
    let per_ms = hz as f64 / 4.0 / 1000.0; // ciclos de instruccion por milisegundo
    let end = (ms as f64 * per_ms) as u64;
    let mut last = s.pin('D', 0);
    let mut changes = Vec::new();
    while s.cycles < end {
        s.step();
        let v = s.pin('D', 0);
        if v != last {
            changes.push(s.cycles as f64 / per_ms);
            last = v;
        }
    }
    changes.windows(2).map(|w| w[1] - w[0]).collect()
}

const BASIC: &str = "DEVICE PIC16F887\nCLOCK INTERNAL 8 MHZ\n\nRD0 = 0\nTMR2.every_ms 500\n\nmain:\n  IF TMR2.expired THEN\n    TMR2.clear\n    TOGGLE RD0\n  ENDIF\n  GOTO main\n";

#[test]
fn a_500_ms_period_at_8_mhz_toggles_every_500_ms_exactly() {
    let gaps = toggles_ms(Language::Basic, BASIC, 3_300);
    assert!(gaps.len() >= 5, "{gaps:?}");
    for g in &gaps {
        assert!((g - 500.0).abs() < 0.05, "intervalo de {g} ms: {gaps:?}");
    }
}

#[test]
fn python_gets_the_same_long_period() {
    let py = "from pyxpic.device import PIC16F887\nfrom pyxpic.periph.gpio import RD0\nfrom pyxpic.periph.timer import TMR2\nCLOCK: Internal\nFREQUENCY: 8MHz\n\n\ndef main() -> None:\n    RD0.output()\n    TMR2.every_ms(500)\n    while True:\n        if TMR2.expired():\n            TMR2.clear()\n            RD0.toggle()\n";
    let gaps = toggles_ms(Language::Python, py, 3_300);
    assert!(gaps.len() >= 5 && gaps.iter().all(|g| (g - 500.0).abs() < 0.05), "{gaps:?}");
}

#[test]
fn clear_restarts_the_period_and_a_period_that_fits_the_hardware_costs_nothing_extra() {
    // un periodo que cabe en Timer2 (20 ms a 8 MHz): sin contador de software, sin aviso
    let short = BASIC.replace("every_ms 500", "every_ms 20");
    let r = analysis(Language::Basic, &short);
    assert!(r.program.is_some() && !r.diagnostics.codes().contains(&"PYP3026"), "{}", r.diagnostics.render(&short, "t"));
    let (c_short, _) = build(Language::Basic, &short);
    let (c_long, _) = build(Language::Basic, BASIC);
    assert_eq!(c_long.ram_bytes, c_short.ram_bytes + 5, "el contador (2 bytes), la bandera de software (1) y dos temporales (2)");
    let gaps = toggles_ms(Language::Basic, &short, 400);
    assert!(gaps.iter().all(|g| (g - 20.0).abs() < 0.05), "{gaps:?}");
}

#[test]
fn the_compiler_says_what_it_did_with_a_cause_and_a_way_out() {
    let r = analysis(Language::Basic, BASIC);
    let d = r.diagnostics.items.iter().find(|d| d.code == "PYP3026").expect("aviso");
    assert!(d.message.contains("contador de software de 25 vueltas"), "{}", d.message);
    let cause = d.cause.as_deref().unwrap();
    assert!(cause.contains("se desborda") && cause.contains("32.768 ms") && cause.contains("20 ms (40000 ciclos)"), "{cause}");
    assert!(d.fix.as_deref().unwrap().contains("interrupt_on"));
    let text = r.diagnostics.render(BASIC, "t.bas");
    assert!(text.contains("\n  causa: ") && text.contains("\n  solucion: "), "{text}");
}

#[test]
fn an_impossible_period_is_an_error_with_its_cause_the_nearest_exact_periods_and_a_fix() {
    // 100,003 ms a 8 MHz son 200 006 ciclos: ni un tick exacto por un contador lo forma
    let bad = BASIC.replace("every_ms 500", "every_us 100003");
    let r = analysis(Language::Basic, &bad);
    assert!(r.program.is_none());
    let d = r.diagnostics.items.iter().find(|d| d.code == "PYP2126").expect("error");
    assert!(d.message.contains("Timer2 no puede dar un periodo exacto de 100.003 ms"), "{}", d.message);
    let cause = d.cause.as_deref().unwrap();
    assert!(cause.contains("contador de 8 bits") && cause.contains("se desborda") && cause.contains("200006 ciclos"), "{cause}");
    let fix = d.fix.as_deref().unwrap();
    assert!(fix.contains("100.002 ms y 100.004 ms") && fix.contains("TMR2.start"), "{fix}");
    // y los dos periodos que propone si se pueden formar
    for ok in ["every_us 100002", "every_us 100004"] {
        let src = BASIC.replace("every_ms 500", ok);
        assert!(analysis(Language::Basic, &src).program.is_some(), "{ok}");
    }
}

#[test]
fn the_timer_interrupt_is_refused_with_a_software_period() {
    let src = format!("{BASIC}\n");
    let src = src.replace("TMR2.every_ms 500\n", "TMR2.every_ms 500\nTMR2.interrupt_on\n");
    let r = analysis(Language::Basic, &src);
    let text = r.diagnostics.render(&src, "t.bas");
    assert!(r.program.is_none() && text.contains("PYP2127") && text.contains("interrupt_on no sirve con un periodo largo"), "{text}");
}

#[test]
fn syntax_and_semantic_errors_say_which_analyzer_found_them() {
    let syntax = analysis(Language::Basic, "DEVICE PIC16F887\nmain:\n  b0 = (\n");
    assert!(syntax.diagnostics.items.iter().all(|d| d.phase == pyxpic_frontend::diag::Phase::Syntax), "{:?}", syntax.diagnostics.items);
    let semantic = analysis(Language::Basic, "DEVICE PIC16F887\nmain:\n  HIGH RE5\n  GOTO main\n");
    assert!(semantic.diagnostics.items.iter().any(|d| d.phase == pyxpic_frontend::diag::Phase::Semantic), "{:?}", semantic.diagnostics.items);
    let py_semantic = analysis(Language::Python, "def main() -> None:\n    x: uint8 = 300\n");
    assert!(py_semantic.diagnostics.items.iter().all(|d| d.phase == pyxpic_frontend::diag::Phase::Semantic));
    let py_syntax = analysis(Language::Python, "def main( -> None:\n");
    assert!(py_syntax.diagnostics.items.iter().any(|d| d.phase == pyxpic_frontend::diag::Phase::Syntax));
}

/// La salida de emergencia: el mismo periodo largo escrito a mano en ensamblador PIC-AS dentro del programa BASIC (un bloque `ASM` que se llama
/// con `GOSUB`). Lo que se haria en C o ensamblador con XC8 tambien se puede hacer sin salir de BASIC; el compilador solo ahorra escribirlo.
#[test]
fn the_same_long_period_written_by_hand_in_an_asm_block_gives_the_same_500_ms() {
    let asm = "DEVICE PIC16F887\nCLOCK INTERNAL 8 MHZ\n\nRD0 = 0\nb2 = 0\nTMR2.every_ms 20\n\nmain:\n  GOSUB tick500\n  IF b3 = 1 THEN\n    TOGGLE RD0\n  ENDIF\n  GOTO main\n\nASM tick500\n    CLRF b3\n    BTFSS PIR1,1          ; TMR2IF: el tick de 20 ms de Timer2\n    RETURN\n    BCF PIR1,1\n    INCF b2,F\n    MOVLW 25              ; 25 ticks x 20 ms = 500 ms\n    XORWF b2,W\n    BTFSS STATUS,2        ; Z = ya van 25\n    RETURN\n    CLRF b2\n    INCF b3,F\n    RETURN\nEND ASM\n";
    let gaps = toggles_ms(Language::Basic, asm, 3_300);
    assert!(gaps.len() >= 5 && gaps.iter().all(|g| (g - 500.0).abs() < 0.05), "{gaps:?}");
}
