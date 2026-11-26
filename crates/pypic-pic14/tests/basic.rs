//! PyPIC BASIC (estilo PICAXE, `.bas`): el mismo IR y backend que Python. Cada programa BASIC se compara con el interprete de referencia y
//! el simulador del nucleo, y con su equivalente en Python.

use std::path::PathBuf;

use pypic_device::DevicePack;
use pypic_frontend::{analyze, analyze_in, Language};
use pypic_ir::{lower, verify, Machine};
use pypic_pic14::codegen::Compiled;
use pypic_pic14::sim::Sim;
use pypic_pic14::{compile, Options};

fn pack() -> DevicePack {
    DevicePack::load(&PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../devices/pic16f887")).unwrap()
}

const HEAD: &str = "DEVICE PIC16F887\nCLOCK INTERNAL 4 MHZ\n";

fn diagnostics(src: &str) -> (bool, String) {
    let r = analyze_in(Language::Basic, src, &pack().target_profile());
    (r.program.is_some(), r.diagnostics.render(src, "t.bas"))
}

fn build(src: &str) -> (pypic_ir::Module, Compiled) {
    let r = analyze_in(Language::Basic, src, &pack().target_profile());
    let p = r.program.unwrap_or_else(|| panic!("frontend:\n{}\n{src}", r.diagnostics.render(src, "t.bas")));
    let m = lower(&p);
    verify(&m).unwrap_or_else(|e| panic!("IR invalida: {e:?}"));
    let c = compile(&m, &Options::default()).unwrap_or_else(|e| panic!("codegen: {e}\n{src}"));
    (m, c)
}

fn global(c: &Compiled, s: &Sim, name: &str) -> u16 {
    let g = c.globals.iter().find(|g| g.name == name).unwrap_or_else(|| panic!("no hay la global {name}"));
    let n = g.ty.bits().max(8) as usize / 8;
    (0..n).map(|i| (s.read(g.addr as usize + i) as u16) << (8 * i)).sum()
}

/// Corre el programa en el interprete de IR y en el simulador y devuelve cada variable pedida en los dos.
fn both(src: &str, names: &[&str], cycles: u64) -> Vec<(u16, u16)> {
    let (m, c) = build(src);
    let mut mach = Machine::new(&m);
    let _ = mach.run_main();
    let mut s = Sim::new(&c.image);
    s.run_until(cycles, |_| false);
    names
        .iter()
        .map(|n| {
            let g = m.globals.iter().position(|g| g.name == *n).unwrap_or_else(|| panic!("no hay la global {n}"));
            (mach.globals[g] as u16, global(&c, &s, n))
        })
        .collect()
}

#[test]
fn blink_in_basic_matches_blink_in_python_instruction_for_instruction() {
    let bas = format!("{HEAD}SYMBOL led = D.0\nmain:\n  HIGH led\n  PAUSE 500\n  LOW led\n  PAUSE 500\n  GOTO main\n");
    let py = "from pypic.device import PIC16F887\nfrom pypic.periph.gpio import RD0\nfrom pypic.system.time import pause\n\nCLOCK: Internal[4 * MHz]\n\n\ndef main() -> None:\n    RD0.output()\n    while True:\n        RD0.on()\n        pause(500)\n        RD0.off()\n        pause(500)\n";
    let (_, b) = build(&bas);
    let r = analyze(py, &pack().target_profile());
    let m = lower(&r.program.unwrap());
    let p = compile(&m, &Options::default()).unwrap();
    // el mismo programa: el BASIC solo agrega un GOTO al final de main en lugar del bucle while True
    assert!(b.flash_words.abs_diff(p.flash_words) <= 3, "BASIC {} palabras, Python {}", b.flash_words, p.flash_words);
    let mut s = Sim::new(&b.image);
    s.run_until(5_000, |_| false);
    assert_eq!(s.pin('D', 0), Some(true), "RD0 en alto tras el primer HIGH");
}

#[test]
fn implicit_variables_and_width_by_expression() {
    let src = format!("{HEAD}b0 = 200\nb1 = 100\nb2 = b0 + b1\nw5 = b0 + b1\nw6 = b0 * 3\nb3 = w6 / 4\nEND\n");
    let r = both(&src, &["b2", "w5", "w6", "b3"], 3_000);
    // b2 = 300 mod 256 = 44 (suma de bytes); w5 = 300 (en 16 bits porque el destino es WORD); w6 = 600; b3 = 150
    assert_eq!(r, vec![(44, 44), (300, 300), (600, 600), (150, 150)]);
}

#[test]
fn if_for_do_while_and_select_case() {
    let src = format!(
        "{HEAD}DIM total AS WORD\nDIM hits AS BYTE\nFOR b0 = 1 TO 10\n  total = total + b0\nNEXT b0\nIF total = 55 THEN\n  b1 = 1\nELSEIF total = 0 THEN\n  b1 = 2\nELSE\n  b1 = 3\nENDIF\nb2 = 0\nDO WHILE b2 < 5\n  b2 = b2 + 1\nLOOP\nb3 = 10\nWHILE b3 > 7\n  b3 = b3 - 1\nWEND\nb4 = 7\nSELECT CASE b4\n  CASE 1, 2\n    hits = 1\n  CASE 5 TO 9\n    hits = 2\n  CASE ELSE\n    hits = 3\nEND SELECT\nEND\n"
    );
    let r = both(&src, &["total", "b1", "b2", "b3", "hits"], 20_000);
    assert_eq!(r, vec![(55, 55), (1, 1), (5, 5), (7, 7), (2, 2)]);
}

#[test]
fn for_counts_down_when_start_is_greater_and_do_loop_until() {
    let src = format!("{HEAD}b1 = 0\nFOR b0 = 5 TO 1\n  b1 = b1 + b0\nNEXT\nb2 = 0\nDO\n  b2 = b2 + 3\nLOOP UNTIL b2 >= 10\nEND\n");
    let r = both(&src, &["b1", "b2"], 10_000);
    assert_eq!(r, vec![(15, 15), (12, 12)]);
}

#[test]
fn goto_and_gosub_with_return() {
    let src = format!("{HEAD}b0 = 0\nb1 = 0\nagain:\n  GOSUB bump\n  IF b0 < 5 THEN again\nEND\nbump:\n  b0 = b0 + 1\n  b1 = b1 + 2\n  RETURN\n");
    let r = both(&src, &["b0", "b1"], 10_000);
    assert_eq!(r, vec![(5, 5), (10, 10)]);
}

#[test]
fn arrays_pins_and_sdk_calls() {
    let src = format!("{HEAD}FROM pypic.periph.eeprom IMPORT EEPROM\nDIM t(4) AS BYTE\nt(0) = 7\nt(1) = t(0) + 1\nb0 = t(1)\nEEPROM.write(3, b0)\nb1 = EEPROM.read(3)\nEND\n");
    let r = both(&src, &["b0", "b1"], 20_000);
    assert_eq!(r, vec![(8, 8), (8, 8)]);
}

#[test]
fn readadc_uses_the_adc_without_start() {
    let src = format!("{HEAD}READADC 0, b0\nREADADC10 1, w1\nEND\n");
    let (m, c) = build(&src);
    let mut mach = Machine::new(&m);
    mach.adc_inputs[0] = 1000;
    mach.adc_inputs[1] = 513;
    let _ = mach.run_main();
    let mut s = Sim::new(&c.image);
    s.adc_inputs[0] = 1000;
    s.adc_inputs[1] = 513;
    s.run_until(8_000, |_| false);
    let b0 = m.globals.iter().position(|g| g.name == "b0").unwrap();
    let w1 = m.globals.iter().position(|g| g.name == "w1").unwrap();
    assert_eq!((mach.globals[b0], global(&c, &s, "b0")), (250, 250), "READADC da los ocho bits de arriba: 1000 >> 2");
    assert_eq!((mach.globals[w1], global(&c, &s, "w1")), (513, 513));
}

fn expect(src: &str, code: &str) {
    let (ok, d) = diagnostics(src);
    assert!(!ok && d.contains(code), "se esperaba {code} con:\n{src}\n{d}");
}

#[test]
fn errors_point_at_the_basic_source_with_their_own_codes() {
    expect(&format!("{HEAD}b0 = 300\n"), "PYB0040"); // no cabe en un BYTE
    expect(&format!("{HEAD}b0 = -1\n"), "PYB0040");
    expect(&format!("{HEAD}b1 = 2\nb0 = -b1\n"), "PYB0042"); // sin signo
    expect(&format!("{HEAD}b0 = 1\nw0 = 2\n"), "PYB0074"); // w0 comparte memoria con b0
    expect(&format!("{HEAD}x = 1\n"), "PYB0047"); // variable sin declarar
    expect(&format!("{HEAD}FOR b0 = 0 TO 255\nNEXT\n"), "PYB0054"); // desborda el BYTE
    expect(&format!("{HEAD}RETURN\n"), "PYB0050");
    expect(&format!("{HEAD}GOSUB nada\n"), "PYB0072");
    expect(&format!("{HEAD}x:\nGOSUB x\nb0 = 1\n"), "PYB0073"); // la subrutina no termina con RETURN
    expect(&format!("{HEAD}HIGH 3\n"), "PYB0062");
    expect(&format!("{HEAD}GOTO nada\n"), "PYP2230"); // el verificador comun: etiqueta inexistente
    expect(&format!("{HEAD}IF b0 = 1 THEN\n"), "PYB0030"); // falta ENDIF
    // un tipo equivocado lo dice el verificador de siempre, con la linea del .bas
    let (_, d) = diagnostics(&format!("{HEAD}b0 = 1\nb1 = nada\n"));
    assert!(d.contains("t.bas:4:"), "{d}");
}

// ---- los ejemplos de examples/basic: cada uno compila y se comporta como su contraparte en Python ----

fn example(rel: &str) -> String {
    std::fs::read_to_string(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/basic").join(rel)).unwrap().replace("\r\n", "\n")
}

fn run_example(rel: &str, setup: impl Fn(&mut Sim), cycles: u64) -> (Sim, Compiled) {
    let (_, c) = build(&example(rel));
    let mut s = Sim::new(&c.image);
    setup(&mut s);
    s.run_until(cycles, |_| false);
    (s, c)
}

#[test]
fn every_basic_example_compiles() {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/basic");
    let mut n = 0;
    let mut stack = vec![dir];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(d).unwrap().flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().is_some_and(|x| x == "bas") {
                build(&std::fs::read_to_string(&p).unwrap().replace("\r\n", "\n"));
                n += 1;
            }
        }
    }
    assert!(n >= 9, "se esperaban los ejemplos BASIC: {n}");
}

