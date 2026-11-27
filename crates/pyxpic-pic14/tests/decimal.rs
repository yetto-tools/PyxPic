//! Literales y constantes decimales: aritmetica racional exacta al compilar, sin punto flotante, sin RAM y sin instrucciones. Un decimal con
//! valor entero (`100 * 0.5`) vale como entero donde se pide uno; con un valor de ejecucion hace falta punto fijo (error `PYP2251`).

use std::path::PathBuf;

use pyxpic_device::DevicePack;
use pyxpic_frontend::analyze;
use pyxpic_ir::{lower, verify};
use pyxpic_pic14::{compile, Options};

fn pack() -> DevicePack {
    DevicePack::load(&PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../devices/pic16f887")).unwrap()
}

fn program(globals: &str, body: &str) -> String {
    format!("from pyxpic.device import PIC16F887\nfrom pyxpic.periph.gpio import RB0\n{globals}\n\n\ndef main() -> None:\n    {body}\n")
}

/// El valor de una constante global tras analizar; `Err` con los diagnosticos si el programa se rechaza.
fn konst(globals: &str, name: &str) -> Result<Option<i128>, String> {
    let src = program(globals, "pass");
    let r = analyze(&src, &pack().target_profile());
    match r.program {
        Some(p) => Ok(p.globals.get(name).and_then(|g| g.konst)),
        None => Err(r.diagnostics.render(&src, "t.py")),
    }
}

fn words(src: &str) -> Vec<(u32, u16)> {
    let r = analyze(src, &pack().target_profile());
    let p = r.program.unwrap_or_else(|| panic!("{}", r.diagnostics.render(src, "t.py")));
    let m = lower(&p);
    verify(&m).unwrap();
    compile(&m, &Options::default()).unwrap().image.iter().collect()
}

#[test]
fn a_decimal_constant_folds_to_an_integer_at_compile_time() {
    assert_eq!(konst("FACTOR: Const = 0.5\nRESULT: Const[uint8] = 100 * FACTOR", "RESULT"), Ok(Some(50)));
    assert_eq!(konst("RESULT: Const[uint16] = 1000 * 0.001", "RESULT"), Ok(Some(1)));
    assert_eq!(konst("RESULT: Const[uint16] = 2.54 * 100", "RESULT"), Ok(Some(254)));
    assert_eq!(konst("RESULT: Const[uint32] = 11.0592 * 1000000", "RESULT"), Ok(Some(11_059_200)));
    assert_eq!(konst("RESULT: Const[uint8] = 3 // 4 * 8", "RESULT"), Ok(Some(0))); // enteros: la division entera sigue siendo '//'
    let e = konst("RESULT: Const[uint8] = 3 / 4", "RESULT").expect_err("'/' con enteros");
    assert!(e.contains("PYP1022") && e.contains("usa '//'"), "{e}");
    assert_eq!(konst("RESULT: Const[uint8] = 3.0 / 4 * 8", "RESULT"), Ok(Some(6)));
    assert_eq!(konst("RESULT: Const[int16] = -2.5 * 4", "RESULT"), Ok(Some(-10)));
    assert_eq!(konst("RESULT: Const[uint16] = 1.5e3", "RESULT"), Ok(Some(1500)));
    assert_eq!(konst("RESULT: Const[uint16] = 25e-1 * 4", "RESULT"), Ok(Some(10)));
}

#[test]
fn decimal_arithmetic_is_exact_not_floating_point() {
    // 0.1 + 0.2 es exactamente 0.3 (con float seria 0.30000000000000004)
    assert_eq!(konst("RESULT: Const[uint8] = (0.1 + 0.2) * 10", "RESULT"), Ok(Some(3)));
    assert_eq!(konst("RESULT: Const[uint8] = (0.1 + 0.2 - 0.3) * 1000000", "RESULT"), Ok(Some(0)));
    // 1/3 * 3 es exactamente 1
    assert_eq!(konst("THIRD: Const = 1.0 / 3\nRESULT: Const[uint8] = THIRD * 3", "RESULT"), Ok(Some(1)));
    // el decimal con nombre se compone con otros
    assert_eq!(konst("A: Const = 0.25\nB: Const = A * 3\nRESULT: Const[uint8] = B * 4", "RESULT"), Ok(Some(3)));
}

#[test]
fn a_constant_decimal_costs_nothing_in_the_pic() {
    let with = words(&program("FACTOR: Const = 0.5\nLIMIT: Const[uint8] = 100 * FACTOR", "RB0.output()\n    x: uint8 = LIMIT\n    if x == LIMIT:\n        RB0.on()"));
    let plain = words(&program("LIMIT: Const[uint8] = 50", "RB0.output()\n    x: uint8 = LIMIT\n    if x == LIMIT:\n        RB0.on()"));
    assert_eq!(with, plain, "el firmware es el de la constante entera");
}

#[test]
fn a_decimal_that_is_not_an_integer_never_reaches_an_integer_silently() {
    for (globals, frag) in [
        ("RESULT: Const[uint8] = 0.5", "decimal"),
        ("RESULT: Const[uint8] = 100 * 0.333", "decimal"),
        ("RESULT: Const[uint8] = 7 / 2.0", "decimal"),
    ] {
        let e = konst(globals, "RESULT").expect_err(globals);
        assert!(e.contains("PYP2041") && e.contains(frag) && e.contains("solo existe al compilar"), "{globals}: {e}");
    }
    // un valor de ejecucion con un decimal es punto fijo, no un entero
    let src = program("", "x: uint8 = 10\n    x = x * 0.5");
    let r = analyze(&src, &pack().target_profile());
    assert!(r.program.is_none());
    let msg = r.diagnostics.render(&src, "t.py");
    assert!(msg.contains("PYP2251") && msg.contains("punto fijo"), "{msg}");
    // solo + - * / entre decimales
    let e = konst("RESULT: Const[uint8] = 5 % 0.5", "RESULT").expect_err("modulo");
    assert!(e.contains("PYP2251") && e.contains("solo + - * /"), "{e}");
    let e = konst("RESULT: Const[uint8] = 1 / 0.0", "RESULT").expect_err("cero");
    assert!(e.contains("PYP2050"), "{e}");
}

#[test]
fn const_without_a_type_takes_integers_too_and_checks_its_declaration() {
    assert_eq!(konst("N: Const = 40 + 2\nRESULT: Const[uint8] = N", "RESULT"), Ok(Some(42)));
    for (globals, code, frag) in [
        ("A: Const = 0.5\nA: Const = 0.25", "PYP2009", "ya esta declarado"),
        ("A: Const\n", "PYP2007", "inicializarse"),
        ("x: uint8 = 3\nA: Const = x", "PYP2011", "expresion constante"),
    ] {
        let e = konst(globals, "A").expect_err(globals);
        assert!(e.contains(code) && e.contains(frag), "{globals}: {e}");
    }
}

#[test]
fn decimal_literals_do_not_break_units_floats_or_hex() {
    // la unidad gana a un decimal suelto; hexadecimal y enteros no cambian
    assert_eq!(konst("RESULT: Const[uint32] = 0x1e3", "RESULT"), Ok(Some(0x1e3)));
    assert_eq!(konst("RESULT: Const[uint16] = 1_000", "RESULT"), Ok(Some(1000)));
    let e = konst("RESULT: Const[uint8] = 1.5j", "RESULT").expect_err("complejo");
    assert!(e.contains("PYP0007") || e.contains("PYP0008") || e.contains("PYP0009"), "{e}");
}
