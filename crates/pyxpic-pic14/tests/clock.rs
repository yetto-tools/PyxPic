//! `CLOCK: Internal` + `FREQUENCY: 8MHz`: la fuente y la frecuencia del reloj en dos lineas, sin
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

fn diagnostics_for(src: &str, t: &pyxpic_frontend::TargetProfile) -> (bool, String) {
    let r = analyze(src, t);
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
    let (m, _) = build(&prog("CLOCK: Internal\nFREQUENCY: 4MHz"));
    assert_eq!(m.clock_hz, Some(4_000_000));
    assert!(!m.clock_defaulted);
    // importar los nombres sigue siendo valido (y no hace nada)
    let (m, _) = build(&prog("from pyxpic.clock import Crystal, MHz\nCLOCK: Crystal\nFREQUENCY: 8MHz"));
    assert_eq!(m.clock_hz, Some(8_000_000));
    // otras unidades
    assert_eq!(source("CLOCK: Crystal\nFREQUENCY: 32768Hz").fosc, "LP");
    assert_eq!(source("CLOCK: Internal\nFREQUENCY: 125kHz").hz, 125_000);
}

#[test]
fn each_source_picks_the_oscillator_mode_from_the_datasheet_table() {
    // (declaracion, FOSC del Device Pack)
    for (decl, fosc) in [
        ("CLOCK: Internal\nFREQUENCY: 4MHz", "INTRC_NOCLKOUT"),
        ("CLOCK: Internal\nFREQUENCY: 8MHz", "INTRC_NOCLKOUT"),
        ("CLOCK: Crystal\nFREQUENCY: 32768Hz", "LP"),
        ("CLOCK: Crystal\nFREQUENCY: 100kHz", "XT"),
        ("CLOCK: Crystal\nFREQUENCY: 4MHz", "XT"),
        ("CLOCK: Crystal\nFREQUENCY: 4_000_001Hz", "HS"),
        ("CLOCK: Crystal\nFREQUENCY: 20MHz", "HS"),
        ("CLOCK: Internal", "INTRC_NOCLKOUT"),
        ("CLOCK: External\nFREQUENCY: 20MHz", "EC"),
        ("CLOCK: Resonator\nFREQUENCY: 8MHz", "HS"),
        ("CLOCK: RC\nFREQUENCY: 2MHz", "EXTRC_NOCLKOUT"),
        ("CLOCK: Crystal\nFREQUENCY: 11.0592MHz", "HS"),
    ] {
        assert_eq!(source(decl).fosc, fosc, "{decl}");
    }
}

#[test]
fn the_internal_oscillator_has_exactly_the_datasheet_frequencies() {
    for (hz, ircf) in [(31_000u32, 0u8), (125_000, 1), (250_000, 2), (500_000, 3), (1_000_000, 4), (2_000_000, 5), (4_000_000, 6), (8_000_000, 7)] {
        assert_eq!(source(&format!("CLOCK: Internal\nFREQUENCY: {hz}Hz")).ircf, Some(ircf), "{hz} Hz");
    }
    for bad in ["20MHz", "3MHz", "16MHz", "32kHz"] {
        let (ok, t) = diagnostics(&prog(&format!("CLOCK: Internal\nFREQUENCY: {bad}")));
        assert!(!ok && t.contains("PYP2194") && t.contains("oscilador interno") && t.contains("8 MHz"), "{bad}: {t}");
    }
}

#[test]
fn impossible_crystals_are_rejected_with_the_limits() {
    for (decl, frag) in [
        ("CLOCK: Crystal\nFREQUENCY: 50kHz", "no cabe"),
        ("CLOCK: Crystal\nFREQUENCY: 50001Hz", "no cabe"),
        ("CLOCK: Crystal\nFREQUENCY: 24MHz", "no cabe"),
        ("CLOCK: Crystal\nFREQUENCY: 90kHz", "no cabe"),
    ] {
        let (ok, t) = diagnostics(&prog(decl));
        assert!(!ok && t.contains("PYP2194") && t.contains(frag), "{decl}: {t}");
    }
}

