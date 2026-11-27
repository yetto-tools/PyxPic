//! `parse_dec(buf, n, error)` y `parse_hex(buf, n, error)`: texto de un arreglo a `uint16`, con el error en una variable bool.
//! El PIC, el interprete de la IR y la conversion de referencia en Rust (`pyxpic_frontend::text::parse`) deben coincidir.

use std::path::PathBuf;

use pyxpic_device::DevicePack;
use pyxpic_frontend::analyze;
use pyxpic_frontend::text::{parse, TextOp};
use pyxpic_ir::{lower, verify, Machine, Module};
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

const HEAD: &str = "from pyxpic.device import PIC16F887\nCLOCK_HZ: Const[uint32] = 4_000_000\n";

fn pic_global(c: &Compiled, s: &Sim, name: &str) -> i128 {
    let g = c.globals.iter().find(|g| g.name == name).unwrap();
    let n = g.ty.bits().max(8) as usize / 8;
    (0..n).map(|i| (s.read(g.addr as usize + i) as i128) << (8 * i)).sum()
}

/// Convierte `text` en el PIC y en el interprete; devuelve (valor, error) tras exigir que coincidan.
fn convert(func: &str, text: &[u8]) -> (u16, bool) {
    let sets: String = text.iter().enumerate().map(|(i, b)| format!("    buf[{i}] = {b}\n")).collect();
    let src = format!("{HEAD}buf: uint8[16]\nerr: bool = True\nv: uint16 = 777\n\n\ndef main() -> None:\n    global err, v\n{sets}    v = {func}(buf, {}, err)\n", text.len());
    let (m, c) = build(&src);
    let mut mach = Machine::new(&m);
    mach.run_main().unwrap();
    let mut s = Sim::new(&c.image);
    s.run_until(2_000_000, |s| s.halted);
    assert!(s.halted, "{func}({text:?})");
    let pic = (pic_global(&c, &s, "v") as u16, pic_global(&c, &s, "err") != 0);
    let reference = (mach.global("v").unwrap() as u16, mach.global("err").unwrap() != 0);
    assert_eq!(pic, reference, "{func}({:?}): PIC {pic:?} frente al interprete {reference:?}", String::from_utf8_lossy(text));
    pic
}

fn expected(op: TextOp, text: &[u8]) -> (u16, bool) {
    match parse(op, text) {
        Some(v) => (v, false),
        None => (0, true),
    }
}

#[test]
fn the_reference_parser_is_what_the_documentation_says() {
    let d = |t: &str| parse(TextOp::ParseDec, t.as_bytes());
    assert_eq!((d("0"), d("0042"), d("65535"), d("65536"), d(""), d("12a"), d(" 7"), d("-5"), d("+5")), (Some(0), Some(42), Some(65535), None, None, None, None, None, None));
    let h = |t: &str| parse(TextOp::ParseHex, t.as_bytes());
    assert_eq!(
        (h("FF"), h("ff"), h("0x1F"), h("0X7fff"), h("FFFF"), h("10000"), h("0x"), h("G"), h("00001F")),
        (Some(255), Some(255), Some(31), Some(0x7FFF), Some(0xFFFF), None, None, None, Some(31))
    );
    assert_eq!((h("0"), h("0x0"), h("x1F")), (Some(0), Some(0), None));
}

#[test]
fn decimal_matches_the_reference_for_valid_edge_and_invalid_text() {
    for t in [
        "0",
        "7",
        "10",
        "99",
        "255",
        "256",
        "6553",
        "6554",
        "65530",
        "65535",
        "65536",
        "65539",
        "99999",
        "655350",
        "0042",
        "0000000000065535",
        "",
        "12a",
        "a12",
        " 7",
        "7 ",
        "-5",
        "+5",
        "1.5",
        "1,5",
        "\u{0}",
        "\u{7f}",
    ] {
        assert_eq!(convert("parse_dec", t.as_bytes()), expected(TextOp::ParseDec, t.as_bytes()), "{t:?}");
    }
}

#[test]
fn hexadecimal_matches_the_reference_with_or_without_the_prefix_and_in_any_case() {
    for t in [
        "0", "F", "f", "FF", "ff", "Ab", "1F", "0x1F", "0X1f", "0xFFFF", "0XffFf", "FFFF", "10000", "0x10000", "00001F", "0x0001", "0x", "0X", "x1F", "G", "0xG", "1G", "1F ", "-1", "\u{0}", "0x 1",
        "fg", "9", "A", "0a", "0xa",
    ] {
        assert_eq!(convert("parse_hex", t.as_bytes()), expected(TextOp::ParseHex, t.as_bytes()), "{t:?}");
    }
}

#[test]
fn every_decimal_and_hex_value_of_a_byte_and_the_word_edges_round_trip() {
    // los valores que se convierten de numero a texto con una f-string vuelven iguales (los bordes de 8 y 16 bits)
    for v in [0u16, 1, 9, 10, 99, 100, 255, 256, 999, 1000, 9999, 10000, 32767, 32768, 65534, 65535] {
        assert_eq!(convert("parse_dec", v.to_string().as_bytes()), (v, false), "{v}");
        assert_eq!(convert("parse_hex", format!("{v:X}").as_bytes()), (v, false), "{v:X}");
        assert_eq!(convert("parse_hex", format!("0x{v:04x}").as_bytes()), (v, false), "0x{v:04x}");
    }
}