#[test]
fn boton_led_follows_the_button() {
    for (button, led) in [(0u8, false), (1, true)] {
        // el arranque deja en cero los latches de los puertos: la entrada se aplica despues
        let (mut s, _) = run_example("gpio/boton_led.bas", |_| {}, 200);
        s.write(0x06, button);
        s.run_until(s.cycles + 200, |_| false);
        assert_eq!(s.pin('D', 0), Some(led), "RB0 = {button}");
    }
}

#[test]
fn adc_example_lights_the_led_above_half_scale() {
    for (v, led) in [(100u16, false), (511, false), (512, true), (1000, true)] {
        let (s, _) = run_example("adc/adc.bas", |s| s.adc_inputs[0] = v, 2_000);
        assert_eq!(s.pin('D', 0), Some(led), "AN0 = {v}");
    }
}

#[test]
fn media_movil_averages_the_last_eight_readings() {
    // con la entrada fija en 1000, tras 8 vueltas la media es 250 (> 127) y RD0 queda en alto; con 100, la media es 25
    let (s, _) = run_example("adc/media_movil.bas", |s| s.adc_inputs[0] = 1000, 40_000);
    assert_eq!(s.pin('D', 0), Some(true));
    let (s, _) = run_example("adc/media_movil.bas", |s| s.adc_inputs[0] = 100, 40_000);
    assert_eq!(s.pin('D', 0), Some(false));
}

