//! PyxPic BASIC (estilo PICAXE, `.bas`): el mismo IR y backend que Python. Cada programa BASIC se compara con el interprete de referencia y
//! el simulador del nucleo, y con su equivalente en Python.

use std::path::PathBuf;

use pyxpic_device::DevicePack;
use pyxpic_frontend::{analyze, analyze_in, Language};
use pyxpic_ir::{lower, verify, Machine};
use pyxpic_pic14::codegen::Compiled;
use pyxpic_pic14::sim::Sim;
use pyxpic_pic14::{compile, Options};

fn pack() -> DevicePack {
    DevicePack::load(&PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../devices/pic16f887")).unwrap()
}

const HEAD: &str = "DEVICE PIC16F887\nCLOCK INTERNAL 4 MHZ\n";

fn diagnostics(src: &str) -> (bool, String) {
    let r = analyze_in(Language::Basic, src, &pack().target_profile());
    (r.program.is_some(), r.diagnostics.render(src, "t.bas"))
}

fn build(src: &str) -> (pyxpic_ir::Module, Compiled) {
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
    let py = "from pyxpic.device import PIC16F887\nfrom pyxpic.periph.gpio import RD0\nfrom pyxpic.system.time import pause\n\nCLOCK: Internal
FREQUENCY: 4MHz\n\n\ndef main() -> None:\n    RD0.output()\n    while True:\n        RD0.on()\n        pause(500)\n        RD0.off()\n        pause(500)\n";
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
    let src = format!("{HEAD}FROM pyxpic.periph.eeprom IMPORT EEPROM\nDIM t(4) AS BYTE\nt(0) = 7\nt(1) = t(0) + 1\nb0 = t(1)\nEEPROM.write(3, b0)\nb1 = EEPROM.read(3)\nEND\n");
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

// ---- el SDK con sintaxis de BASIC: sin FROM...IMPORT, comandos sin parentesis y los comandos de PICAXE ----

#[test]
fn sertxd_sends_text_decimal_values_and_raw_bytes() {
    let (_, c) = build(&example("uart/saludo.bas"));
    let mut s = Sim::new(&c.image);
    s.run_until(40_000, |_| false);
    let sent: Vec<u8> = s.uart_tx.iter().map(|(b, _)| *b).collect();
    assert!(sent.starts_with(b"Hola PyxPic 1\r\n"), "{:?}", String::from_utf8_lossy(&sent));
    // un byte suelto y uno constante no imprimible van sin tocar
    let src = format!("{HEAD}b0 = 200\nSERTXD(b0, 7, \"a\", #b0, 255)\nEND\n");
    let (_, c) = build(&src);
    let mut s = Sim::new(&c.image);
    s.run_until(30_000, |_| false);
    let sent: Vec<u8> = s.uart_tx.iter().map(|(b, _)| *b).collect();
    assert_eq!(sent, vec![200, 7, b'a', b'2', b'0', b'0', 255]);
}

#[test]
fn read_and_write_use_the_eeprom_and_pwmout_the_ccp_modules() {
    let src = format!("{HEAD}WRITE 5, 77\nREAD 5, b0\nWRITE 6, b0 + 1\nREAD 6, b1\nEND\n");
    let r = both(&src, &["b0", "b1"], 40_000);
    assert_eq!(r, vec![(77, 77), (78, 78)]);
    let src = format!("{HEAD}w0 = 703\nPWMOUT C.2, 255, w0\nw1 = 100\nPWMOUT C.1, 255, w1\nEND\n");
    let (_, c) = build(&src);
    let mut s = Sim::new(&c.image);
    s.run_until(8_000, |_| false);
    assert_eq!((s.read(0x15), s.read(0x17) & 0x3F), (175, 0x3C), "PWM1: CCPR1L = 703 >> 2 y DC1B = 3");
    assert_eq!(s.read(0x1B), 25, "PWM2: CCPR2L = 100 >> 2");
}

#[test]
fn the_sdk_needs_no_import_and_accepts_commands_without_parentheses_in_any_case() {
    // los nombres del SDK se importan solos y no distinguen mayusculas; OBJETO.metodo argumentos equivale a OBJETO.metodo(argumentos)
    let plain = format!("{HEAD}adc.start\nw0 = adc.read(an0)\nEND\n");
    let explicit = format!("{HEAD}FROM pyxpic.periph.adc IMPORT ADC, AN0\nADC.start()\nw0 = ADC.read(AN0)\nEND\n");
    let (a, b) = (build(&plain).1, build(&explicit).1);
    assert_eq!(a.flash_words, b.flash_words, "mismo programa");
    let src = format!("{HEAD}TMR2.every_ms 20\nEND\n");
    build(&src);
    // un nombre que no es del SDK sigue siendo un error del verificador
    expect(&format!("{HEAD}b0 = FOSC_9\n"), "PYP2043");
}

#[test]
fn the_new_commands_report_clear_errors() {
    expect(&format!("{HEAD}PWMOUT D.0, 255, w0\n"), "PYB0065"); // el pin no es de un modulo PWM
    expect(&format!("{HEAD}PWMOUT C.2, 300, w0\n"), "PYB0066"); // el periodo es PR2: 0 a 255
    expect(&format!("{HEAD}PWMOUT C.2, 255, w0\nPWMOUT C.1, 200, w0\n"), "PYB0067"); // los dos comparten Timer2
    expect(&format!("{HEAD}READ 0, w0\n"), "PYB0064"); // READ da un byte
    expect(&format!("{HEAD}b0 = \"hola\"\n"), "PYB0090"); // el texto solo va en SERTXD
    expect(&format!("{HEAD}SERRXD(w0)\n"), "PYB0064");
}

// ---- interrupciones y ensamblador en linea ----

#[test]
fn on_interrupt_makes_the_label_the_interrupt_routine() {
    let src = format!("{HEAD}ON INTERRUPT tick\nENABLE INTERRUPTS\nmain:\n  b1 = b1 + 1\n  GOTO main\ntick:\n  b0 = b0 + 1\n  RETURN\n");
    let (m, c) = build(&src);
    assert!(m.functions.iter().any(|f| f.name == "tick" && f.is_interrupt), "tick es la funcion de interrupcion");
    let mut s = Sim::new(&c.image);
    s.run_until(300, |_| false);
    assert_eq!(s.read(0x0B) & 0x80, 0x80, "ENABLE INTERRUPTS sube GIE");
    for _ in 0..5 {
        s.interrupt();
        s.run_until(150, |_| false);
    }
    assert_eq!(global(&c, &s, "b0"), 5, "la rutina de interrupcion corrio cinco veces");
    assert!(global(&c, &s, "b1") > 0, "y main siguio contando");
    // DISABLE INTERRUPTS baja GIE
    let (_, c) = build(&format!("{HEAD}ENABLE INTERRUPTS\nDISABLE INTERRUPTS\nEND\n"));
    let mut s = Sim::new(&c.image);
    s.run_until(300, |_| false);
    assert_eq!(s.read(0x0B) & 0x80, 0);
}

#[test]
fn an_asm_block_is_a_subroutine_written_in_pic_as() {
    let src = format!("{HEAD}b0 = 165\nGOSUB swap\nEND\nASM swap\n    SWAPF b0,F      ; las variables se usan por su nombre\n    RETURN\nEND ASM\n");
    let r = both_asm(&src);
    assert_eq!(r, 0x5A, "SWAPF intercambia los nibbles de 0xA5");
    // una variable que solo nombra el ensamblador tambien se declara, y se escribe en minusculas aunque se nombre en mayusculas
    let src = format!("{HEAD}GOSUB poner\nEND\nASM poner\n    MOVLW 77\n    MOVWF B9\n    RETURN\nEND ASM\n");
    let (_, c) = build(&src);
    let mut s = Sim::new(&c.image);
    s.run_until(500, |_| false);
    assert_eq!(global(&c, &s, "b9"), 77);
}

fn both_asm(src: &str) -> u16 {
    let (_, c) = build(src);
    let mut s = Sim::new(&c.image);
    s.run_until(1_000, |_| false);
    global(&c, &s, "b0")
}

#[test]
fn register_bits_can_be_set_and_cleared_with_command_syntax() {
    let (_, c) = build(&format!("{HEAD}INTCON.GIE.set\nINTCON.T0IE.set\nINTCON.GIE.clear\nEND\n"));
    let mut s = Sim::new(&c.image);
    s.run_until(500, |_| false);
    assert_eq!(s.read(0x0B) & 0xA0, 0x20, "T0IE sube y GIE queda en cero");
}

#[test]
fn interrupt_and_asm_errors_are_clear() {
    expect(&format!("{HEAD}ON INTERRUPT nada\nEND\n"), "PYB0103"); // sin etiqueta
    expect(&format!("{HEAD}ON INTERRUPT tick\nGOSUB tick\nEND\ntick:\n  RETURN\n"), "PYB0106"); // a la ISR no se llama
    expect(&format!("{HEAD}ON INTERRUPT a\nON INTERRUPT b\nEND\na:\n  RETURN\nb:\n  RETURN\n"), "PYB0105");
    expect(&format!("{HEAD}ON INTERRUPT tick\nEND\ntick:\n  b0 = 1\n"), "PYB0073"); // no termina en RETURN
    expect(&format!("{HEAD}IF b0 = 1 THEN\n  ASM x\n    NOP\n  END ASM\nENDIF\n"), "PYB0104");
    expect(&format!("{HEAD}ENABLE b0\n"), "PYB0102");
    expect(&format!("{HEAD}ASM x\n  NOP\n"), "PYB0101"); // falta END ASM
                                                         // un error de ensamblador sale con la linea del .bas
    let src = format!("{HEAD}GOSUB x\nEND\nASM x\n    NOP\n    FOOBAR 1\n    RETURN\nEND ASM\n");
    let r = analyze_in(Language::Basic, &src, &pack().target_profile());
    let m = lower(&r.program.expect("el verificador no ensambla"));
    let e = match compile(&m, &Options::default()) {
        Err(e) => e.to_string(),
        Ok(_) => panic!("FOOBAR no es una instruccion"),
    };
    eprintln!("{e}");
    assert!(e.contains("linea 7:5") && e.contains("FOOBAR"), "{e}");
}

#[test]
fn the_interrupt_and_asm_examples_work() {
    let (_, c) = build(&example("temporizadores/timer_interrupcion.bas"));
    let mut s = Sim::new(&c.image);
    s.run_until(3_000, |_| false);
    assert_eq!(s.pin('D', 0), Some(false));
    for _ in 0..25 {
        s.interrupt();
        s.run_until(100, |_| false);
    }
    assert_eq!(s.pin('D', 0), Some(true), "25 avisos de Timer2 = un cambio de RD0");
    assert_eq!(global(&c, &s, "b0"), 0);
    let (_, c) = build(&example("asm-en-linea/intercambiar.bas"));
    let mut s = Sim::new(&c.image);
    s.run_until(30_000, |_| false);
    let sent: Vec<u8> = s.uart_tx.iter().map(|(b, _)| *b).collect();
    assert_eq!(sent, vec![0x5A], "SWAPF 0xA5 = 0x5A");
}

// ---- BAUD: la velocidad de la UART se declara en el encabezado ----

fn uart_report(baud: Option<u32>) -> (u32, f64) {
    let directive = baud.map(|b| format!("BAUD {b}\n")).unwrap_or_default();
    let (_, c) = build(&format!("{HEAD}{directive}b0 = 65\nSERTXD(b0)\nEND\n"));
    let u = c.uart.expect("el programa usa la UART");
    (u.baud, u.error_pct)
}

#[test]
fn baud_sets_the_uart_speed_and_defaults_to_9600() {
    for baud in [1200u32, 4800, 9600, 19200] {
        let (asked, error) = uart_report(Some(baud));
        assert_eq!(asked, baud, "la velocidad pedida es la de BAUD");
        assert!(error.abs() < 3.0, "{baud} baudios a 4 MHz: error de {error} %");
    }
    assert_eq!(uart_report(None).0, 9600, "sin BAUD son 9600");
    // BAUD tambien inicia la UART cuando el programa la usa con sus metodos
    let (_, c) = build(&format!("{HEAD}BAUD 4800\nUART.write 65\nEND\n"));
    assert_eq!(c.uart.expect("UART iniciada por BAUD").baud, 4800);
    // con otra velocidad el generador de baudios (SPBRG) queda distinto
    let spbrg = |baud| {
        let (_, c) = build(&format!("{HEAD}BAUD {baud}\nSERTXD(\"a\")\nEND\n"));
        let mut s = Sim::new(&c.image);
        s.run_until(2_000, |_| false);
        s.read(0x99)
    };
    assert_ne!(spbrg(9600), spbrg(4800));
}

#[test]
fn baud_errors_are_clear() {
    expect(&format!("{HEAD}BAUD 9600\nBAUD 4800\nSERTXD(\"a\")\n"), "PYB0107"); // repetido
    expect(&format!("{HEAD}BAUD b0\nSERTXD(\"a\")\n"), "PYB0108"); // no es una constante
    expect(&format!("{HEAD}BAUD 0\n"), "PYB0108");
    expect(&format!("{HEAD}BAUD 9600\nUART.start baud = 4800\nSERTXD(\"a\")\n"), "PYB0109");
    // dos veces la velocidad
}

#[test]
fn a_pin_is_assigned_by_its_name_like_high_and_low() {
    let words = |body: &str| -> Vec<(u32, u16)> { build(&format!("{HEAD}DIM x AS BYTE\n\nmain:\n{body}\n  GOTO main\n")).1.image.iter().collect() };
    // RB0 = 1 es HIGH RB0 y RB0 = 0 es LOW RB0, con cualquier pin RA0..RE7
    assert_eq!(words("  RB0 = 1\n  RD7 = 0"), words("  HIGH RB0\n  LOW RD7"));
    assert_eq!(words("  RB0 = 5"), words("  HIGH RB0"), "distinto de cero es alto");
    // con un valor de ejecucion: distinto de cero es alto
    let (_, c) = build(&format!("{HEAD}DIM x AS BYTE\nx = 1\n\nmain:\n  RB3 = x\n  END\n"));
    let mut s = Sim::new(&c.image);
    s.run_until(100_000, |s| s.halted);
    assert_eq!(s.pin('B', 3), Some(true));
    // un nombre de pin en minusculas vale igual, y las lecturas siguen siendo por el nombre
    let (ok, t) = diagnostics(&format!("{HEAD}DIM v AS BYTE\n\nmain:\n  rb2 = 1\n  v = RC0\n  IF RB2 = 1 THEN\n    RB0 = 0\n  ENDIF\n  GOTO main\n"));
    assert!(ok, "{t}");
    // un pin que no existe no es un pin
    let (ok, t) = diagnostics(&format!("{HEAD}main:\n  RB9 = 1\n  GOTO main\n"));
    assert!(!ok && t.contains("PYB0047"), "{t}");
}

#[test]
fn pin_names_come_from_the_device_and_depend_on_how_the_pin_is_configured() {
    let diag = |body: &str| diagnostics(&format!("{HEAD}DIM v AS BYTE\n\nmain:\n{body}\n  GOTO main\n"));
    // solo existen los pines del dispositivo: el puerto E del PIC16F887 llega a RE3
    for (body, code, frag) in [
        ("  RE5 = 1", "PYB0049", "el puerto E tiene RE0, RE1, RE2, RE3"),
        ("  HIGH RE4", "PYB0049", "no existe en PIC16F887"),
        ("  SYMBOL_BAD = 1", "PYB0047", "no existe"),
        // RE3 es solo entrada: se lee, no se escribe
        ("  RE3 = 1", "PYB0053", "RE3 es solo entrada"),
        ("  TOGGLE RE3", "PYB0053", "solo entrada"),
        // el canal analogico sale del pin: solo los pines ANn lo tienen
        ("  READADC RC1, v", "PYB0066", "los analogicos son RA0 (AN0)"),
        // una funcion a la vez: la UART toma RC6 y RC7; READADC deja el pin como analogico
        ("  SERTXD(\"x\")\n  HIGH RC6", "PYB0054", "RC6 es TX o CK de EUSART"),
        ("  SERTXD(\"x\")\n  v = RC7", "PYB0054", "RC7 es RX o DT de EUSART"),
        ("  READADC RA0, v\n  HIGH RA0", "PYB0054", "RA0 es analogico (AN0)"),
    ] {
        let (ok, t) = diag(body);
        assert!(!ok && t.contains(code) && t.contains(frag), "{body}: {t}");
    }
    // lo que si vale: RE3 se lee, un pin analogico se lee por su nombre, y los demas pines digitales se usan libres
    for body in ["  v = RE3", "  READADC RA0, v", "  READADC RE1, v\n  RB0 = 1", "  SERTXD(\"x\")\n  HIGH RB5\n  RD7 = 0"] {
        let (ok, t) = diag(body);
        assert!(ok, "{body}: {t}");
    }
    // READADC por el nombre del pin da el mismo firmware que por el numero del canal
    let words = |body: &str| -> Vec<(u32, u16)> { build(&format!("{HEAD}DIM v AS BYTE\n\nmain:\n{body}\n  GOTO main\n")).1.image.iter().collect() };
    assert_eq!(words("  READADC RA3, v"), words("  READADC 3, v"));
    assert_eq!(words("  READADC RB0, v"), words("  READADC 12, v"), "RB0 es AN12 en este dispositivo");
}

#[test]
fn pin_names_ignore_case_like_the_rest_of_basic() {
    let words = |body: &str| -> Vec<(u32, u16)> { build(&format!("{HEAD}DIM v AS BYTE\n\nmain:\n{body}\n  GOTO main\n")).1.image.iter().collect() };
    assert_eq!(words("  rb0 = 1\n  Rd7 = 0\n  READADC ra3, v"), words("  RB0 = 1\n  RD7 = 0\n  READADC RA3, v"));
    assert_eq!(words("  high rb0\n  toggle Rc2"), words("  HIGH RB0\n  TOGGLE RC2"));
    // el error nombra el pin en su forma canonica
    let (ok, t) = diagnostics(&format!("{HEAD}main:\n  re5 = 1\n  GOTO main\n"));
    assert!(!ok && t.contains("RE5 no existe en PIC16F887"), "{t}");
    let (ok, t) = diagnostics(&format!("{HEAD}main:\n  rE3 = 1\n  GOTO main\n"));
    assert!(!ok && t.contains("RE3 es solo entrada"), "{t}");
}

// ---- un caracter entre comillas es su codigo ASCII ("a" = 97) ----------------------------------------------------------------

fn words_of(src: &str) -> Vec<(u32, u16)> {
    build(src).1.image.iter().collect()
}

#[test]
fn a_one_character_string_is_its_ascii_code_in_an_expression() {
    // el mismo firmware con el caracter y con su codigo; en SERTXD sigue siendo texto
    let with = |cond: &str| format!("{HEAD}main:\n  SERRXD(b0)\n  IF b0 = {cond} THEN\n    RB0 = 1\n  ENDIF\n  GOTO main\n");
    assert_eq!(words_of(&with("\"a\"")), words_of(&with("97")));
    assert_eq!(words_of(&with("\"A\"")), words_of(&with("65")));
    assert_eq!(words_of(&with("\" \"")), words_of(&with("32")));
    // tambien en una asignacion, una constante y una comparacion distinta de la igualdad
    let a = format!("{HEAD}SYMBOL k = \"z\"\nmain:\n  b0 = \"0\"\n  IF b1 >= \"a\" THEN\n    b0 = k\n  ENDIF\n  GOTO main\n");
    let b = format!("{HEAD}SYMBOL k = 122\nmain:\n  b0 = 48\n  IF b1 >= 97 THEN\n    b0 = k\n  ENDIF\n  GOTO main\n");
    assert_eq!(words_of(&a), words_of(&b));
    // un texto de mas de un caracter en una expresion sigue siendo el error de siempre
    let (ok, t) = diagnostics(&format!("{HEAD}main:\n  SERRXD(b0)\n  IF b0 = \"ab\" THEN\n    RB0 = 1\n  ENDIF\n  GOTO main\n"));
    assert!(!ok && t.contains("PYB0090"), "{t}");
}

#[test]
fn a_uart_command_program_answers_each_key_and_drives_the_pin() {
    // el programa va aqui (no se lee de examples/): el ejemplo se edita y esta prueba debe seguir diciendo lo mismo
    let src = "' Comandos por la UART: a enciende RB0 y b o c lo apagan; cada tecla responde con el valor de RB0.\nDEVICE PIC16F887\nCLOCK INTERNAL 4 MHZ\nBAUD 9600\n\nRB0 = 0\nmain:\n  SERTXD(\"Hola PyxPic \", #RB0, CR, LF)\n  GOSUB serial\n  GOTO main\n\nserial:\n  SERRXD(b0)\n  IF b0 = \"a\" THEN\n    SERTXD(\"Boton A presionado. Valor: \", #RB0, CR, LF)\n    RB0 = 1\n  ELSEIF b0 = \"b\" THEN\n    SERTXD(\"Boton B presionado. Valor: \", #RB0, CR, LF)\n    RB0 = 0\n  ELSEIF b0 = \"c\" THEN\n    SERTXD(\"Boton C presionado. Valor: \", #RB0, CR, LF)\n    RB0 = 0\n  ENDIF\n  RETURN\n";
    let (_, c) = build(src);
    let mut s = Sim::new(&c.image);
    s.run_until(2_000_000, |_| false);
    let text = |s: &Sim, from: usize| String::from_utf8_lossy(&s.uart_tx.iter().skip(from).map(|t| t.0).collect::<Vec<_>>()).to_string();
    assert_eq!(text(&s, 0), "Hola PyxPic 0\r\n");
    for (key, pin, answer) in [(b'a', true, "Boton A presionado. Valor: 0\r\n"), (b'b', false, "Boton B presionado. Valor: 1\r\n"), (b'c', false, "Boton C presionado. Valor: 0\r\n")] {
        let before = s.uart_tx.len();
        let now = s.cycles;
        s.uart_feed(key, now + 100, true);
        s.run_until(s.cycles + 3_000_000, |_| false);
        assert!(text(&s, before).starts_with(answer), "{}: {:?}", key as char, text(&s, before));
        assert_eq!(s.pin('B', 0), Some(pin), "RB0 despues de {}", key as char);
    }
}
