//! `CLOCK: Internal[4 * MHz]`, `Crystal[20 * MHz]`, `ExternalClock[20 * MHz]`: la fuente y la frecuencia del reloj en una linea, sin
//! imports. Fija CLOCK_HZ, FOSC y (oscilador interno) OSCCON, con los limites del datasheet.

use std::path::PathBuf;

use pyxpic_device::DevicePack;
use pyxpic_frontend::analyze;
use pyxpic_ir::{lower, verify, Module};
use pyxpic_pic14::codegen::Compiled;
use pyxpic_pic14::sim::Sim;
use pyxpic_pic14::{compile, Options};

fn pack() -> DevicePack {
    DevicePack::load(&PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../devices/pic16f887")).unwrap()
}

fn build(src: &str) -> (Module, Compiled) {
    let r = analyze(src, &pack().target_profile());
    let p = r.program.unwrap_or_else(|| panic!("frontend:\n{}\n{src}", r.diagnostics.render(src, "t.py")));
    let m = lower(&p);
    verify(&m).unwrap_or_else(|e| panic!("IR invalida: {e:?}"));
    let c = compile(&m, &Options::default()).unwrap_or_else(|e| panic!("codegen: {e}\n{src}"));
    (m, c)
}

fn diagnostics(src: &str) -> (bool, String) {
    let r = analyze(src, &pack().target_profile());
    (r.program.is_some(), r.diagnostics.render(src, "t.py"))
}

fn prog(clock: &str) -> String {
    format!("from pyxpic.device import PIC16F887\n{clock}\n\n\ndef main() -> None:\n    pass\n")
}

fn source(clock: &str) -> pyxpic_frontend::ClockSource {
    let (m, _) = build(&prog(clock));
    m.clock_source.expect("el programa declara el reloj")
}

// ---- una linea, sin imports -----------------------------------------------------------------------------------------------

#[test]
fn the_clock_is_one_line_without_imports() {
    let (m, _) = build(&prog("CLOCK: Internal[4 * MHz]"));
    assert_eq!(m.clock_hz, Some(4_000_000));
    assert!(!m.clock_defaulted);
    // importar los nombres sigue siendo valido (y no hace nada)
    let (m, _) = build(&prog("from pyxpic.clock import Crystal, MHz\nCLOCK: Crystal[8 * MHz]"));
    assert_eq!(m.clock_hz, Some(8_000_000));
    // otras unidades
    assert_eq!(source("CLOCK: Crystal[32768 * Hz]").fosc, "LP");
    assert_eq!(source("CLOCK: Internal[125 * kHz]").hz, 125_000);
}

#[test]
fn each_source_picks_the_oscillator_mode_from_the_datasheet_table() {
    // (declaracion, FOSC del Device Pack)
    for (decl, fosc) in [
        ("CLOCK: Internal[4 * MHz]", "INTRC_NOCLKOUT"),
        ("CLOCK: Internal[8 * MHz]", "INTRC_NOCLKOUT"),
        ("CLOCK: Crystal[32768 * Hz]", "LP"),
        ("CLOCK: Crystal[100 * kHz]", "XT"),
        ("CLOCK: Crystal[4 * MHz]", "XT"),
        ("CLOCK: Crystal[4_000_001 * Hz]", "HS"),
        ("CLOCK: Crystal[20 * MHz]", "HS"),
        ("CLOCK: ExternalClock[20 * MHz]", "EC"),
        ("CLOCK: ExternalClock[1 * MHz]", "EC"),
    ] {
        assert_eq!(source(decl).fosc, fosc, "{decl}");
    }
}

#[test]
fn the_internal_oscillator_has_exactly_the_datasheet_frequencies() {
    for (hz, ircf) in [(31_000u32, 0u8), (125_000, 1), (250_000, 2), (500_000, 3), (1_000_000, 4), (2_000_000, 5), (4_000_000, 6), (8_000_000, 7)] {
        assert_eq!(source(&format!("CLOCK: Internal[{hz} * Hz]")).ircf, Some(ircf), "{hz} Hz");
    }
    for bad in ["20 * MHz", "3 * MHz", "16 * MHz", "32 * kHz"] {
        let (ok, t) = diagnostics(&prog(&format!("CLOCK: Internal[{bad}]")));
        assert!(!ok && t.contains("PYP2194") && t.contains("oscilador interno") && t.contains("8 MHz"), "{bad}: {t}");
    }
}

#[test]
fn impossible_crystals_and_external_clocks_are_rejected_with_the_limits() {
    for (decl, frag) in [
        ("CLOCK: Crystal[50 * kHz]", "no cabe"),
        ("CLOCK: Crystal[50 * kHz + 1]", "no cabe"),
        ("CLOCK: Crystal[24 * MHz]", "no cabe"),
        ("CLOCK: Crystal[90 * kHz]", "no cabe"),
        ("CLOCK: ExternalClock[21 * MHz]", "continua a 20 MHz"),
    ] {
        let (ok, t) = diagnostics(&prog(decl));
        assert!(!ok && t.contains("PYP2194") && t.contains(frag), "{decl}: {t}");
    }
}

#[test]
fn bad_declarations_are_located_errors() {
    for (decl, code, frag) in [
        ("CLOCK_HZ: Const[uint32] = 4_000_000\nCLOCK: Internal[4 * MHz]", "PYP2192", "solo una"),
        ("CLOCK: Internal[4 * MHz]\nCLOCK_HZ: Const[uint32] = 4_000_000", "PYP2192", "ya se declaro"),
        ("CLOCK: Internal[4 * MHz] = 3", "PYP2190", "no se asigna"),
        ("RELOJ: Internal[4 * MHz]", "PYP2190", "se llama CLOCK"),
        ("n: uint32 = 3\nCLOCK: Internal[n * MHz]", "PYP2193", "constante"),
        ("CLOCK: Internal[0 * MHz]", "PYP2193", "mayor que 0"),
        ("CLOCK: Internal[]", "PYP1", ""),
    ] {
        let (ok, t) = diagnostics(&prog(decl));
        assert!(!ok && t.contains(code) && t.contains(frag), "{decl}: {t}");
    }
    let (ok, t) = diagnostics("from pyxpic.device import PIC16F887\nfrom pyxpic.clock import Nope\n\n\ndef main() -> None:\n    pass\n");
    assert!(!ok && t.contains("PYP2190") && t.contains("Nope"), "{t}");
    // una constante de frecuencia no se puede redeclarar
    let (ok, t) = diagnostics(&prog("MHz: uint8 = 3"));
    assert!(!ok && t.contains("PYP2009"), "{t}");
}

#[test]
fn a_crystal_takes_its_pins_and_an_external_clock_takes_clkin() {
    let run = |clock: &str, pin: &str| diagnostics(&format!("from pyxpic.device import PIC16F887\nfrom pyxpic.periph.gpio import {pin}\n{clock}\n\n\ndef main() -> None:\n    {pin}.output()\n"));
    let (ok, t) = run("CLOCK: Crystal[4 * MHz]", "RA6");
    assert!(!ok && t.contains("PYP2150") && t.contains("RA6") && t.contains("OSC2"), "{t}");
    let (ok, t) = run("CLOCK: Crystal[4 * MHz]", "RA7");
    assert!(!ok && t.contains("PYP2150") && t.contains("OSC1"), "{t}");
    let (ok, t) = run("CLOCK: ExternalClock[4 * MHz]", "RA7");
    assert!(!ok && t.contains("PYP2150") && t.contains("CLKIN"), "{t}");
    // EC deja RA6 libre, el oscilador interno deja libres los dos, y otro pin nunca choca
    let (ok, t) = run("CLOCK: ExternalClock[4 * MHz]", "RA6");
    assert!(ok, "{t}");
    let (ok, t) = run("CLOCK: Internal[4 * MHz]", "RA7");
    assert!(ok, "{t}");
    let (ok, t) = run("CLOCK: Crystal[4 * MHz]", "RB0");
    assert!(ok, "{t}");
}

// ---- OSCCON y el tiempo -----------------------------------------------------------------------------------------------------

fn blink_cycles(clock: &str, ms: u32) -> u64 {
    let src = format!("from pyxpic.device import PIC16F887\nfrom pyxpic.periph.gpio import RD0\nfrom pyxpic.system.time import pause\n{clock}\n\n\ndef main() -> None:\n    RD0.output()\n    RD0.on()\n    pause({ms})\n    RD0.off()\n");
    let (_, c) = build(&src);
    let mut s = Sim::new(&c.image);
    let (mut up, mut down, mut last) = (None, None, None);
    while !s.halted && s.cycles < 5_000_000 {
        s.step();
        let now = s.pin('D', 0);
        if now != last {
            match now {
                Some(true) => up = Some(s.cycles),
                Some(false) if up.is_some() => down = Some(s.cycles),
                _ => {}
            }
            last = now;
        }
    }
    down.unwrap() - up.unwrap() - 1
}

#[test]
fn the_declared_frequency_sets_the_exact_cycle_count_of_a_pause() {
    // TCY = 4 / FOSC: 10 ms son FOSC / 400 ciclos
    for (clock, cycles) in [
        ("CLOCK: Internal[4 * MHz]", 10_000u64),
        ("CLOCK: Internal[8 * MHz]", 20_000),
        ("CLOCK: Crystal[20 * MHz]", 50_000),
        ("CLOCK: Crystal[1 * MHz]", 2_500),
        ("CLOCK: ExternalClock[2 * MHz]", 5_000),
    ] {
        assert_eq!(blink_cycles(clock, 10), cycles, "{clock}");
    }
    // igual que la forma antigua
    assert_eq!(blink_cycles("CLOCK_HZ: Const[uint32] = 8_000_000", 10), 20_000);
}

#[test]
fn the_internal_oscillator_at_4_mhz_writes_nothing_and_other_speeds_write_osccon_and_wait() {
    let osccon_writes = |clock: &str| {
        let (_, c) = build(&prog(clock));
        let mut s = Sim::new(&c.image);
        s.run_until(2_000, |s| s.halted);
        assert!(s.halted, "{clock}");
        (s.read(0x8F), s.cycles)
    };
    // 4 MHz es el valor del reset: no se toca OSCCON (queda el valor de reset del simulador) y el arranque es corto
    let (v4, c4) = osccon_writes("CLOCK: Internal[4 * MHz]");
    assert_eq!(v4 & 0x70, 0x60);
    for (clock, ircf) in [("CLOCK: Internal[8 * MHz]", 7u8), ("CLOCK: Internal[1 * MHz]", 4), ("CLOCK: Internal[125 * kHz]", 1), ("CLOCK: Internal[31 * kHz]", 0)] {
        let (v, cycles) = osccon_writes(clock);
        assert_eq!(v >> 4 & 7, ircf, "{clock}: IRCF");
        assert_eq!(v & 1, 0, "{clock}: SCS = 0, manda FOSC");
        // el oscilador esta estable al terminar el arranque: HTS (bit 2) o LTS (bit 1) segun la frecuencia
        assert_ne!(v & if ircf == 0 { 0x02 } else { 0x04 }, 0, "{clock}: estable");
        assert!(cycles > c4, "{clock}: espero a la estabilidad ({cycles} frente a {c4} ciclos)");
    }
}

#[test]
fn the_simulator_follows_osccon_so_uart_and_adc_timing_use_the_real_clock() {
    let (_, c) = build("from pyxpic.device import PIC16F887\nfrom pyxpic.periph.uart import UART\nCLOCK: Internal[8 * MHz]\n\n\ndef main() -> None:\n    UART.start(baud=9600)\n    UART.write(65)\n    UART.write(66)\n    while UART.sending():\n        pass\n");
    let mut s = Sim::new(&c.image);
    s.run_until(2_000_000, |s| s.halted);
    assert_eq!(s.fosc_hz, 8_000_000, "OSCCON cambio la frecuencia");
    assert_eq!(s.uart_tx.iter().map(|t| t.0).collect::<Vec<_>>(), b"AB");
    // la trama entre los dos bytes dura lo que dicen 8 MHz a 9600 baudios (el compilador calcula SPBRG con 8 MHz)
    let rep = c.uart.unwrap();
    assert_eq!(rep.fosc_hz, 8_000_000);
    let frame = s.uart_tx[1].1 - s.uart_tx[0].1;
    let baud = 10.0 * 8_000_000.0 / (4.0 * frame as f64);
    assert!((baud - 9600.0).abs() / 9600.0 < 0.02, "{baud}");
}

#[test]
fn the_old_clock_hz_warning_is_only_for_clock_hz_because_clock_does_configure_the_oscillator() {
    let uart = "from pyxpic.system.time import pause\n\n\ndef main() -> None:\n    pause(5)\n";
    let (ok, t) = diagnostics(&format!("from pyxpic.device import PIC16F887\nCLOCK_HZ: Const[uint32] = 20_000_000\n{uart}"));
    assert!(ok && t.contains("PYP3005"), "CLOCK_HZ a secas: el compilador no configura el oscilador y avisa: {t}");
    let (ok, t) = diagnostics(&format!("from pyxpic.device import PIC16F887\nCLOCK: Crystal[20 * MHz]\n{uart}"));
    assert!(ok && !t.contains("PYP3005"), "con CLOCK el compilador configura el oscilador: {t}");
}

// ---- el aviso de los pines que el oscilador externo toma (PYP3016) ------------------------------------------------------------

#[test]
fn a_crystal_warns_that_ra6_and_ra7_stop_being_available() {
    let (ok, t) = diagnostics(&prog("CLOCK: Crystal[20 * MHz]"));
    assert!(ok, "es un aviso, no un error: {t}");
    assert!(t.contains("warning[PYP3016]") && t.contains("RA7 (OSC1/CLKIN)") && t.contains("RA6 (OSC2/CLKOUT)") && t.contains("cualquier otro uso") && t.contains("ExternalClock"), "{t}");
    assert!(t.contains("Crystal[20 MHz]"), "{t}");
}

#[test]
fn an_external_clock_warns_only_about_ra7_and_says_ra6_is_free() {
    let (ok, t) = diagnostics(&prog("CLOCK: ExternalClock[8 * MHz]"));
    assert!(ok && t.contains("warning[PYP3016]") && t.contains("RA7 (OSC1/CLKIN)") && t.contains("RA6 (OSC2/CLKOUT) queda libre"), "{t}");
    assert!(!t.contains("Crystal"), "{t}");
}

#[test]
fn the_internal_oscillator_and_the_manual_form_do_not_warn_about_pins() {
    for clock in ["CLOCK: Internal[4 * MHz]", "CLOCK: Internal[8 * MHz]"] {
        let (ok, t) = diagnostics(&prog(clock));
        assert!(ok && !t.contains("PYP3016"), "{clock}: {t}");
    }
    let (_, t) = diagnostics(&prog("CLOCK_HZ: Const[uint32] = 4_000_000"));
    assert!(!t.contains("PYP3016"), "{t}");
}

#[test]
fn the_default_clock_warning_suggests_the_new_declaration() {
    let src = "from pyxpic.device import PIC16F887\nfrom pyxpic.system.time import pause\n\n\ndef main() -> None:\n    pause(1)\n";
    let (ok, t) = diagnostics(src);
    assert!(ok && t.contains("PYP3004") && t.contains("declara 'CLOCK: Internal[4 * MHz]'") && !t.contains("Const[uint32]"), "{t}");
}