#[test]
fn eeprom_arranques_counts_boots_across_resets() {
    let (_, c) = build(&example("eeprom/eeprom_arranques.bas"));
    let mut s = Sim::new(&c.image);
    for boot in 1..=3u8 {
        s.reset();
        s.run_until(s.cycles + 6_000 * 2 + 3_000_000, |_| false);
        assert_eq!(s.eeprom[0], boot, "arranque {boot}");
    }
}

#[test]
fn comparador_example_follows_the_half_supply_reference() {
    // CVREF = VDD/2 (511,5 cuentas): RA0 por encima -> C1OUT = 0 -> RD0 en alto; por debajo -> RD0 en bajo
    for (v, led) in [(900u16, true), (100, false)] {
        let (s, _) = run_example("comparadores/comparador.bas", |s| s.adc_inputs[0] = v, 3_000);
        assert_eq!(s.pin('D', 0), Some(led), "RA0 = {v}");
    }
}

#[test]
fn pwm_example_takes_the_duty_from_the_adc() {
    let (s, _) = run_example("pwm/pwm.bas", |s| s.adc_inputs[0] = 700, 6_000);
    assert_eq!(s.read(0x15), (700 >> 2) as u8, "CCPR1L = los 8 bits altos de 700");
    assert_eq!((s.read(0x17) >> 4) & 3, (700 & 3) as u8, "DC1B = los 2 bajos");
}

#[test]
fn timer_blink_and_the_crystal_example_toggle_the_led() {
    // Timer2 cada 20 ms x 25 = 500 ms: el primer cambio de RD0 llega a los 500 ms (500 000 ciclos a 4 MHz)
    let (s, _) = run_example("temporizadores/timer_blink.bas", |_| {}, 520_000);
    assert_eq!(s.pin('D', 0), Some(true), "tras 520 ms RD0 cambio una vez");
    let (s, _) = run_example("temporizadores/timer_blink.bas", |_| {}, 480_000);
    assert_eq!(s.pin('D', 0), Some(false), "a los 480 ms todavia no");
    // cristal de 20 MHz: 2000 ms = 10 000 000 ciclos; RD0 sube al principio y baja a los 2 s
    let (mut s, _) = run_example("reloj/reloj_cristal.bas", |_| {}, 1_000);
    assert_eq!(s.pin('D', 0), Some(true));
    s.run_until(10_000_100, |_| false);
    assert_eq!(s.pin('D', 0), Some(false));
}

#[test]
fn uart_eco_example_echoes_what_it_receives() {
    let (_, c) = build(&example("uart/eco.bas"));
    let mut s = Sim::new(&c.image);
    s.run_until(2_000, |_| false);
    // un byte cada 3 000 ciclos (un caracter a 9600 baudios dura ~1 040)
    for (i, b) in [0x41u8, 0x00, 0xFF].iter().enumerate() {
        s.uart_feed(*b, 3_000 + i as u64 * 3_000, true);
    }
    s.run_until(s.cycles + 40_000, |_| false);
    let sent: Vec<u8> = s.uart_tx.iter().map(|(b, _)| *b).collect();
    assert_eq!(sent, vec![0x41, 0x00, 0xFF]);
}
