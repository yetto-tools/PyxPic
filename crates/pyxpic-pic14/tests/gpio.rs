//! Pines: el mismo programa en el interprete de IR y, compilado, en el simulador PIC14.
//! Se comparan los cambios de nivel de cada pin (en orden), no solo el estado final.

use std::path::PathBuf;

use pyxpic_device::DevicePack;
use pyxpic_frontend::{analyze, TargetProfile};
use pyxpic_ir::{lower, verify, Machine, Module};
use pyxpic_pic14::codegen::{CodegenError, Compiled, Options};
use pyxpic_pic14::compile;
use pyxpic_pic14::sim::Sim;

fn pack_target() -> TargetProfile {
    DevicePack::load(&PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../devices/pic16f887")).unwrap().target_profile()
}

fn build(src: &str) -> (Module, Compiled) {
    let r = analyze(src, &pack_target());
    let p = r.program.unwrap_or_else(|| panic!("frontend:\n{}\n{src}", r.diagnostics.render(src, "t.py")));
    let m = lower(&p);
    verify(&m).unwrap_or_else(|e| panic!("IR invalida: {e:?}\n{}", pyxpic_ir::print(&m)));
    let c = compile(&m, &Options::default()).unwrap_or_else(|e| panic!("codegen: {e}\n{src}"));
    (m, c)
}

/// Eventos (pin, nivel) del oraculo de IR.
fn ir_trace<'m>(m: &'m Module, inputs: &[(&str, bool)]) -> (Vec<(String, bool)>, Machine<'m>) {
    let mut mach = Machine::new(m);
    for (p, l) in inputs {
        mach.set_input(p, *l);
    }
    mach.run_main().unwrap();
    let t = mach.pin_events.iter().map(|&(i, l)| (m.pins[i].name.clone(), l)).collect();
    (t, mach)
}

/// Eventos del simulador PIC14: se sondean los pines tras cada instruccion con la misma regla.
/// `inject` = (pc de main, valor de PORT a escribir al entrar a main) para emular entradas externas.
fn pic_trace(m: &Module, c: &Compiled, inject: Option<&[(u8, u8)]>) -> (Vec<(String, bool)>, Sim) {
    let main_addr = c.functions.iter().find(|(n, _)| n == "main").unwrap().1;
    let mut s = Sim::new(&c.image);
    let mut last: Vec<Option<bool>> = vec![None; m.pins.len()];
    let mut events = Vec::new();
    let mut injected = false;
    while !s.halted && s.cycles < 50_000_000 {
        if let (Some(w), false) = (inject, injected) {
            if s.pc as u32 == main_addr {
                for &(addr, val) in w {
                    s.write(addr as usize, val);
                }
                injected = true;
            }
        }
        s.step();
        for (i, p) in m.pins.iter().enumerate() {
            let now = s.pin(p.port, p.bit);
            if now != last[i] {
                if let Some(l) = now {
                    events.push((p.name.clone(), l));
                }
                last[i] = now;
            }
        }
    }
    assert!(s.halted, "no termino");
    (events, s)
}

fn same_trace(src: &str) -> Vec<(String, bool)> {
    let (m, c) = build(src);
    let (want, _) = ir_trace(&m, &[]);
    let (got, _) = pic_trace(&m, &c, None);
    assert_eq!(got, want, "trazas distintas en:\n{src}\n{}", c.listing);
    got
}

#[test]
fn toggling_and_writing_pins_matches_the_oracle() {
    let t = same_trace(
        "from pyxpic.periph.gpio import RD0, RB0, RA1\n\
         def main() -> None:\n    RD0.output()\n    RB0.output()\n    RA1.output()\n\
         \x20   for i in range(10):\n        RD0.toggle()\n        if i > 4:\n            RB0.on()\n        else:\n            RB0.off()\n\
         \x20   RD0.value(1)\n    x: u8 = 0\n    RB0.value(x)\n    RB0.value(True)\n    RA1.value(7)\n    RA1.off()\n",
    );
    // cada pin aparece (al pasar a salida y al cambiar) y el orden coincide con el oraculo
    assert!(t.iter().any(|(p, l)| p == "RD0" && *l) && t.iter().any(|(p, l)| p == "RB0" && !*l));
}

#[test]
fn direction_changes_are_traced_too() {
    // salida -> entrada -> salida: al volver a ser salida reaparece el nivel del latch
    same_trace(
        "from pyxpic.periph.gpio import RC3\n\
         def main() -> None:\n    RC3.output()\n    RC3.on()\n    RC3.input()\n    RC3.toggle()\n    RC3.output()\n    RC3.toggle()\n",
    );
}

