//! `UART.write_text` (#63, primera parte): texto ASCII constante en la memoria de programa, sin bufer en RAM.
//! Sin f-strings, sin conversion de numeros y sin recepcion de texto todavia.

use std::path::PathBuf;

use pypic_device::DevicePack;
use pypic_frontend::analyze;
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

/// Compila `main`, la corre en el simulador y en el interprete, y devuelve los bytes que enviaron (iguales en los dos).
fn sent(body: &str, extra: &str) -> Vec<u8> {
    let src = format!("{}{extra}def main() -> None:\n    UART.start(baud=9600)\n{body}    while UART.sending():\n        pass\n", head());
    let (m, c) = build(&src);
    let mut mach = Machine::new(&m);
    mach.run_main().unwrap();
    let mut s = Sim::new(&c.image);
    s.fosc_hz = FOSC;
    s.run_until(8_000_000, |s| s.halted);
    assert!(s.halted && s.uart_violations.is_empty(), "{:?}", s.uart_violations);
    let got: Vec<u8> = s.uart_tx.iter().map(|t| t.0).collect();
    assert_eq!(got, mach.uart_tx, "simulador e interprete deben coincidir");
    got
}

#[test]
fn a_literal_goes_out_byte_for_byte_including_crlf() {
    assert_eq!(sent("    UART.write_text(\"Hola\\r\\n\")\n", ""), b"Hola\r\n");
}

#[test]
fn escapes_and_edge_bytes_are_exact() {
    assert_eq!(sent("    UART.write_text(\"a\\tb\\\\c\\\"d\\'e\\0f\")\n", ""), vec![b'a', 9, b'b', b'\\', b'c', b'"', b'd', b'\'', b'e', 0, b'f']);
    // el mayor ASCII y el menor imprimible
    assert_eq!(sent("    UART.write_text(\"~ \")\n", ""), vec![126, 32]);
    // comillas simples
    assert_eq!(sent("    UART.write_text('OK')\n", ""), b"OK");
}

#[test]
fn several_texts_and_a_single_byte_write_keep_their_order() {
    assert_eq!(sent("    UART.write_text(\"AB\")\n    UART.write(67)\n    UART.write_text(\"DE\")\n", ""), b"ABCDE");
}

#[test]
fn a_rom_table_can_be_sent_by_name() {
    assert_eq!(sent("    UART.write_text(SALUDO)\n", "SALUDO: Rom[uint8] = [72, 105, 13, 10]\n"), b"Hi\r\n");
}

#[test]
fn the_longest_text_is_255_bytes_and_goes_out_whole() {
    let t = "x".repeat(255);
    let got = sent(&format!("    UART.write_text(\"{t}\")\n"), "");
    assert_eq!(got.len(), 255);
    assert!(got.iter().all(|&b| b == b'x'));
}

// ---- memoria: flash si, RAM no -----------------------------------------------------------------------------------

fn compiled(body: &str) -> Compiled {
    build(&format!("{}def main() -> None:\n    UART.start(baud=9600)\n{body}", head())).1
}

#[test]
fn the_text_lives_in_flash_and_no_ram_buffer_is_reserved() {
    let short = compiled("    UART.write_text(\"A\")\n");
    let long = compiled(&format!("    UART.write_text(\"{}\")\n", "A".repeat(200)));
    assert_eq!(short.ram_bytes, long.ram_bytes, "la RAM no depende de la longitud del texto");
    assert!(long.flash_words >= short.flash_words + 199, "los bytes van a la memoria de programa: {} frente a {}", long.flash_words, short.flash_words);
    // solo tres temporales de un byte: el indice, el byte actual y el resultado de la comparacion del bucle
    let none = compiled("    UART.write(65)\n");
    assert_eq!(short.ram_bytes, none.ram_bytes + 3, "{} frente a {}", short.ram_bytes, none.ram_bytes);
}

#[test]
fn equal_literals_share_their_bytes_in_flash() {
    let same = compiled("    UART.write_text(\"ABCDEFGHIJ\")\n    UART.write_text(\"ABCDEFGHIJ\")\n");
    let different = compiled("    UART.write_text(\"ABCDEFGHIJ\")\n    UART.write_text(\"KLMNOPQRST\")\n");
    // el segundo texto distinto agrega sus 10 bytes (un RETLW cada uno) y, a lo mas, relleno de alineacion de la tabla
    let extra = different.flash_words - same.flash_words;
    assert!((10..=12).contains(&extra), "{} frente a {}", different.flash_words, same.flash_words);
}