#[test]
fn bad_declarations_are_located_errors() {
    for (decl, code, frag) in [
        ("CLOCK_HZ: Const[uint32] = 4_000_000\nCLOCK: Internal\nFREQUENCY: 4MHz", "PYP2192", "solo una"),
        ("CLOCK: Internal\nFREQUENCY: 4MHz\nCLOCK_HZ: Const[uint32] = 4_000_000", "PYP2192", "ya se declaro"),
        ("CLOCK: Internal = 3", "PYP2190", "no se asigna"),
        ("RELOJ: Internal", "PYP2190", "se llama CLOCK"),
        ("Clock: Internal", "PYP2190", "se llama CLOCK"),
        ("CLOCK: Internal(4MHz)", "PYP2190", "se declara aparte"),
        ("CLOCK: Internal[4 * MHz]", "PYP2190", "se declara aparte"),
        ("CLOCK: ExternalClock", "PYP2190", "ya no existe"),
        ("FREQUENCY: 4MHz", "PYP2190", "acompanan a CLOCK"),
        ("CLOCK: Crystal\nFREQUENCY: 4", "PYP2232", "lleva la unidad"),
        ("CLOCK: Crystal\nFREQUENCY: 3ms", "PYP2232", "tiempo"),
        ("CLOCK: Crystal\nFREQUENCY: 4MHz + 1ms", "PYP2231", "magnitudes distintas"),
        ("CLOCK: Crystal\nFREQUENCY: 4MHz * 4MHz", "PYP2231", "no tiene una unidad"),
        ("CLOCK: Crystal\nFREQUENCY: 4MHz\nFREQUENCY: 8MHz", "PYP2192", "una sola vez"),
        ("CLOCK: Secondary\nFREQUENCY: 32768Hz", "PYP2194", "no admite"),
        ("CLOCK: Crystal\nFREQUENCY: 8MHz\nOSC_MODE: XT", "PYP2194", "no cabe en el modo XT"),
        ("CLOCK: Crystal\nFREQUENCY: 8MHz\nOSC_MODE: ZZ", "PYP2194", "no existe"),
        ("CLOCK: Internal\nFREQUENCY: 8MHz\nOSC_MODE: HS", "PYP2194", "no se aplica"),
        ("CLOCK: External\nFREQUENCY: 21MHz", "PYP2194", "no cabe"),
        ("CLOCK: Crystal", "PYP2193", "necesita su frecuencia"),
        ("n: uint32 = 3\nCLOCK: Internal\nFREQUENCY: n * MHz", "PYP2193", "constante"),
        ("CLOCK: Internal\nFREQUENCY: 0MHz", "PYP2193", "mayor que 0"),
        ("CLOCK: Internal\nFREQUENCY: ", "PYP1", ""),
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
fn a_crystal_takes_its_pins() {
    let run = |clock: &str, pin: &str| diagnostics(&format!("from pyxpic.device import PIC16F887\nfrom pyxpic.periph.gpio import {pin}\n{clock}\n\n\ndef main() -> None:\n    {pin}.output()\n"));
    let (ok, t) = run("CLOCK: Crystal\nFREQUENCY: 4MHz", "RA6");
    assert!(!ok && t.contains("PYP2150") && t.contains("RA6") && t.contains("OSC2"), "{t}");
    let (ok, t) = run("CLOCK: Crystal\nFREQUENCY: 4MHz", "RA7");
    assert!(!ok && t.contains("PYP2150") && t.contains("OSC1"), "{t}");
    // el oscilador interno deja libres los dos, y otro pin nunca choca
    let (ok, t) = run("CLOCK: Internal\nFREQUENCY: 4MHz", "RA7");
    assert!(ok, "{t}");
    let (ok, t) = run("CLOCK: Crystal\nFREQUENCY: 4MHz", "RB0");
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
        ("CLOCK: Internal\nFREQUENCY: 4MHz", 10_000u64),
        ("CLOCK: Internal\nFREQUENCY: 8MHz", 20_000),
        ("CLOCK: Crystal\nFREQUENCY: 20MHz", 50_000),
        ("CLOCK: Crystal\nFREQUENCY: 1MHz", 2_500),
        ("CLOCK: Crystal\nFREQUENCY: 11.0592MHz", 27_648),
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
    let (v4, c4) = osccon_writes("CLOCK: Internal\nFREQUENCY: 4MHz");
    assert_eq!(v4 & 0x70, 0x60);
    for (clock, ircf) in [("CLOCK: Internal\nFREQUENCY: 8MHz", 7u8), ("CLOCK: Internal\nFREQUENCY: 1MHz", 4), ("CLOCK: Internal\nFREQUENCY: 125kHz", 1), ("CLOCK: Internal\nFREQUENCY: 31kHz", 0)] {
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
    let (_, c) = build("from pyxpic.device import PIC16F887\nfrom pyxpic.periph.uart import UART\nCLOCK: Internal\nFREQUENCY: 8MHz\n\n\ndef main() -> None:\n    UART.start(baud=9600)\n    UART.write(65)\n    UART.write(66)\n    while UART.sending():\n        pass\n");
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
    let (ok, t) = diagnostics(&format!("from pyxpic.device import PIC16F887\nCLOCK: Crystal\nFREQUENCY: 20MHz\n{uart}"));
    assert!(ok && !t.contains("PYP3005"), "con CLOCK el compilador configura el oscilador: {t}");
}

// ---- el aviso de los pines que el oscilador externo toma (PYP3016) ------------------------------------------------------------

#[test]
fn a_crystal_warns_that_ra6_and_ra7_stop_being_available() {
    let (ok, t) = diagnostics(&prog("CLOCK: Crystal\nFREQUENCY: 20MHz"));
    assert!(ok, "es un aviso, no un error: {t}");
    assert!(t.contains("warning[PYP3016]") && t.contains("RA7 (OSC1)") && t.contains("RA6 (OSC2)") && t.contains("cualquier otro uso"), "{t}");
    assert!(t.contains("CLOCK: Crystal (20 MHz)"), "{t}");
}

#[test]
fn the_internal_oscillator_and_the_manual_form_do_not_warn_about_pins() {
    for clock in ["CLOCK: Internal\nFREQUENCY: 4MHz", "CLOCK: Internal\nFREQUENCY: 8MHz"] {
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
    assert!(ok && t.contains("PYP3004") && t.contains("declara 'CLOCK: Internal' y 'FREQUENCY: 4MHz'") && !t.contains("Const[uint32]"), "{t}");
}

// ---- OSC_MODE, FOSC/FCY y el perfil del dispositivo ------------------------------------------------------------------------

#[test]
fn osc_mode_forces_a_mode_inside_its_range_and_auto_picks_the_datasheet_one() {
    assert_eq!(source("CLOCK: Crystal\nFREQUENCY: 2MHz").fosc, "XT");
    assert_eq!(source("CLOCK: Crystal\nFREQUENCY: 2MHz\nOSC_MODE: HS").fosc, "HS");
    assert_eq!(source("CLOCK: Crystal\nFREQUENCY: 2MHz\nOSC_MODE: Auto").fosc, "XT");
    assert_eq!(source("CLOCK: Resonator\nFREQUENCY: 455kHz").fosc, "XT");
}

#[test]
fn the_clock_constants_are_available_and_exact() {
    for (clock, hz) in [("CLOCK: Internal", 4_000_000u32), ("CLOCK: Internal\nFREQUENCY: 8MHz", 8_000_000), ("CLOCK: Crystal\nFREQUENCY: 11.0592MHz", 11_059_200), ("CLOCK_HZ: Const[uint32] = 2_000_000", 2_000_000)] {
        let src = format!("from pyxpic.device import PIC16F887\n{clock}\nA: Const[uint32] = SOURCE_FREQ\nB: Const[uint32] = FOSC\nC: Const[uint32] = FCY\n\n\ndef main() -> None:\n    pass\n");
        let r = analyze(&src, &pack().target_profile());
        let p = r.program.unwrap_or_else(|| panic!("{clock}: {}", r.diagnostics.render(&src, "t.py")));
        let k = |n: &str| p.globals.get(n).and_then(|g| g.konst);
        assert_eq!((k("A"), k("B"), k("C")), (Some(hz as i128), Some(hz as i128), Some(hz as i128 / 4)), "{clock}");
    }
}

#[test]
fn the_limits_come_from_the_device_pack_and_not_from_a_global_table() {
    // un perfil de otro dispositivo (interno hasta 16 MHz, HS hasta 40 MHz): lo que vale en uno no vale en el otro
    let mut profile = pyxpic_frontend::OscillatorProfile::pic16f887();
    profile.internal = vec![(1_000_000, 0), (16_000_000, 7)];
    for m in profile.modes.iter_mut().filter(|m| m.mode == "HS") {
        m.max_hz = 40_000_000;
    }
    let mut t = pack().target_profile();
    t.oscillator = Some(profile);
    t.max_clock_hz = Some(64_000_000);
    let ok = |decl: &str, t: &pyxpic_frontend::TargetProfile| analyze(&prog(decl), t).program.is_some();
    assert!(ok("CLOCK: Internal\nFREQUENCY: 16MHz", &t));
    assert!(ok("CLOCK: Crystal\nFREQUENCY: 32MHz", &t));
    let f887 = pack().target_profile();
    assert!(!ok("CLOCK: Internal\nFREQUENCY: 16MHz", &f887));
    assert!(!ok("CLOCK: Crystal\nFREQUENCY: 32MHz", &f887));
    // sin oscilador interno: 'CLOCK: Internal' es un error con el remedio
    let mut none = pack().target_profile();
    let mut p = pyxpic_frontend::OscillatorProfile::pic16f887();
    p.internal_fosc = None;
    none.oscillator = Some(p);
    let (ok, t) = diagnostics_for(&prog("CLOCK: Internal"), &none);
    assert!(!ok && t.contains("no tiene oscilador interno"), "{t}");
}

#[test]
fn units_must_have_the_right_magnitude_and_unused_ones_are_errors() {
    // una magnitud sin consumidor (500ms donde se espera un numero) no se interpreta en silencio
    let (ok, t) = diagnostics(&prog("X: uint32 = 500ms"));
    assert!(!ok && t.contains("PYP2230") && t.contains("tiempo"), "{t}");
    let (ok, t) = diagnostics(&prog("X: uint32 = 3.3V"));
    assert!(!ok && t.contains("PYP2230") && t.contains("voltaje"), "{t}");
    // los literales con unidad exacta
    let (ok, t) = diagnostics(&prog("CLOCK: Crystal\nFREQUENCY: 0.5Hz"));
    assert!(!ok && t.contains("PYP0009") && t.contains("entero de Hz"), "{t}");
    for f in ["8e6Hz", "11_059_200Hz", "32.768kHz", "4MHz * 2", "16MHz / 2", "8 * MHz", "2MHz + 2MHz"] {
        let (ok, t) = diagnostics(&prog(&format!("CLOCK: Crystal\nFREQUENCY: {f}")));
        assert!(ok || !t.contains("PYP0") && !t.contains("PYP2231") && !t.contains("PYP2232"), "{f}: {t}");
    }
}

#[test]
fn basic_and_python_declare_the_same_clock() {
    let py = "from pyxpic.device import PIC16F887\nCLOCK: Crystal\nFREQUENCY: 16MHz\n\n\ndef main() -> None:\n    pass\n";
    let bas = "DEVICE PIC16F887
CLOCK CRYSTAL 16 MHZ

main:
  GOTO main
";
    let t = pack().target_profile();
    let a = analyze(py, &t).program.expect("python").clock_source;
    let b = pyxpic_frontend::analyze_in(pyxpic_frontend::Language::Basic, bas, &t).program.expect("basic").clock_source;
    assert_eq!(a, b);
    assert_eq!(a.map(|c| (c.hz, c.fosc)), Some((16_000_000, "HS".to_string())));
}

// ---- fase 3: PLL, divisor, salida de reloj, monitor de fallas y cambio de fuente ----------------------------------------

fn with_pll_profile() -> pyxpic_frontend::TargetProfile {
    // un dispositivo imaginario con PLL x4 / x8 (entrada de 4 a 16 MHz, solo cristal) y divisores 2 y 4; maximo 64 MHz
    let mut p = pyxpic_frontend::OscillatorProfile::pic16f887();
    p.pll = Some(pyxpic_frontend::PllProfile { multipliers: vec![(4, vec![("PLLEN".into(), "ON".into())]), (8, vec![])], sources: vec![pyxpic_frontend::ClockKind::Crystal], min_input_hz: 4_000_000, max_input_hz: 16_000_000 });
    p.dividers = vec![(2, vec![]), (4, vec![])];
    let mut t = pack().target_profile();
    t.oscillator = Some(p);
    t.max_clock_hz = Some(64_000_000);
    t
}

#[test]
fn the_pll_and_the_divider_multiply_and_divide_the_source_exactly() {
    let t = with_pll_profile();
    let src = |decl: &str| {
        let s = prog(decl);
        let r = analyze(&s, &t);
        r.program.unwrap_or_else(|| panic!("{decl}: {}", r.diagnostics.render(&s, "t.py")))
    };
    let p = src("CLOCK: Crystal\nFREQUENCY: 8MHz\nPLL: 4");
    let c = p.clock_source.unwrap();
    assert_eq!((c.hz, c.system_hz), (8_000_000, 32_000_000));
    assert_eq!(c.extra_config, vec![("PLLEN".to_string(), "ON".to_string())]);
    assert_eq!(p.clock_hz, Some(32_000_000));
    let c = src("CLOCK: Crystal\nFREQUENCY: 8MHz\nPLL: 8\nCLOCK_DIVIDER: 2").clock_source.unwrap();
    assert_eq!(c.system_hz, 32_000_000);
    let c = src("CLOCK: Crystal\nFREQUENCY: 8MHz\nCLOCK_DIVIDER: 4").clock_source.unwrap();
    assert_eq!(c.system_hz, 2_000_000);
    // PLL: Off y CLOCK_DIVIDER: 1 son validos y no hacen nada
    assert_eq!(src("CLOCK: Crystal\nFREQUENCY: 8MHz\nPLL: Off\nCLOCK_DIVIDER: 1").clock_source.unwrap().system_hz, 8_000_000);
    // FOSC y FCY siguen a la frecuencia resultante; SOURCE_FREQ es la de la fuente
    let s = "from pyxpic.device import PIC16F887\nCLOCK: Crystal\nFREQUENCY: 8MHz\nPLL: 4\nS: Const[uint32] = SOURCE_FREQ\nF: Const[uint32] = FOSC\nY: Const[uint32] = FCY\n\n\ndef main() -> None:\n    pass\n";
    let p = analyze(s, &t).program.unwrap();
    let k = |n: &str| p.globals.get(n).and_then(|g| g.konst);
    assert_eq!((k("S"), k("F"), k("Y")), (Some(8_000_000), Some(32_000_000), Some(8_000_000)));
}

#[test]
fn pll_and_divider_errors_name_what_the_device_has() {
    let t = with_pll_profile();
    for (decl, frag) in [
        ("CLOCK: Crystal\nFREQUENCY: 8MHz\nPLL: 3", "no tiene multiplicador x3; valen 4, 8"),
        ("CLOCK: Crystal\nFREQUENCY: 8MHz\nPLL: On", "necesita el multiplicador"),
        ("CLOCK: Crystal\nFREQUENCY: 2MHz\nPLL: 4", "admite una entrada de 4 MHz a 16 MHz"),
        ("CLOCK: Internal\nFREQUENCY: 8MHz\nPLL: 4", "solo trabaja con Crystal"),
        ("CLOCK: Crystal\nFREQUENCY: 8MHz\nCLOCK_DIVIDER: 3", "no tiene el divisor 3; valen 2, 4"),
        ("CLOCK: Crystal\nFREQUENCY: 16MHz\nPLL: 8", "excede el reloj maximo"),
        ("CLOCK: Crystal\nFREQUENCY: 9000001Hz\nCLOCK_DIVIDER: 4", "no se divide en forma exacta"),
    ] {
        let s = prog(decl);
        let r = analyze(&s, &t);
        let msg = r.diagnostics.render(&s, "t.py");
        assert!(r.program.is_none() && msg.contains(frag), "{decl}: {msg}");
    }
    // el PIC16F887 no tiene ni PLL ni divisores
    for (decl, frag) in [("PLL: 4", "no tiene PLL"), ("PLL: On", "no tiene PLL"), ("CLOCK_DIVIDER: 2", "no tiene divisor de reloj")] {
        let (ok, t) = diagnostics(&prog(&format!("CLOCK: Crystal\nFREQUENCY: 8MHz\n{decl}")));
        assert!(!ok && t.contains("PYP2194") && t.contains(frag), "{decl}: {t}");
    }
    let (ok, t) = diagnostics(&prog("CLOCK: Crystal\nFREQUENCY: 8MHz\nPLL: Off\nCLOCK_DIVIDER: 1"));
    assert!(ok, "apagar lo que el dispositivo no tiene es valido: {t}");
}

#[test]
fn the_clock_output_switches_the_fosc_variant_and_takes_ra6() {
    assert_eq!(source("CLOCK: Internal\nFREQUENCY: 4MHz").fosc, "INTRC_NOCLKOUT");
    let c = source("CLOCK: Internal\nFREQUENCY: 4MHz\nCLOCK_OUTPUT: On");
    assert_eq!(c.fosc, "INTRC_CLKOUT");
    assert_eq!(c.pins, vec![("RA6".to_string(), "CLKOUT".to_string())]);
    assert_eq!(source("CLOCK: Internal\nCLOCK_OUTPUT: Off").fosc, "INTRC_NOCLKOUT");
    assert_eq!(source("CLOCK: RC\nFREQUENCY: 2MHz\nCLOCK_OUTPUT: On").fosc, "EXTRC_CLKOUT");
    // RA6 deja de ser E/S
    let run = |decl: &str| diagnostics(&format!("from pyxpic.device import PIC16F887\nfrom pyxpic.periph.gpio import RA6\n{decl}\n\n\ndef main() -> None:\n    RA6.output()\n"));
    let (ok, t) = run("CLOCK: Internal\nCLOCK_OUTPUT: On");
    assert!(!ok && t.contains("PYP2150") && t.contains("CLKOUT"), "{t}");
    let (ok, t) = run("CLOCK: Internal\nCLOCK_OUTPUT: Off");
    assert!(ok, "{t}");
    // los modos de cristal no tienen CLOCK_OUTPUT: OSC2 es del cristal
    let (ok, t) = diagnostics(&prog("CLOCK: Crystal\nFREQUENCY: 8MHz\nCLOCK_OUTPUT: On"));
    assert!(!ok && t.contains("no tiene salida de reloj") && t.contains("INTOSC, RC"), "{t}");
}

#[test]
fn fail_safe_and_switching_set_their_config_bits_only_where_the_datasheet_allows() {
    let c = source("CLOCK: Crystal\nFREQUENCY: 8MHz\nFAIL_SAFE_CLOCK: On\nCLOCK_SWITCHING: On");
    assert_eq!(c.extra_config, vec![("FCMEN".to_string(), "ON".to_string()), ("IESO".to_string(), "ON".to_string())]);
    let c = source("CLOCK: External\nFREQUENCY: 8MHz\nFAIL_SAFE_CLOCK: On");
    assert_eq!(c.extra_config, vec![("FCMEN".to_string(), "ON".to_string())]);
    assert_eq!(source("CLOCK: Crystal\nFREQUENCY: 8MHz\nFAIL_SAFE_CLOCK: Off").extra_config, vec![("FCMEN".to_string(), "OFF".to_string())]);
    for (decl, frag) in [
        ("CLOCK: Internal\nFAIL_SAFE_CLOCK: On", "no aplica al modo INTOSC"),
        ("CLOCK: External\nFREQUENCY: 8MHz\nCLOCK_SWITCHING: On", "no aplica al modo EC"),
        ("CLOCK: Internal\nCLOCK_SWITCHING: On", "no aplica al modo INTOSC"),
    ] {
        let (ok, t) = diagnostics(&prog(decl));
        assert!(!ok && t.contains("PYP2194") && t.contains(frag), "{decl}: {t}");
    }
}

#[test]
fn clock_settings_need_clock_and_are_validated_as_declarations() {
    for (decl, code, frag) in [
        ("PLL: 4", "PYP2190", "acompanan a CLOCK"),
        ("CLOCK: Internal\nPLL: 4\nPLL: 4", "PYP2192", "una sola vez"),
        ("CLOCK: Internal\nCLOCK_OUTPUT: 3", "PYP2190", "On u Off"),
        ("CLOCK: Internal\nFAIL_SAFE_CLOCK: Maybe", "PYP2190", "On u Off"),
        ("CLOCK: Internal\nPLL: Maybe", "PYP2190", "Off, On o el multiplicador"),
        ("CLOCK: Internal\nCLOCK_DIVIDER: 0", "PYP2193", "mayor que 0"),
        ("CLOCK: Internal\nCLOCK_DIVIDER: 2MHz", "PYP2193", "sin unidad"),
        ("CLOCK_HZ: Const[uint32] = 4_000_000\nCLOCK_OUTPUT: On", "PYP2192", "CLOCK_HZ"),
    ] {
        let (ok, t) = diagnostics(&prog(decl));
        assert!(!ok && t.contains(code) && t.contains(frag), "{decl}: {t}");
    }
}
