//! `pyxpic.periph.spi`: `SPI.start` y `SPI.transfer` contra el MSSP del simulador, la configuracion de los registros y las reglas del
//! lenguaje. Sin pruebas en Proteus ni en hardware.

use std::path::PathBuf;

use pyxpic_device::DevicePack;
use pyxpic_frontend::analyze;
use pyxpic_ir::{lower, verify};
use pyxpic_pic14::codegen::Compiled;
use pyxpic_pic14::sim::Sim;
use pyxpic_pic14::{compile, Options};

fn pack() -> DevicePack {
    DevicePack::load(&PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../devices/pic16f887")).unwrap()
}

const HEAD: &str = "from pyxpic.device import PIC16F887\nfrom pyxpic.periph.spi import SPI\n\nCLOCK: Internal[4 * MHz]\n";

fn diagnostics(src: &str) -> (bool, String) {
    let r = analyze(src, &pack().target_profile());
    (r.program.is_some(), r.diagnostics.render(src, "t.py"))
}

fn build(src: &str) -> Compiled {
    let r = analyze(src, &pack().target_profile());
    let p = r.program.unwrap_or_else(|| panic!("frontend:\n{}\n{src}", r.diagnostics.render(src, "t.py")));
    let m = lower(&p);
    verify(&m).unwrap_or_else(|e| panic!("IR invalida: {e:?}"));
    compile(&m, &Options::default()).unwrap_or_else(|e| panic!("codegen: {e}\n{src}"))
}

fn run(src: &str, miso: &[u8]) -> (Compiled, Sim) {
    let c = build(src);
    let mut s = Sim::new(&c.image);
    s.spi.miso.extend(miso);
    s.run_until(100_000, |_| false);
    (c, s)
}

fn global(c: &Compiled, s: &Sim, name: &str) -> u8 {
    let g = c.globals.iter().find(|g| g.name == name).unwrap_or_else(|| panic!("no hay la global {name}"));
    s.read(g.addr as usize)
}

fn main_with(body: &str) -> String {
    format!("{HEAD}\n\ndef main() -> None:\n{body}    while True:\n        pass\n")
}

const SSPCON: usize = 0x14;
const SSPSTAT: usize = 0x94;
const TRISC: usize = 0x87;

#[test]
fn transfer_sends_in_order_and_returns_what_arrives() {
    let src = format!(
        "{HEAD}a: uint8 = 0\nb: uint8 = 0\n\n\ndef main() -> None:\n    global a, b\n    SPI.start(freq=1000000)\n    a = SPI.transfer(0x9F)\n    SPI.transfer(0x55)\n    b = SPI.transfer(0)\n    while True:\n        pass\n"
    );
    let (c, s) = run(&src, &[0x11, 0x22, 0x33]);
    assert!(s.spi.violations.is_empty(), "{:?}", s.spi.violations);
    assert_eq!(s.spi.mosi, vec![0x9F, 0x55, 0x00]);
    // el byte cuyo resultado se ignora se lee igual: el siguiente no arrastra el anterior
    assert_eq!((global(&c, &s, "a"), global(&c, &s, "b")), (0x11, 0x33));
    assert_eq!(s.read(SSPSTAT) & 0x01, 0, "BF queda en 0");
}

#[test]
fn every_divider_exchanges_the_byte() {
    for freq in [1_000_000, 250_000, 62_500] {
        let src = format!("{HEAD}r: uint8 = 0\n\n\ndef main() -> None:\n    global r\n    SPI.start(freq={freq})\n    r = SPI.transfer(0xA5)\n    while True:\n        pass\n");
        let (c, s) = run(&src, &[0x5A]);
        assert_eq!((global(&c, &s, "r"), s.spi.mosi.clone()), (0x5A, vec![0xA5]), "{freq}");
        assert!(s.spi.violations.is_empty(), "{freq}: {:?}", s.spi.violations);
    }
}

#[test]
fn start_programs_the_mode_bits_the_divider_and_the_pin_directions() {
    // (modo, CKP, CKE): tabla 13-1 del datasheet
    for (mode, ckp, cke) in [(0, 0x00, 0x40), (1, 0x00, 0x00), (2, 0x10, 0x40), (3, 0x10, 0x00)] {
        let (_, s) = run(&main_with(&format!("    SPI.start(freq=1000000, mode={mode})\n")), &[]);
        assert_eq!((s.read(SSPCON), s.read(SSPSTAT) & 0xC0), (0x20 | ckp, cke), "modo {mode}");
        assert_eq!(s.read(TRISC) & 0x38, 0x10, "SCK y SDO salidas, SDI entrada");
    }
    // 4 MHz: FOSC/4 = 1 MHz, /16 = 250 kHz, /64 = 62,5 kHz; nunca mas rapido que lo pedido
    for (freq, sspm) in [(20_000_000, 0), (1_000_000, 0), (999_999, 1), (250_000, 1), (100_000, 2), (62_500, 2)] {
        let (_, s) = run(&main_with(&format!("    SPI.start(freq={freq})\n")), &[]);
        assert_eq!(s.read(SSPCON), 0x20 | sspm, "{freq}");
    }
}

#[test]
fn a_much_slower_clock_than_requested_warns() {
    let (ok, msg) = diagnostics(&main_with("    SPI.start(freq=900000)\n"));
    assert!(ok && msg.contains("PYP3023") && msg.contains("250000"), "{msg}");
}

#[test]
fn bad_programs_are_reported() {
    for (body, code) in [
        ("SPI.start()", "PYP2234"),
        ("SPI.start(1000000)", "PYP2234"),
        ("SPI.start(freq=1000000, mode=4)", "PYP2234"),
        ("SPI.start(velocidad=1)", "PYP2234"),
        ("SPI.start(freq=1000000)\n    SPI.read()", "PYP2235"),
        ("x: uint8 = SPI", "PYP2236"),
        ("SPI.transfer(1)", "PYP2237"),
        ("SPI.start(freq=1000)", "PYP2238"),
        ("SPI.start(freq=1000000)\n    SPI.start(freq=1000000, mode=3)", "PYP2238"),
        ("SPI.start(freq=1000000)\n    SPI.transfer()", "PYP2055"),
    ] {
        let (ok, msg) = diagnostics(&main_with(&format!("    {body}\n")));
        assert!(!ok && msg.contains(code), "{body}: {msg}");
    }
}

#[test]
fn spi_and_i2c_share_the_mssp() {
    let src = format!("from pyxpic.periph.i2c import I2C\n{}", main_with("    SPI.start(freq=1000000)\n    I2C.start(freq=100000)\n"));
    let (ok, msg) = diagnostics(&src);
    assert!(!ok && msg.contains("PYP2239"), "{msg}");
}

#[test]
fn the_import_is_checked_and_the_pins_are_taken() {
    let (ok, msg) = diagnostics("from pyxpic.device import PIC16F887\nfrom pyxpic.periph.spi import I2C\n");
    assert!(!ok && msg.contains("PYP2233"), "{msg}");
    for pin in ["RC3", "RC4", "RC5"] {
        let src = format!("from pyxpic.periph.gpio import {pin}\n{}", main_with(&format!("    {pin}.output()\n    SPI.start(freq=1000000)\n")));
        let (ok, msg) = diagnostics(&src);
        assert!(!ok && msg.contains("PYP2150"), "{pin}: {msg}");
    }
}

#[test]
fn the_loopback_example_lights_the_led_only_with_the_wire() {
    let src = std::fs::read_to_string(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/python/spi/lazo.py")).unwrap().replace("\r\n", "\n");
    let c = build(&src);
    for (wire, led) in [(true, 1), (false, 0)] {
        let mut s = Sim::new(&c.image);
        s.spi.loopback = wire;
        s.run_until(20_000, |_| false);
        assert!(s.spi.violations.is_empty(), "{:?}", s.spi.violations);
        assert_eq!(s.spi.mosi.first(), Some(&0));
        // sin cable SDI se lee 0xFF, distinto del 0 enviado
        assert_eq!(s.read(0x08) & 1, led, "cable: {wire}");
    }
}
