//! f-strings en `UART.write_text` (#63, segunda parte): `f"ADC: {valor}\r\n"`, `{v:5}`, `{v:05}`, `{v:04X}`, `{v:x}`, `{v:08b}`.
//! Solo uint8 y uint16 sin signo; el nombre de una variable por campo. Los valores constantes se formatean al compilar.

use std::path::PathBuf;

use pypic_device::DevicePack;
use pypic_frontend::ast::{FKind, FSpec};
use pypic_frontend::{analyze, format_uint};
use pypic_ir::{lower, verify, Machine};
use pypic_pic14::codegen::Compiled;
use pypic_pic14::sim::Sim;
use pypic_pic14::{compile, Options};

const FOSC: u32 = 4_000_000;

fn pack() -> DevicePack {
    DevicePack::load(&PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../devices/pic16f887")).unwrap()
}

fn build(src: &str) -> (pypic_ir::Module, Compiled) {
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

fn head() -> String {
    format!("from pypic.device import PIC16F887\nfrom pypic.periph.uart import UART\nCLOCK_HZ: Const[uint32] = {FOSC}\n")
}

/// Bytes enviados (simulador == interprete) por un programa que asigna cada valor a la variable global `v` y envia `plantilla`.
fn sent(ty: &str, values: &[u16], template: &str) -> Vec<u8> {
    let mut body = String::new();
    for v in values {
        body += &format!("    v = {v}\n    show()\n");
    }
    let src = format!("{}v: {ty} = 0\n\n\ndef show() -> None:\n    UART.write_text(f\"{template}\")\n\n\ndef main() -> None:\n    global v\n    UART.start(baud=19200)\n{body}    while UART.sending():\n        pass\n", head());
    let (m, c) = build(&src);
    let mut mach = Machine::new(&m);
    mach.run_main().unwrap();
    let mut s = Sim::new(&c.image);
    s.fosc_hz = FOSC;
    s.run_until(80_000_000, |s| s.halted);
    assert!(s.halted && s.uart_violations.is_empty(), "{:?}", s.uart_violations);
    let got: Vec<u8> = s.uart_tx.iter().map(|t| t.0).collect();
    assert_eq!(got, mach.uart_tx, "simulador e interprete deben coincidir");
    got
}

fn spec(kind: FKind, width: u8, zero: bool) -> FSpec {
    FSpec { kind, width, zero }
}

fn expect(values: &[u16], bits: u8, sp: FSpec, sep: &str) -> Vec<u8> {
    values.iter().flat_map(|&v| format_uint(v, bits, sp).into_iter().chain(sep.bytes())).collect()
}

const U8: &[u16] = &[0, 1, 9, 10, 11, 99, 100, 101, 128, 199, 200, 254, 255];
const U16: &[u16] = &[0, 1, 9, 10, 99, 100, 999, 1000, 1023, 9999, 10000, 12345, 32767, 40000, 65535];

// ---- el formateador de referencia contra el de Rust (oraculo independiente) ----------------------------------------

#[test]
fn the_reference_formatter_agrees_with_rusts_own() {
    for &v in U16 {
        let (d, x, lx, b) = (spec(FKind::Dec, 0, false), spec(FKind::HexUpper, 0, true), spec(FKind::HexLower, 0, true), spec(FKind::Bin, 0, true));
        assert_eq!(format_uint(v, 16, d), format!("{v}").into_bytes());
        assert_eq!(format_uint(v, 16, spec(FKind::Dec, 7, false)), format!("{v:7}").into_bytes());
        assert_eq!(format_uint(v, 16, spec(FKind::Dec, 7, true)), format!("{v:07}").into_bytes());
        assert_eq!(format_uint(v, 16, x), format!("{v:04X}").into_bytes());
        assert_eq!(format_uint(v, 16, lx), format!("{v:04x}").into_bytes());
        assert_eq!(format_uint(v, 16, b), format!("{v:016b}").into_bytes());
        assert_eq!(format_uint(v, 16, spec(FKind::HexUpper, 6, true)), format!("{v:06X}").into_bytes());
    }
    for &v in U8 {
        assert_eq!(format_uint(v, 8, spec(FKind::HexUpper, 0, true)), format!("{v:02X}").into_bytes());
        assert_eq!(format_uint(v, 8, spec(FKind::Bin, 0, true)), format!("{v:08b}").into_bytes());
        assert_eq!(format_uint(v, 8, spec(FKind::Dec, 5, false)), format!("{v:5}").into_bytes());
    }
}

// ---- en el PIC: los mismos bytes -----------------------------------------------------------------------------------

#[test]
fn decimal_matches_for_edge_values_of_uint8_and_uint16() {
    assert_eq!(sent("uint8", U8, "{v}|"), expect(U8, 8, spec(FKind::Dec, 0, false), "|"));
    assert_eq!(sent("uint16", U16, "{v}|"), expect(U16, 16, spec(FKind::Dec, 0, false), "|"));
}

#[test]
fn decimal_width_pads_with_spaces_or_zeros_and_never_truncates() {
    assert_eq!(sent("uint8", U8, "{v:5}|"), expect(U8, 8, spec(FKind::Dec, 5, false), "|"));
    assert_eq!(sent("uint8", U8, "{v:05}|"), expect(U8, 8, spec(FKind::Dec, 5, true), "|"));
    assert_eq!(sent("uint16", U16, "{v:7}|"), expect(U16, 16, spec(FKind::Dec, 7, false), "|"));
    assert_eq!(sent("uint16", U16, "{v:05}|"), expect(U16, 16, spec(FKind::Dec, 5, true), "|"));
    // un ancho menor que el numero no lo recorta
    assert_eq!(sent("uint16", &[12345], "{v:2}|"), b"12345|");
}

#[test]
fn hexadecimal_and_binary_have_fixed_width_with_leading_zeros() {
    assert_eq!(sent("uint8", U8, "{v:X}|"), expect(U8, 8, spec(FKind::HexUpper, 0, true), "|"));
    assert_eq!(sent("uint8", U8, "{v:02x}|"), expect(U8, 8, spec(FKind::HexLower, 2, true), "|"));
    assert_eq!(sent("uint16", U16, "{v:04X}|"), expect(U16, 16, spec(FKind::HexUpper, 4, true), "|"));
    assert_eq!(sent("uint16", U16, "{v:x}|"), expect(U16, 16, spec(FKind::HexLower, 0, true), "|"));
    assert_eq!(sent("uint8", U8, "{v:08b}|"), expect(U8, 8, spec(FKind::Bin, 8, true), "|"));
    assert_eq!(sent("uint16", &[0, 1, 0xA5A5, 0xFFFF, 0x8001], "{v:016b}|"), expect(&[0, 1, 0xA5A5, 0xFFFF, 0x8001], 16, spec(FKind::Bin, 16, true), "|"));
    // mas ancho que el tipo: ceros a la izquierda
    assert_eq!(sent("uint8", &[255, 5], "{v:05X}|"), b"000FF|00005|");
}

#[test]
fn text_fields_escapes_and_literal_braces_are_exact() {
    assert_eq!(sent("uint8", &[7], "ADC={v} 0x{v:02X}\\r\\n"), b"ADC=7 0x07\r\n");
    assert_eq!(sent("uint8", &[7], "{{{v}}} \\t'\\\\'"), b"{7} \t'\\'");
    assert_eq!(sent("uint8", &[1, 2], "[{v}][{v}]"), b"[1][1][2][2]");
    // un campo solo y un texto sin campos
    assert_eq!(sent("uint16", &[300], "{v}"), b"300");
    assert_eq!(sent("uint8", &[1], "sin campos"), b"sin campos");
}

// ---- valores constantes: se formatean al compilar y no cuestan codigo ------------------------------------------------

fn words(body: &str, globals: &str) -> usize {
    build(&format!("{}{globals}def main() -> None:\n    UART.start(baud=19200)\n{body}", head())).1.flash_words
}

#[test]
fn constant_fields_are_resolved_at_compile_time() {
    let folded = words("    UART.write_text(f\"X={K} {K:04X} {K:016b}\\r\\n\")\n", "K: Const[uint16] = 1234\n");
    let plain = words("    UART.write_text(\"X=1234 04D2 0000010011010010\\r\\n\")\n", "");
    assert_eq!(folded, plain, "una f-string de constantes cuesta lo mismo que el texto escrito a mano");
    let (_, c) = build(&format!("{}K: Const[uint16] = 1234\ndef main() -> None:\n    UART.start(baud=19200)\n    UART.write_text(f\"X={{K}} {{K:04X}}\")\n    while UART.sending():\n        pass\n", head()));
    let mut s = Sim::new(&c.image);
    s.fosc_hz = FOSC;
    s.run_until(8_000_000, |s| s.halted);
    assert_eq!(s.uart_tx.iter().map(|t| t.0).collect::<Vec<_>>(), b"X=1234 04D2");
}

// ---- memoria y costo ------------------------------------------------------------------------------------------------

#[test]
fn no_text_buffer_is_reserved_and_the_formats_have_a_documented_size() {
    let base = build(&format!("{}v: uint16 = 0\ndef main() -> None:\n    UART.start(baud=19200)\n    UART.write_text(\"x\")\n", head())).1;
    let one = |ty: &str, f: &str| build(&format!("{}v: {ty} = 0\ndef main() -> None:\n    UART.start(baud=19200)\n    UART.write_text(f\"{{v{f}}}\")\n", head())).1;
    // la RAM de los campos son temporales de un byte (u8) o de dos (u16): nunca un bufer de texto
    for (ty, f) in [("uint8", ""), ("uint16", ""), ("uint8", ":02X"), ("uint16", ":04X"), ("uint16", ":016b")] {
        let c = one(ty, f);
        assert!(c.ram_bytes <= base.ram_bytes + 12, "{ty}{f}: {} frente a {}", c.ram_bytes, base.ram_bytes);
        eprintln!("{ty}{f}: +{} palabras, +{} bytes de RAM sobre un texto fijo", c.flash_words - base.flash_words, c.ram_bytes - base.ram_bytes);
    }
}

// ---- reglas ----------------------------------------------------------------------------------------------------------

#[test]
fn bad_fstrings_are_located_errors() {
    let h = head();
    let run = |ty: &str, text: &str| diagnostics(&format!("{h}v: {ty} = 0\nw: int8 = 0\nb: bool = False\ndef main() -> None:\n    UART.start(baud=9600)\n    UART.write_text(f\"{text}\")\n"));
    for (text, code, frag) in [
        ("{w}", "PYP2167", "int8"),
        ("{b}", "PYP2167", "bool"),
        ("{v+1}", "PYP1030", "nombre de una variable"),
        ("{v.x}", "PYP1030", "nombre de una variable"),
        ("{}", "PYP1030", "nombre de una variable"),
        ("{v", "PYP1030", "falta la '}'"),
        ("a}b", "PYP1030", "suelta"),
        ("{v:q}", "PYP1031", "no existe"),
        ("{v:0}", "PYP1031", "ancho"),
        ("{v:99}", "PYP1031", "ancho"),
        ("{nope}", "PYP2", "nope"),
    ] {
        let (ok, t) = run("uint16", text);
        assert!(!ok && t.contains(code) && t.contains(frag), "{text}: {t}");
    }
    // el ancho hexadecimal o binario no puede ser menor que los digitos del tipo (no se recorta en silencio)
    let (ok, t) = run("uint16", "{v:2X}");
    assert!(!ok && t.contains("PYP2168") && t.contains("4 digitos"), "{t}");
    let (ok, t) = run("uint8", "{v:4b}");
    assert!(!ok && t.contains("PYP2168") && t.contains("8 digitos"), "{t}");
    let (ok, t) = run("uint8", "{v:02X}");
    assert!(ok, "{t}");
}

#[test]
fn fstrings_are_only_accepted_by_write_text() {
    let (ok, t) = diagnostics(&format!("{}v: uint8 = 1\ndef main() -> None:\n    UART.start(baud=9600)\n    x = f\"{{v}}\"\n", head()));
    assert!(!ok && t.contains("PYP2164"), "{t}");
    let (ok, t) = diagnostics(&format!("{}v: uint8 = 1\ndef main() -> None:\n    UART.start(baud=9600, bits=9)\n    UART.write_text(f\"{{v}}\")\n", head()));
    assert!(!ok && t.contains("PYP2187"), "{t}");
    let (ok, t) = diagnostics(&format!("{}def main() -> None:\n    UART.start(baud=9600)\n    UART.write_text(f\"\")\n", head()));
    assert!(!ok && t.contains("PYP2165"), "{t}");
}
