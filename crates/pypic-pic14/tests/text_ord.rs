//! `ord('A')`: el codigo ASCII de un caracter, resuelto al compilar.

use std::path::PathBuf;

use pypic_device::DevicePack;
use pypic_frontend::analyze;
use pypic_ir::{lower, verify};
use pypic_pic14::sim::Sim;
use pypic_pic14::{compile, Options};

fn pack() -> DevicePack {
    DevicePack::load(&PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../devices/pic16f887")).unwrap()
}

const HEAD: &str = "from pypic.device import PIC16F887\nCLOCK_HZ: Const[uint32] = 4_000_000\n";

/// El programa que guarda `expr` en la global `v`.
fn program(expr: &str) -> String {
    format!("{HEAD}v: uint8 = 0\n\n\ndef main() -> None:\n    global v\n    v = {expr}\n")
}

/// Compila `v = <expr>` y devuelve el valor de `v` en el simulador.
fn run(expr: &str) -> u8 {
    let src = program(expr);
    let r = analyze(&src, &pack().target_profile());
    let p = r.program.unwrap_or_else(|| panic!("frontend:\n{}\n{src}", r.diagnostics.render(&src, "t.py")));
    let m = lower(&p);
    verify(&m).unwrap_or_else(|e| panic!("IR invalida: {e:?}"));
    let c = compile(&m, &Options::default()).unwrap();
    let mut s = Sim::new(&c.image);
    s.run_until(100_000, |s| s.halted);
    assert!(s.halted);
    let g = c.globals.iter().find(|g| g.name == "v").unwrap();
    s.read(g.addr as usize)
}

#[test]
fn ord_is_the_ascii_code() {
    let cases = [(r#"ord('A')"#, 65), (r#"ord("a")"#, 97), (r#"ord('0')"#, 48), (r#"ord(' ')"#, 32), (r#"ord('\n')"#, 10), (r#"ord('\0')"#, 0), (r#"ord('\'')"#, 39), (r#"ord('A') + 1"#, 66)];
    for (expr, value) in cases {
        assert_eq!(run(expr), value, "{expr}");
    }
}

#[test]
fn ord_rejects_anything_but_one_ascii_character() {
    for expr in ["ord('')", "ord('AB')", "ord('\u{f1}')", "ord(65)", "ord()", "ord('A', 'B')", "ord(v)"] {
        let src = program(expr);
        let r = analyze(&src, &pack().target_profile());
        let text = r.diagnostics.render(&src, "t.py");
        assert!(r.program.is_none() && text.contains("PYP2232"), "{expr}: {text}");
    }
}

#[test]
fn a_user_function_named_ord_wins() {
    let src = format!("{HEAD}v: uint8 = 0\n\n\ndef ord(x: uint8) -> uint8:\n    return x + 1\n\n\ndef main() -> None:\n    global v\n    v = ord(1)\n");
    assert!(analyze(&src, &pack().target_profile()).program.is_some());
}
