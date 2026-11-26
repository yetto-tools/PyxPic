//! EEPROM de datos (`pypic.periph.eeprom`, #45): lectura y escritura contra el interprete de referencia, la secuencia 55h/AAh con las
//! interrupciones, el tiempo de escritura (D122), WRERR, la persistencia entre reinicios, las reglas del lenguaje y los costos.

use std::path::PathBuf;

use pypic_device::DevicePack;
use pypic_frontend::analyze;
use pypic_ir::{lower, verify, Machine};
use pypic_pic14::codegen::Compiled;
use pypic_pic14::sim::Sim;
use pypic_pic14::{compile, Options};

fn pack() -> DevicePack {
    DevicePack::load(&PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../devices/pic16f887")).unwrap()
}

fn head() -> String {
    "from pypic.device import PIC16F887\nfrom pypic.periph.eeprom import EEPROM\nfrom pypic.regs import INTCON, EECON1, PIR2\nfrom pypic import interrupt\nCLOCK_HZ: Const[uint32] = 4_000_000\n".to_string()
}

fn diagnostics(src: &str) -> (bool, String) {
    let r = analyze(src, &pack().target_profile());
    (r.program.is_some(), r.diagnostics.render(src, "t.py"))
}

fn build(src: &str) -> (pypic_ir::Module, Compiled) {
    let r = analyze(src, &pack().target_profile());
    let p = r.program.unwrap_or_else(|| panic!("frontend:\n{}\n{src}", r.diagnostics.render(src, "t.py")));
    let m = lower(&p);
    verify(&m).unwrap_or_else(|e| panic!("IR invalida: {e:?}"));
    let c = compile(&m, &Options::default()).unwrap_or_else(|e| panic!("codegen: {e}\n{src}"));
    (m, c)
}

fn prog(globals: &str, body: &str) -> String {
    format!("{}{globals}\n\n\ndef main() -> None:\n{body}    while True:\n        pass\n", head())
}

fn run(src: &str, cycles: u64) -> Sim {
    let (_, c) = build(src);
    let mut s = Sim::new(&c.image);
    s.run_until(cycles, |_| false);
    s
}

fn global(c: &Compiled, s: &Sim, name: &str) -> u8 {
    let g = c.globals.iter().find(|g| g.name == name).unwrap_or_else(|| panic!("no hay la global {name}"));
    s.read(g.addr as usize)
}

const EECON1: usize = 0x18C;
const PIR2: usize = 0x0D;
const INTCON: usize = 0x0B;
/// 6 ms (D122) a 4 MHz: 6000 ciclos de instruccion.
const T_WRITE: u64 = 6_000;

// ---- lectura y escritura: simulador contra interprete -----------------------------------------------------------------------

#[test]
fn write_then_read_matches_the_reference_interpreter() {
    let src = prog(
        "a: uint8 = 0\nb: uint8 = 0\nc: uint8 = 0\nd: uint8 = 0\nk: uint8 = 7\n",
        "    global a, b, c, d\n    EEPROM.write(0, 0xA5)\n    EEPROM.write(255, 0x3C)\n    EEPROM.write(k, 0x42)\n    a = EEPROM.read(0)\n    b = EEPROM.read(255)\n    c = EEPROM.read(k)\n    d = EEPROM.read(100)\n",
    );
    let (m, c) = build(&src);
    let mut mach = Machine::new(&m);
    let _ = mach.run_main();
    let mut s = Sim::new(&c.image);
    s.run_until(3 * T_WRITE + 2_000, |_| false);
    for (name, want) in [("a", 0xA5u8), ("b", 0x3C), ("c", 0x42), ("d", 0xFF)] {
        assert_eq!(global(&c, &s, name), want, "simulador: {name}");
        let g = m.globals.iter().position(|g| g.name == name).unwrap();
        assert_eq!(mach.globals[g] as u8, want, "interprete: {name}");
    }
    assert_eq!((s.eeprom[0], s.eeprom[255], s.eeprom[7], s.eeprom[100]), (0xA5, 0x3C, 0x42, 0xFF), "una EEPROM borrada vale 0xFF");
    assert_eq!((mach.eeprom[0], mach.eeprom[255], mach.eeprom[7]), (0xA5, 0x3C, 0x42));
    assert!(s.eeprom_violations.is_empty(), "{:?}", s.eeprom_violations);
}

#[test]
fn every_address_and_every_value_round_trips() {
    // 256 direcciones con un valor distinto cada una, leidas con un indice variable
    let src = prog(
        "i: uint8 = 0\nsum: uint16 = 0\nok: uint8 = 1\n",
        "    global i, sum, ok\n    i = 0\n    while True:\n        EEPROM.write(i, i ^ 0x5A)\n        if i == 255:\n            break\n        i += 1\n    i = 0\n    while True:\n        if EEPROM.read(i) != (i ^ 0x5A):\n            ok = 0\n        sum += uint16(EEPROM.read(i))\n        if i == 255:\n            break\n        i += 1\n",
    );
    let (_, c) = build(&src);
    let mut s = Sim::new(&c.image);
    s.run_until(256 * (T_WRITE + 120) + 60_000, |_| false);
    assert_eq!(global(&c, &s, "ok"), 1);
    assert!((0..256usize).all(|a| s.eeprom[a] == (a as u8) ^ 0x5A));
    assert!(s.eeprom_violations.is_empty(), "{:?}", s.eeprom_violations);
}

#[test]
fn the_eeprom_survives_a_reset_but_a_ram_variable_does_not() {
    let (_, c) = build(&prog("n: uint8 = 3\nv: uint8 = 0\n", "    global n, v\n    v = EEPROM.read(5)\n    EEPROM.write(5, v + 1)\n    n += 1\n"));
    let mut s = Sim::new(&c.image);
    s.run_until(T_WRITE + 2_000, |_| false);
    assert_eq!((s.eeprom[5], global(&c, &s, "n")), (0x00, 4), "primer arranque: 0xFF + 1 = 0");
    for boot in 1..=4u8 {
        s.reset();
        s.run_until(s.cycles + T_WRITE + 2_000, |_| false);
        assert_eq!(s.eeprom[5], boot, "arranque {}", boot + 1);
        assert_eq!(global(&c, &s, "n"), 4, "la RAM se reinicia");
    }
}

// ---- escribir bloquea y start_write no ----------------------------------------------------------------------------------------

/// Programa con una marca en RD0 antes y despues de `body` para medir ciclos.
fn cycles_between(body: &str) -> u64 {
    let src = format!("{}from pypic.periph.gpio import RD0\n\nz: uint8 = 0\n\n\ndef main() -> None:\n    global z\n    RD0.output()\n    RD0.on()\n{body}    RD0.off()\n    while True:\n        pass\n", head());
    let (_, c) = build(&src);
    let mut s = Sim::new(&c.image);
    let (mut on, mut off) = (None, None);
    while s.cycles < 20_000 && off.is_none() {
        s.step();
        let rd0 = s.read(0x08) & 1 != 0;
        if rd0 && on.is_none() {
            on = Some(s.cycles);
        }
        if !rd0 && on.is_some() && off.is_none() {
            off = Some(s.cycles);
        }
    }
    off.unwrap() - on.unwrap()
}

#[test]
fn write_blocks_for_the_write_time_and_start_write_does_not() {
    let base = cycles_between("");
    let start = cycles_between("    EEPROM.start_write(3, 0x77)\n") - base;
    let full = cycles_between("    EEPROM.write(3, 0x77)\n") - base;
    assert!(start < 60, "start_write vuelve enseguida: {start} ciclos");
    assert!((T_WRITE..T_WRITE + 40).contains(&full), "write espera el tiempo de escritura (6 ms = {T_WRITE} ciclos): {full}");
    eprintln!("COSTOS eeprom: start_write {start}, write {full}");
}

#[test]
fn start_write_leaves_busy_until_the_cycle_ends_and_then_sets_eeif() {
    let (_, c) = build(&prog("b0: uint8 = 9\nb1: uint8 = 9\n", "    global b0, b1\n    EEPROM.start_write(9, 0x11)\n    if EEPROM.busy():\n        b0 = 1\n    else:\n        b0 = 0\n    while EEPROM.busy():\n        pass\n    if EEPROM.busy():\n        b1 = 1\n    else:\n        b1 = 0\n"));
    let mut s = Sim::new(&c.image);
    s.run_until(200, |_| false);
    assert_eq!(global(&c, &s, "b0"), 1, "busy() es verdadero justo despues de start_write");
    assert_eq!(s.eeprom[9], 0xFF, "el byte aun no cambio durante la escritura");
    assert_eq!(s.read(PIR2) & 0x10, 0, "EEIF aun en 0");
    s.run_until(T_WRITE + 400, |_| false);
    assert_eq!(global(&c, &s, "b1"), 0, "busy() baja al terminar");
    assert_eq!(s.eeprom[9], 0x11);
    assert_ne!(s.read(PIR2) & 0x10, 0, "EEIF sube al terminar la escritura");
    assert_eq!(s.read(EECON1) & 0x06, 0, "WR baja por hardware y WREN ya estaba en 0");
    assert!(s.eeprom_violations.is_empty(), "{:?}", s.eeprom_violations);
}

#[test]
fn write_clears_eeif_at_the_start_and_wren_stays_off() {
    let s = run(&prog("", "    EEPROM.write(1, 1)\n    PIR2.EEIF.clear()\n    EEPROM.write(2, 2)\n"), 2 * T_WRITE + 2_000);
    // la segunda escritura vuelve a subir EEIF; en reposo WREN esta en 0 (proteccion contra escrituras accidentales, 10.4)
    assert_ne!(s.read(PIR2) & 0x10, 0);
    assert_eq!(s.read(EECON1) & 0x04, 0, "WREN en 0 al terminar");
    assert_eq!((s.eeprom[1], s.eeprom[2]), (1, 2));
}

// ---- la secuencia obligatoria y las interrupciones ----------------------------------------------------------------------------

#[test]
fn a_write_without_the_unlock_sequence_does_nothing_and_the_simulator_says_so() {
    // WR = 1 a mano, sin 55h/AAh: el datasheet dice que no se inicia (10.1.3)
    let s = run(&prog("", "    EECON1.WREN.set()\n    EECON1.WR.set()\n"), 3_000);
    assert_eq!(s.eeprom[0], 0xFF);
    assert!(s.eeprom_violations.iter().any(|v| v.contains("falta la secuencia")), "{:?}", s.eeprom_violations);
    assert_eq!(s.read(EECON1) & 0x02, 0, "WR queda en 0: no hay escritura");
    // y con la secuencia pero WREN = 0 tampoco
    let s = run(&prog("", "    EECON1.WREN.clear()\n    EECON1.WR.set()\n"), 3_000);
    assert!(s.eeprom_violations.iter().any(|v| v.contains("WREN = 0")), "{:?}", s.eeprom_violations);
}

#[test]
fn write_restores_gie_only_if_it_was_set() {
    for (gie_before, label) in [(true, "GIE = 1"), (false, "GIE = 0")] {
        let body = format!("{}    EEPROM.write(4, 0x99)\n", if gie_before { "    INTCON.GIE.set()\n" } else { "" });
        let s = run(&prog("", &body), T_WRITE + 2_000);
        assert_eq!(s.read(INTCON) & 0x80 != 0, gie_before, "{label}: GIE queda como estaba");
        assert_eq!(s.eeprom[4], 0x99, "{label}");
        assert!(s.eeprom_violations.is_empty(), "{label}: {:?}", s.eeprom_violations);
    }
}

#[test]
fn an_interrupt_at_any_instruction_never_breaks_the_unlock_sequence() {
    let src = prog("n: uint8 = 0\n", "    INTCON.GIE.set()\n    EEPROM.write(10, 0x5A)\n").replacen("def main()", "@interrupt\ndef isr() -> None:\n    global n\n    n += 1\n\n\ndef main()", 1);
    let (_, c) = build(&src);
    let main_addr = c.functions.iter().find(|(n, _)| n == "main").unwrap().1;
    let mut probe = Sim::new(&c.image);
    let mut start = 0u64;
    while probe.pc as u32 != main_addr {
        probe.step();
        start += 1;
    }
    let (mut hits, mut delivered) = (0, 0);
    // las primeras ~70 instrucciones de main cubren la configuracion, la secuencia y la entrada a la espera
    for n in start..start + 70 {
        let mut s = Sim::new(&c.image);
        for _ in 0..n {
            s.step();
        }
        // una interrupcion solo ocurre con GIE = 1: el simulador la fuerza, asi que se respeta GIE como lo hace el hardware
        if s.read(INTCON) & 0x80 != 0 {
            s.interrupt();
            delivered += 1;
        }
        s.run_until(T_WRITE + 3_000, |_| false);
        assert_eq!(s.eeprom[10], 0x5A, "interrupcion tras {n} instrucciones");
        assert!(s.eeprom_violations.is_empty(), "interrupcion tras {n}: {:?}", s.eeprom_violations);
        assert_ne!(s.read(INTCON) & 0x80, 0, "GIE vuelve a 1 tras {n} instrucciones");
        hits += global(&c, &s, "n") as u32;
    }
    assert!(delivered > 10 && hits > 0, "se entregaron {delivered} interrupciones");
    // con GIE = 1 la secuencia (de GIE a 0 hasta WR) no admite ninguna: no hay una instruccion en esa ventana con GIE = 1
}

#[test]
fn a_forced_interrupt_in_the_middle_of_the_sequence_is_flagged() {
    // el contrato en el simulador: si algo interrumpe entre 55h y WR, la secuencia se pierde y se anota
    let (_, c) = build(&prog("", "    EEPROM.write(10, 0x5A)\n"));
    let mut found = false;
    for n in 0..120u64 {
        let mut s = Sim::new(&c.image);
        for _ in 0..n {
            s.step();
        }
        s.interrupt();
        s.run_until(2_000, |_| false);
        found |= !s.eeprom_violations.is_empty();
    }
    assert!(found, "una interrupcion forzada entre 55h y WR se detecta");
}

// ---- WRERR: un reinicio a mitad de la escritura ------------------------------------------------------------------------------

#[test]
fn a_reset_in_the_middle_of_a_write_sets_wrerr_and_leaves_the_old_byte() {
    let (_, c) = build(&prog("f: uint8 = 9\n", "    global f\n    if EEPROM.failed():\n        f = 1\n    else:\n        f = 0\n    EEPROM.write(6, 0x44)\n"));
    let mut s = Sim::new(&c.image);
    s.eeprom[6] = 0x10;
    s.run_until(2_000, |_| false); // a mitad del tiempo de escritura
    assert_eq!(s.eeprom[6], 0x10);
    s.reset();
    assert_ne!(s.read(EECON1) & 0x08, 0, "WRERR = 1 tras un reinicio durante la escritura");
    assert_eq!(s.eeprom[6], 0x10, "el contenido anterior se conserva");
    s.run_until(s.cycles + 200, |_| false);
    assert_eq!(global(&c, &s, "f"), 1, "failed() lo informa tras el reinicio");
    s.run_until(s.cycles + T_WRITE + 400, |_| false);
    assert_eq!(s.eeprom[6], 0x44, "reescribir termina bien");
    assert_eq!(s.read(EECON1) & 0x08, 0, "write baja WRERR al empezar");
}

// ---- las reglas del lenguaje --------------------------------------------------------------------------------------------------

#[test]
fn language_rules_are_located_errors() {
    let h = head();
    let wrap = |body: &str| format!("{h}\n\ndef main() -> None:\n{body}    pass\n");
    for (src, code, frag) in [
        (wrap("    EEPROM.read()\n"), "PYP2055", "una direccion"),
        (wrap("    EEPROM.write(1)\n"), "PYP2055", "la direccion (0 a 255) y el valor"),
        (wrap("    EEPROM.busy(1)\n"), "PYP2055", "ningun argumento"),
        (wrap("    EEPROM.borrar(1)\n"), "PYP2212", "disponibles"),
        (wrap("    EEPROM.write(addr=1, value=2)\n"), "PYP2211", "argumentos con nombre"),
        (wrap("    x = EEPROM\n"), "PYP2213", "memoria de datos"),
        (wrap("    EEPROM.write(256, 1)\n"), "PYP20", ""),
        (wrap("    EEPROM.write(1, 300)\n"), "PYP20", ""),
        (wrap("    n: uint16 = 5\n    EEPROM.read(n)\n"), "PYP2041", "uint8"),
        (wrap("    x: uint8 = EEPROM.busy()\n"), "PYP20", ""),
        (wrap("    x = EEPROM.write(1, 2)\n"), "PYP20", ""),
    ] {
        let (ok, t) = diagnostics(&src);
        assert!(!ok && t.contains(code) && t.contains(frag), "{src}\n{t}");
    }
    let (ok, t) = diagnostics("from pypic.device import PIC16F887\nfrom pypic.periph.eeprom import ROM\n\ndef main() -> None:\n    pass\n");
    assert!(!ok && t.contains("PYP2210") && t.contains("exporta: EEPROM"), "{t}");
    // el modulo existe solo si el dispositivo lo trae
    let mut target = pack().target_profile();
    target.peripherals = Some(["ADC".to_string()].into_iter().collect());
    let r = analyze("from pypic.periph.eeprom import EEPROM\n\ndef main() -> None:\n    pass\n", &target);
    let t = r.diagnostics.render("", "t.py");
    assert!(r.program.is_none() && t.contains("PYP2210") && t.contains("no trae"), "{t}");
}

// ---- costos fijos (medidos en el simulador) -----------------------------------------------------------------------------------

#[test]
fn read_busy_and_failed_costs_are_fixed() {
    let base = cycles_between("");
    let cost = |body: &str| cycles_between(body) - base;
    let read_const = cost("    z = EEPROM.read(3)\n");
    let read_var = cost("    z = EEPROM.read(z)\n");
    let busy = cost("    if EEPROM.busy():\n        z = 1\n");
    let failed = cost("    if EEPROM.failed():\n        z = 1\n");
    eprintln!("COSTOS eeprom: read(const) {read_const}, read(var) {read_var}, busy {busy}, failed {failed}");
    assert!(read_const >= 7 && read_const <= 14, "read: {read_const}");
    assert!(read_var >= read_const && read_var - read_const <= 2, "read variable: {read_var}");
    assert!(busy >= 6 && busy <= 14 && failed >= 6 && failed <= 14, "busy {busy}, failed {failed}");
    // no depende de la direccion
    for a in [0u8, 1, 127, 255] {
        assert_eq!(cost(&format!("    z = EEPROM.read({a})\n")), read_const, "read({a})");
    }
}

// ---- el ejemplo ------------------------------------------------------------------------------------------------------------------

#[test]
fn the_boot_counter_example_counts_across_power_cycles_and_blinks_that_many_times() {
    let src = std::fs::read_to_string(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/python/eeprom/eeprom_arranques.py")).unwrap();
    let (_, c) = build(&src);
    let mut s = Sim::new(&c.image);
    for boot in 1..=4u8 {
        s.reset();
        let (mut last, mut rising) = (false, 0u32);
        let end = s.cycles + 300_000 + u64::from(boot) * 410_000;
        while s.cycles < end {
            s.step();
            let rd0 = s.read(0x08) & 1 != 0;
            if rd0 && !last {
                rising += 1;
            }
            last = rd0;
        }
        assert_eq!(s.eeprom[0], boot, "arranque {boot}: la EEPROM guarda el contador");
        assert_eq!(rising, u32::from(boot), "arranque {boot}: parpadea {boot} veces");
    }
    assert!(s.eeprom_violations.is_empty(), "{:?}", s.eeprom_violations);
}

#[test]
fn a_write_inside_an_interrupt_does_not_turn_gie_on() {
    // dentro de la interrupcion GIE ya esta en 0: la secuencia no debe encenderlo (lo hace el RETFIE, no la escritura)
    let src = format!("{}\nseen: uint8 = 0\n\n\n@interrupt\ndef isr() -> None:\n    global seen\n    EEPROM.write(20, 0x31)\n    seen = INTCON.read() >> 7\n\n\ndef main() -> None:\n    INTCON.GIE.set()\n    while True:\n        pass\n", head());
    let (_, c) = build(&src);
    let mut s = Sim::new(&c.image);
    s.run_until(200, |_| false);
    s.interrupt();
    s.run_until(s.cycles + T_WRITE + 2_000, |_| false);
    assert_eq!(s.eeprom[20], 0x31);
    assert_eq!(global(&c, &s, "seen"), 0, "GIE seguia en 0 dentro de la interrupcion, tras la escritura");
    assert_ne!(s.read(INTCON) & 0x80, 0, "el RETFIE lo vuelve a habilitar");
    assert!(s.eeprom_violations.is_empty(), "{:?}", s.eeprom_violations);
}