// ---- costo --------------------------------------------------------------------------------------------------------

fn cycles_of(text_len: usize) -> u64 {
    let src = format!(
        "{}from pypic.periph.gpio import RD0\ndef main() -> None:\n    RD0.output()\n    UART.start(baud=9600)\n    RD0.on()\n    UART.write_text(\"{}\")\n    RD0.off()\n",
        head(),
        "A".repeat(text_len)
    );
    let (_, c) = build(&src);
    let mut s = Sim::new(&c.image);
    s.fosc_hz = FOSC;
    let (mut up, mut down, mut last) = (None, None, None);
    while !s.halted && s.cycles < 400_000 {
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
fn the_cost_per_byte_is_constant_apart_from_waiting_for_the_transmitter() {
    // los dos primeros bytes entran de inmediato (uno al registro de desplazamiento y otro a TXREG); desde el tercero cada byte espera
    // a que salga una trama (1040 ciclos a 9600 baudios y 4 MHz), asi que el costo por byte pasa a ser el de la trama
    let c: Vec<u64> = (1..=6).map(cycles_of).collect();
    assert!(c[1] - c[0] < 60, "el segundo byte no espera: {c:?}");
    let (d1, d2, d3) = (c[3] - c[2], c[4] - c[3], c[5] - c[4]);
    assert!([d1, d2, d3].iter().all(|d| (1030..=1050).contains(d)), "desde el tercero un byte cuesta una trama: {c:?}");
}

// ---- reglas -------------------------------------------------------------------------------------------------------

#[test]
fn bad_text_is_a_located_error() {
    let h = head();
    let run = |arg: &str| diagnostics(&format!("{h}def main() -> None:\n    UART.start(baud=9600)\n    UART.write_text({arg})\n"));
    for (arg, code, frag) in [
        ("\"caf\u{e9}\"", "PYP2165", "no es ASCII"),
        ("\"a\\qb\"", "PYP2165", "no existe"),
        ("\"\"", "PYP2165", "vacio"),
        ("65", "PYP2166", "texto entre comillas"),
        ("main", "PYP2166", "texto entre comillas"),
    ] {
        let (ok, t) = run(arg);
        assert!(!ok && t.contains(code) && t.contains(frag), "{arg}: {t}");
    }
    let long = format!("\"{}\"", "x".repeat(256));
    let (ok, t) = run(&long);
    assert!(!ok && t.contains("PYP2165") && t.contains("255"), "{t}");
    // una variable no es texto constante: no se copia a RAM en silencio
    let (ok, t) = diagnostics(&format!("{h}n: uint8 = 3\ndef main() -> None:\n    UART.start(baud=9600)\n    UART.write_text(n)\n"));
    assert!(!ok && t.contains("PYP2166"), "{t}");
    let (ok, t) = diagnostics(&format!("{h}def main() -> None:\n    UART.start(baud=9600)\n    UART.write_text(\"a\", \"b\")\n"));
    assert!(!ok && t.contains("PYP2055"), "{t}");
    let (ok, t) = diagnostics(&format!("{h}def main() -> None:\n    UART.start(baud=9600)\n    UART.write_text()\n"));
    assert!(!ok && t.contains("PYP2055"), "{t}");
}

#[test]
fn text_is_only_accepted_by_write_text() {
    let (ok, t) = diagnostics(&format!("{}def main() -> None:\n    UART.start(baud=9600)\n    x = \"hola\"\n", head()));
    assert!(!ok && t.contains("PYP2164"), "{t}");
    let (ok, t) = diagnostics(&format!("{}def main() -> None:\n    UART.start(baud=9600)\n    UART.write(\"A\")\n", head()));
    assert!(!ok && t.contains("PYP2164"), "{t}");
}

#[test]
fn write_text_counts_as_using_the_uart_and_is_an_eight_bit_operation() {
    let h = head();
    let (ok, t) = diagnostics(&format!("{h}def main() -> None:\n    UART.write_text(\"hi\")\n"));
    assert!(!ok && t.contains("PYP2181"), "{t}");
    let (ok, t) = diagnostics(&format!("{h}def main() -> None:\n    UART.start(baud=9600, bits=9)\n    UART.write_text(\"hi\")\n"));
    assert!(!ok && t.contains("PYP2187"), "{t}");
}
