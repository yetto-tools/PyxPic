//! Registros de `pyxpic.regs`: prueba diferencial entre el interprete de la IR y el simulador del nucleo.
//! Se usan registros de los cuatro bancos; ambos deben dejar los mismos valores en cada registro y en las globales.

use std::path::PathBuf;

use pyxpic_device::DevicePack;
use pyxpic_frontend::{analyze, TargetProfile};
use pyxpic_ir::{lower, verify, Machine, Module};
use pyxpic_pic14::codegen::{Compiled, Options};
use pyxpic_pic14::compile;
use pyxpic_pic14::sim::Sim;

fn pack_target() -> TargetProfile {
    DevicePack::load(&PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../devices/pic16f887")).unwrap().target_profile()
}

fn build(src: &str) -> (Module, Compiled) {
    let r = analyze(src, &pack_target());
    let p = r.program.unwrap_or_else(|| panic!("frontend:\n{}\n{src}", r.diagnostics.render(src, "t.py")));
    let m = lower(&p);
    verify(&m).unwrap_or_else(|e| panic!("IR invalida: {e:?}"));
    let c = compile(&m, &Options::default()).unwrap_or_else(|e| panic!("codegen: {e}\n{src}"));
    (m, c)
}

fn errors(src: &str) -> String {
    let r = analyze(src, &pack_target());
    assert!(r.program.is_none(), "debia rechazarse:\n{src}");
    r.diagnostics.render(src, "t.py")
}

/// Ejecuta en la IR y en el simulador y compara globales y registros tocados.
fn differential(src: &str) -> (Machine<'static>, Sim) {
    let (m, c) = build(src);
    let m: &'static Module = Box::leak(Box::new(m));
    let mut mach = Machine::new(m);
    mach.run_main().unwrap();
    let mut s = Sim::new(&c.image);
    s.run_until(1_000_000, |s| s.halted);
    assert!(s.halted, "el programa no termino");
    for g in c.globals.iter().filter(|g| !g.is_const) {
        let n = g.ty.bits() as usize / 8;
        let v: i128 = (0..n).map(|i| (s.read(g.addr as usize + i) as i128) << (8 * i)).sum();
        assert_eq!(g.ty.wrap(v), mach.global(&g.name).unwrap(), "global {}\n{src}", g.name);
    }
    for (&addr, &v) in &mach.regs {
        assert_eq!(s.read(addr as usize), v, "registro {addr:#05x}\n{src}");
    }
    (mach, s)
}

const HEAD: &str = "from pyxpic.device import PIC16F887\nfrom pyxpic.regs import TRISB, TRISC, PR2, ANSEL, EEDAT, STATUS, INTCON, OPTION_REG\n";

#[test]
fn write_and_read_in_every_bank() {
    // TRISB y TRISC: banco 1; PR2: banco 1; EEDAT: banco 2; ANSEL: banco 3
    let src = format!(
        "{HEAD}a: u8 = 0\nb: u8 = 0\nc: u8 = 0\nd: u8 = 0\ndef main() -> None:\n    global a, b, c, d\n    TRISB.write(0xA5)\n    PR2.write(0x3C)\n    EEDAT.write(0x77)\n    ANSEL.write(0x81)\n    a = TRISB.read()\n    b = PR2.read()\n    c = EEDAT.read()\n    d = ANSEL.read()\n"
    );
    let (mach, s) = differential(&src);
    assert_eq!(mach.regs[&0x86], 0xA5);
    assert_eq!(s.read(0x188), 0x81);
}

#[test]
fn bit_set_clear_toggle_value() {
    let src = format!(
        "{HEAD}x: u8 = 0\ny: u8 = 0\nz: u8 = 0\ndef main() -> None:\n    global x, y, z\n    TRISC.write(0x0F)\n    TRISC.TRISC7.set()\n    TRISC.TRISC0.clear()\n    TRISC.TRISC3.toggle()\n    TRISC.TRISC4.toggle()\n    x = TRISC.read()\n    y = TRISC.TRISC7.value()\n    z = TRISC.TRISC0.value()\n"
    );
    let (mach, _) = differential(&src);
    assert_eq!(mach.regs[&0x87], 0b1001_0110);
    assert_eq!(mach.global("y").unwrap(), 1);
    assert_eq!(mach.global("z").unwrap(), 0);
}

#[test]
fn bits_in_other_banks_and_status_flags() {
    let src = format!(
        "{HEAD}a: u8 = 0\nb: u8 = 0\nc: u8 = 0\ndef main() -> None:\n    global a, b, c\n    ANSEL.write(0)\n    ANSEL.ANS5.set()\n    ANSEL.ANS0.toggle()\n    OPTION_REG.write(0xFF)\n    OPTION_REG.T0CS.clear()\n    a = ANSEL.read()\n    b = OPTION_REG.read()\n    c = INTCON.GIE.value()\n"
    );
    let (mach, _) = differential(&src);
    assert_eq!(mach.regs[&0x188], 0b0010_0001);
    assert_eq!(mach.regs[&0x81], 0xDF);
}

#[test]
fn registers_compose_with_expressions_and_loops() {
    let src = format!(
        "{HEAD}n: u8 = 0\ndef main() -> None:\n    global n\n    EEDAT.write(0)\n    i: u8 = 0\n    while i < 5:\n        EEDAT.write(EEDAT.read() + i)\n        i += 1\n    if EEDAT.EEDAT0.value() == 0:\n        n = 1\n    n += EEDAT.read()\n"
    );
    let (mach, _) = differential(&src);
    assert_eq!(mach.regs[&0x10C], 10);
    assert_eq!(mach.global("n").unwrap(), 11);
}

#[test]
fn returns_to_bank_zero_after_each_access() {
    // despues de tocar un registro de otro banco, las variables (banco 0) siguen siendo accesibles
    let src = format!("{HEAD}v: u8 = 0\ndef main() -> None:\n    global v\n    ANSEL.write(0x12)\n    v = 7\n    v += ANSEL.read()\n");
    let (mach, _) = differential(&src);
    assert_eq!(mach.global("v").unwrap(), 0x19);
}

#[test]
fn rejects_what_the_hardware_or_the_compiler_cannot_give() {
    let imp = "from pyxpic.device import PIC16F887\n";
    let e = errors(&format!("{imp}from pyxpic.regs import NOEXISTE\n"));
    assert!(e.contains("PYP2130") && e.contains("registros:"), "{e}");
    let e = errors(&format!("{imp}from pyxpic.regs import INDF\n"));
    assert!(e.contains("PYP2130") && e.contains("no es un registro fisico"), "{e}");
    let e = errors(&format!("{imp}from pyxpic.regs import STATUS\ndef main() -> None:\n    STATUS.write(0)\n"));
    assert!(e.contains("PYP2135"), "{e}");
    let e = errors(&format!("{imp}from pyxpic.regs import STATUS\ndef main() -> None:\n    STATUS.RP0.set()\n"));
    assert!(e.contains("PYP2135") && e.contains("banco"), "{e}");
    let e = errors(&format!("{imp}from pyxpic.regs import STATUS\ndef main() -> None:\n    STATUS.NOBIT.set()\n"));
    assert!(e.contains("PYP2133") && e.contains("bits:"), "{e}");
    let e = errors(&format!("{imp}from pyxpic.regs import TRISB\ndef main() -> None:\n    TRISB.on()\n"));
    assert!(e.contains("PYP2134"), "{e}");
    let e = errors(&format!("{imp}from pyxpic.regs import TRISB\ndef main() -> None:\n    x: u8 = TRISB\n"));
    assert!(e.contains("PYP2136"), "{e}");
    let e = errors(&format!("{imp}from pyxpic.regs import TRISB\ndef main() -> None:\n    TRISB.write(300)\n"));
    assert!(e.contains("error"), "{e}");
}

#[test]
fn status_flags_can_be_read_and_cleared() {
    let src = format!("{HEAD}z: u8 = 0\ndef main() -> None:\n    global z\n    STATUS.C.set()\n    STATUS.DC.clear()\n    z = STATUS.C.value()\n");
    // STATUS lo modifican las propias instrucciones (Z, DC...): se comprueba solo la variable, no el registro plano de la IR
    let (_, c) = build(&src);
    let mut s = Sim::new(&c.image);
    s.run_until(10_000, |s| s.halted);
    let g = c.globals.iter().find(|g| g.name == "z").unwrap();
    assert_eq!(s.read(g.addr as usize), 1);
}