#[test]
fn variable_values_drive_pins_without_glitches() {
    let (m, c) = build(
        "from pyxpic.periph.gpio import RD1\nn: u8 = 0\n\
         def main() -> None:\n    global n\n    RD1.output()\n    for i in range(6):\n        n = n + 1\n        RD1.value(n & 1)\n        RD1.value(n & 2)\n",
    );
    let (want, _) = ir_trace(&m, &[]);
    let (got, _) = pic_trace(&m, &c, None);
    assert_eq!(got, want);
    // dentro de cada value() no hay un nivel intermedio: nunca dos eventos iguales seguidos del mismo pin
    for w in got.windows(2) {
        assert_ne!(w[0], w[1], "evento repetido: {w:?}");
    }
}

#[test]
fn reading_an_input_pin_matches_the_oracle() {
    let (m, c) = build(
        "from pyxpic.periph.gpio import RA0, RD2\nseen: u8 = 9\nflag: bool = False\n\
         def main() -> None:\n    global seen, flag\n    RA0.input()\n    RD2.output()\n    seen = RA0.value()\n    flag = RA0.value() == 1\n    if seen == 1:\n        RD2.on()\n",
    );
    for level in [false, true] {
        let (want, mach) = ir_trace(&m, &[("RA0", level)]);
        // el simulador: la entrada externa se emula escribiendo el latch de PORTA al entrar a main
        let (got, s) = pic_trace(&m, &c, Some(&[(0x05, level as u8)]));
        assert_eq!(got, want, "nivel {level}");
        let seen = c.globals.iter().find(|g| g.name == "seen").unwrap();
        assert_eq!(s.read(seen.addr as usize) as i128, mach.global("seen").unwrap(), "nivel {level}");
    }
}

#[test]
fn startup_clears_port_latches_and_pin_cfg_sets_tris_ansel_and_keeps_bank0() {
    let (_, c) = build(
        "from pyxpic.periph.gpio import RB0, RD0, RE3\n\
         def main() -> None:\n    RB0.output()\n    RD0.output()\n    RE3.input()\n",
    );
    let mut s = Sim::new(&c.image);
    // basura en los latches antes del arranque: el startup debe dejarlos en 0
    s.write(0x06, 0xFF);
    s.write(0x08, 0xFF);
    s.run_until(100_000, |s| s.halted);
    assert!(s.halted);
    assert_eq!(s.read(0x06), 0, "PORTB");
    assert_eq!(s.read(0x08), 0, "PORTD");
    assert_eq!(s.read(0x86) & 1, 0, "TRISB<0> = salida");
    assert_eq!(s.read(0x88) & 1, 0, "TRISD<0> = salida");
    // RB0 es AN12 -> ANSELH bit 4 pasa a digital; el resto sigue analogico (0x3F)
    assert_eq!(s.read(0x189), 0x3F & !0x10, "ANSELH");
    assert_eq!(s.read(0x188), 0xFF, "ANSEL intacto: ni RD0 ni RE3 son analogicos de ANSEL en este programa");
    assert_eq!(s.read(0x89) & 0x08, 0x08, "TRISE<3> sigue como entrada");
    assert_eq!(s.read(0x03) & 0x60, 0, "banco 0 restaurado (RP1:RP0 = 00)");
}

#[test]
fn analog_pins_in_ansel_are_cleared() {
    let (_, c) = build("from pyxpic.periph.gpio import RA0, RA5\ndef main() -> None:\n    RA0.output()\n    RA5.input()\n");
    let s = {
        let mut s = Sim::new(&c.image);
        s.run_until(100_000, |s| s.halted);
        s
    };
    // RA0 = AN0, RA5 = AN4
    assert_eq!(s.read(0x188), !0x01 & !0x10, "ANSEL");
}

#[test]
fn unsupported_port_is_reported() {
    let r = analyze("from pyxpic.periph.gpio import RF0\ndef main() -> None:\n    RF0.output()\n", &TargetProfile::generic());
    let m = lower(&r.program.unwrap());
    let e = compile(&m, &Options::default()).unwrap_err();
    assert!(matches!(&e, CodegenError::Unsupported(s) if s.contains("RF0") && s.contains("A a E")), "{e}");
}

#[test]
fn ir_dump_names_the_pin_operations() {
    let (m, _) = build("from pyxpic.periph.gpio import RD0\ndef main() -> None:\n    RD0.output()\n    RD0.toggle()\n    RD0.on()\n");
    let out = pyxpic_ir::print(&m);
    assert!(out.contains("pin RD0 (puerto D, bit 0)") && out.contains("pin RD0 = output") && out.contains("pin RD0 toggle"), "{out}");
}