#[test]
fn the_error_flag_is_always_written_and_n_is_clamped_to_the_capacity() {
    // un error y luego un valor valido: el indicador vuelve a False
    let src = format!("{HEAD}buf: uint8[8]\nerr: bool = False\nfirst: bool = False\nv: uint16 = 0\n\n\ndef main() -> None:\n    global err, first, v\n    buf[0] = 120\n    v = parse_dec(buf, 1, err)\n    first = err\n    buf[0] = 53\n    v = parse_dec(buf, 1, err)\n");
    let (m, c) = build(&src);
    let mut mach = Machine::new(&m);
    mach.run_main().unwrap();
    let mut s = Sim::new(&c.image);
    s.run_until(1_000_000, |s| s.halted);
    for (got, want) in [("first", 1), ("err", 0), ("v", 5)] {
        assert_eq!(pic_global(&c, &s, got), want, "{got}");
        assert_eq!(mach.global(got).unwrap(), want, "{got} (interprete)");
    }
    // n = 200 se acota a 8 bytes (la capacidad): "5" seguido de siete ceros no es un numero
    let src = format!("{HEAD}buf: uint8[8]\nerr: bool = False\nn: uint8 = 200\nv: uint16 = 0\n\n\ndef main() -> None:\n    global err, v\n    buf[0] = 53\n    v = parse_dec(buf, n, err)\n");
    let (m, c) = build(&src);
    let mut mach = Machine::new(&m);
    mach.run_main().unwrap();
    let mut s = Sim::new(&c.image);
    s.run_until(1_000_000, |s| s.halted);
    assert_eq!((pic_global(&c, &s, "err"), pic_global(&c, &s, "v")), (1, 0));
    assert_eq!((mach.global("err").unwrap(), mach.global("v").unwrap()), (1, 0));
    assert!(mach.array_oob.is_empty(), "no lee fuera del arreglo: {:?}", mach.array_oob);
}

#[test]
fn the_error_variable_can_be_a_local_and_the_call_can_be_a_statement() {
    let src = format!("{HEAD}buf: uint8[4]\nout: uint8 = 0\n\n\ndef main() -> None:\n    global out\n    bad: bool = False\n    buf[0] = 55\n    buf[1] = 120\n    parse_dec(buf, 2, bad)\n    if bad:\n        out = 1\n");
    let (m, c) = build(&src);
    let mut mach = Machine::new(&m);
    mach.run_main().unwrap();
    let mut s = Sim::new(&c.image);
    s.run_until(1_000_000, |s| s.halted);
    assert_eq!((pic_global(&c, &s, "out"), mach.global("out").unwrap()), (1, 1));
}

// ---- reglas -------------------------------------------------------------------------------------------------------------------

#[test]
fn bad_calls_are_located_errors() {
    let prog = |decl: &str, body: &str| {
        diagnostics(&format!("{HEAD}buf: uint8[8]\nflag: bool = False\nm: uint16 = 0\nnum: uint8 = 0\n{decl}\n\n\ndef main() -> None:\n    global flag, m, num\n    x: uint16 = 0\n{body}"))
    };
    for (body, code, frag) in [
        ("    x = parse_dec(buf, 4)\n", "PYP2055", "parse_dec(buf, n, error)"),
        ("    x = parse_hex(buf, 4, flag, 1)\n", "PYP2055", "parse_hex(buf, n, error)"),
        ("    x = parse_dec(num, 4, flag)\n", "PYP2189", "arreglo"),
        ("    x = parse_dec(\"12\", 4, flag)\n", "PYP2189", "arreglo"),
        ("    x = parse_dec(buf, 4, num)\n", "PYP2189", "bool"),
        ("    x = parse_dec(buf, 4, 5)\n", "PYP2189", "nombre de una variable bool"),
        ("    x = parse_dec(buf, 4, undefined)\n", "PYP2022", "no esta declarada"),
        ("    x = parse_dec(buf, m, flag)\n", "PYP2", ""),
        ("    y: uint8 = parse_dec(buf, 4, flag)\n", "PYP2", ""),
    ] {
        let (ok, t) = prog("", body);
        assert!(!ok && t.contains(code) && t.contains(frag), "{body}: {t}");
    }
    // una global necesita 'global' para anotar el error, como cualquier asignacion
    let (ok, t) = diagnostics(&format!("{HEAD}buf: uint8[8]\nflag: bool = False\n\n\ndef main() -> None:\n    x: uint16 = parse_dec(buf, 4, flag)\n"));
    assert!(!ok && t.contains("PYP2023"), "{t}");
    // una funcion del usuario con ese nombre tiene prioridad
    let (ok, t) = diagnostics(&format!("{HEAD}def parse_dec(a: uint8) -> uint8:\n    return a\n\n\ndef main() -> None:\n    x: uint8 = parse_dec(3)\n"));
    assert!(ok, "{t}");
}

// ---- costo ------------------------------------------------------------------------------------------------------------------------

fn cycles(func: &str, text: &[u8]) -> u64 {
    let sets: String = text.iter().enumerate().map(|(i, b)| format!("    buf[{i}] = {b}\n")).collect();
    let src = format!("{HEAD}from pyxpic.periph.gpio import RD0\nbuf: uint8[16]\nerr: bool = False\nv: uint16 = 0\n\n\ndef main() -> None:\n    global err, v\n    RD0.output()\n{sets}    RD0.on()\n    v = {func}(buf, {}, err)\n    RD0.off()\n", text.len());
    let (_, c) = build(&src);
    let mut s = Sim::new(&c.image);
    let (mut up, mut down, mut last) = (None, None, None);
    while !s.halted && s.cycles < 1_000_000 {
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
fn the_cost_is_measured_and_grows_with_the_length_of_the_text() {
    let (d1, d5, h1, h4) = (cycles("parse_dec", b"7"), cycles("parse_dec", b"65535"), cycles("parse_hex", b"F"), cycles("parse_hex", b"FFFF"));
    // ciclos de instruccion exactos (el tiempo depende del largo del texto; unos 100 por cifra)
    assert_eq!((d1, d5, h1, h4), (135, 549, 130, 433));
}
