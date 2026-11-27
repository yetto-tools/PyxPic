//! `UART.read_line(buf)` y `UART.write_buf(buf, n)` (#63, tercera parte): recepcion de una linea en un arreglo de RAM y envio de un
//! arreglo. Sin conversion de texto a numero todavia.

use std::path::PathBuf;

use pyxpic_device::DevicePack;
use pyxpic_frontend::analyze;
use pyxpic_ir::{lower, verify, Machine, Module};
use pyxpic_pic14::codegen::Compiled;
use pyxpic_pic14::sim::Sim;
use pyxpic_pic14::{compile, Options};

const FOSC: u32 = 4_000_000;
/// Una trama de 10 bits a 9600 baudios y 4 MHz (SPBRG = 25, BRGH = 1).
const FRAME: u64 = 1040;

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

fn head() -> String {
    format!("from pyxpic.device import PIC16F887\nfrom pyxpic.periph.uart import UART\nCLOCK_HZ: Const[uint32] = {FOSC}\n")
}

fn pic_global(c: &Compiled, s: &Sim, name: &str) -> i128 {
    let g = c.globals.iter().find(|g| g.name == name).unwrap();
    s.read(g.addr as usize) as i128
}

fn pic_array(c: &Compiled, s: &Sim, name: &str) -> Vec<u8> {
    let a = c.arrays.iter().find(|a| a.name == name).unwrap();
    (0..a.len).map(|k| s.read(a.addr as usize + k)).collect()
}

/// Corre el programa con `input` en el interprete y en el simulador (un byte por trama y un margen); exige los mismos globales,
/// arreglos y bytes enviados.
fn run(src: &str, input: &[u8]) -> (Machine<'static>, Compiled, Sim) {
    let (m, c) = build(&format!("{}{src}", head()));
    let m: &'static Module = Box::leak(Box::new(m));
    let mut mach = Machine::new(m);
    mach.uart_rx.extend(input.iter().copied());
    mach.run_main().unwrap();
    let mut s = Sim::new(&c.image);
    s.fosc_hz = FOSC;
    for (i, b) in input.iter().enumerate() {
        s.uart_feed(*b, 3_000 + i as u64 * (FRAME + 400), true);
    }
    s.run_until(80_000_000, |s| s.halted);
    assert!(s.halted && s.uart_violations.is_empty(), "{:?}", s.uart_violations);
    for g in m.globals.iter().filter(|g| !g.is_const) {
        assert_eq!(pic_global(&c, &s, &g.name), mach.global(&g.name).unwrap(), "global {}", g.name);
    }
    for (i, (n, _)) in m.arrays.iter().enumerate() {
        assert_eq!(pic_array(&c, &s, n), mach.arrays[i], "arreglo {n}");
    }
    let sent: Vec<u8> = s.uart_tx.iter().map(|t| t.0).collect();
    assert_eq!(sent, mach.uart_tx, "bytes enviados: simulador e interprete");
    (mach, c, s)
}

const ONE_LINE: &str = "buf: uint8[8]\nn: uint8 = 0\n\n\ndef main() -> None:\n    global n\n    UART.start(baud=9600)\n    n = UART.read_line(buf)\n";

#[test]
fn a_line_is_stored_without_its_terminator_and_returns_its_length() {
    let (_, c, s) = run(ONE_LINE, b"hola\n");
    assert_eq!(pic_global(&c, &s, "n"), 4);
    assert_eq!(pic_array(&c, &s, "buf")[..4], *b"hola");
}

#[test]
fn crlf_and_stray_carriage_returns_are_ignored() {
    let (_, c, s) = run(ONE_LINE, b"ab\r\n");
    assert_eq!((pic_global(&c, &s, "n"), pic_array(&c, &s, "buf")[..2].to_vec()), (2, b"ab".to_vec()));
    let (_, c, s) = run(ONE_LINE, b"a\rb\rc\n");
    assert_eq!((pic_global(&c, &s, "n"), pic_array(&c, &s, "buf")[..3].to_vec()), (3, b"abc".to_vec()));
}

#[test]
fn an_empty_line_returns_zero_and_leaves_the_buffer_alone() {
    let (_, c, s) = run(ONE_LINE, b"\n");
    assert_eq!(pic_global(&c, &s, "n"), 0);
    assert_eq!(pic_array(&c, &s, "buf"), vec![0; 8]);
    let (_, c, s) = run(ONE_LINE, b"\r\n");
    assert_eq!(pic_global(&c, &s, "n"), 0);
}

#[test]
fn a_line_longer_than_the_buffer_is_truncated_and_the_excess_is_discarded() {
    // 8 de capacidad: se guardan 8, se descarta el resto hasta el \n y la linea siguiente empieza limpia
    let two = "buf: uint8[8]\nn1: uint8 = 0\nn2: uint8 = 0\nfirst: uint8 = 0\n\n\ndef main() -> None:\n    global n1, n2, first\n    UART.start(baud=9600)\n    n1 = UART.read_line(buf)\n    n2 = UART.read_line(buf)\n    first = buf[0]\n";
    let (mach, c, s) = run(two, b"ABCDEFGHIJKLMNOP\nxyz\n");
    assert_eq!(pic_global(&c, &s, "n1"), 8, "se guardan 8, no 16");
    assert_eq!(pic_global(&c, &s, "n2"), 3, "la segunda linea no arrastra el exceso de la primera");
    assert_eq!(pic_global(&c, &s, "first"), b'x' as i128);
    assert_eq!(pic_array(&c, &s, "buf")[..3], *b"xyz");
    assert!(mach.array_oob.is_empty(), "nunca se escribe fuera de rango: {:?}", mach.array_oob);
    // una linea de exactamente la capacidad no se distingue de una truncada: se usa un buffer de N + 1 para detectarlo
    let (_, c, s) = run(ONE_LINE, b"12345678\n");
    assert_eq!(pic_global(&c, &s, "n"), 8);
}

#[test]
fn write_buf_sends_the_first_n_bytes_and_clamps_n_to_the_capacity() {
    let echo = "buf: uint8[6]\nn: uint8 = 0\nhow_many: uint8 = 40\n\n\ndef main() -> None:\n    global n\n    UART.start(baud=9600)\n    n = UART.read_line(buf)\n    UART.write_buf(buf, n)\n    UART.write(33)\n    UART.write_buf(buf, 2)\n    UART.write(33)\n    UART.write_buf(buf, how_many)\n    while UART.sending():\n        pass\n";
    let (_, _, s) = run(echo, b"hola\n");
    let sent: Vec<u8> = s.uart_tx.iter().map(|t| t.0).collect();
    // "hola", "!", "ho", "!", y 40 pedidos se recortan a los 6 de la capacidad: "hola" + dos ceros
    assert_eq!(sent, [b"hola!".as_slice(), b"ho!".as_slice(), b"hola\0\0".as_slice()].concat());
    // n = 0 no envia nada
    let (_, _, s) = run("buf: uint8[4]\n\n\ndef main() -> None:\n    UART.start(baud=9600)\n    UART.write_buf(buf, 0)\n", b"");
    assert!(s.uart_tx.is_empty());
}

#[test]
fn a_command_loop_parses_the_first_byte_of_each_line() {
    // lee lineas y contesta 'A' si empiezan con 'L', 'B' si no; una linea vacia no contesta
    let cmd = "buf: uint8[8]\n\n\ndef main() -> None:\n    UART.start(baud=9600)\n    for k in range(4):\n        n: uint8 = UART.read_line(buf)\n        if n > 0:\n            if buf[0] == 76:\n                UART.write(65)\n            else:\n                UART.write(66)\n    while UART.sending():\n        pass\n";
    let (_, _, s) = run(cmd, b"LED ON\nstop\n\nL\n");
    let sent: Vec<u8> = s.uart_tx.iter().map(|t| t.0).collect();
    assert_eq!(sent, b"ABA");
}

// ---- reglas -------------------------------------------------------------------------------------------------------

#[test]
fn the_buffer_must_be_a_named_uint8_array() {
    let h = head();
    let run = |decl: &str, body: &str| diagnostics(&format!("{h}{decl}\n\n\ndef main() -> None:\n    UART.start(baud=9600)\n{body}"));
    for (decl, body, code, frag) in [
        ("n: uint8 = 0", "    UART.read_line(n)\n", "PYP2188", "arreglo"),
        ("", "    UART.read_line(5)\n", "PYP2188", "arreglo"),
        ("T: Rom[uint8] = [1, 2]", "    UART.write_buf(T, 2)\n", "PYP2188", "arreglo"),
        ("buf: uint8[4]", "    UART.read_line()\n", "PYP2055", "read_line(buf)"),
        ("buf: uint8[4]", "    UART.read_line(buf, 3)\n", "PYP2055", "read_line(buf)"),
        ("buf: uint8[4]", "    UART.write_buf(buf)\n", "PYP2055", "write_buf(buf, n)"),
        ("buf: uint8[4]\nm: uint16 = 3", "    UART.write_buf(buf, m)\n", "PYP2", ""),
    ] {
        let (ok, t) = run(decl, body);
        assert!(!ok && t.contains(code) && t.contains(frag), "{decl} / {body}: {t}");
    }
    let (ok, t) = diagnostics(&format!("{h}buf: uint8[4]\n\n\ndef main() -> None:\n    UART.read_line(buf)\n"));
    assert!(!ok && t.contains("PYP2181"), "{t}");
    let (ok, t) = diagnostics(&format!("{h}buf: uint8[4]\n\n\ndef main() -> None:\n    UART.start(baud=9600, bits=9)\n    UART.read_line(buf)\n"));
    assert!(!ok && t.contains("PYP2187"), "{t}");
    let (ok, t) = diagnostics(&format!("{h}buf: uint8[4]\n\n\ndef main() -> None:\n    UART.start(baud=9600)\n    n: uint8 = UART.read_line(buf)\n    UART.write_buf(buf, n)\n"));
    assert!(ok, "{t}");
}

#[test]
fn an_overrun_while_waiting_is_cleared_and_the_line_in_progress_restarts_instead_of_hanging() {
    // el programa esta ocupado transmitiendo cuando llegan 3 bytes: el FIFO guarda 2 y el tercero (el \n) desborda; read_line debe
    // borrar OERR y seguir esperando, no quedarse mudo. La linea siguiente se recibe entera.
    let src = "buf: uint8[8]\nn: uint8 = 0\n\n\ndef main() -> None:\n    global n\n    UART.start(baud=9600)\n    UART.write_text(\"xxxxxxxxxxxx\")\n    n = UART.read_line(buf)\n";
    let (_, c) = build(&format!("{}{src}", head()));
    let mut s = Sim::new(&c.image);
    s.fosc_hz = FOSC;
    for (i, b) in b"AB\n".iter().enumerate() {
        s.uart_feed(*b, 4_000 + i as u64 * 200, true); // mientras sale el texto de 12 bytes (~12 000 ciclos)
    }
    for (i, b) in b"cd\n".iter().enumerate() {
        s.uart_feed(*b, 40_000 + i as u64 * 2_000, true); // despues de terminar de transmitir
    }
    s.run_until(500_000, |s| s.halted);
    assert!(s.halted, "read_line no se queda colgada tras un desbordamiento");
    assert_eq!(pic_global(&c, &s, "n"), 2);
    assert_eq!(pic_array(&c, &s, "buf")[..2], *b"cd");
}
