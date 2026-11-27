//! `pypic.periph.i2c` (#45): `I2C.start`, `begin`, `restart`, `end` y `write` contra el MSSP del simulador con esclavos virtuales,
//! contra el interprete de la IR, las reglas del lenguaje y la configuracion de los registros. Sin pruebas en Proteus ni en hardware.

use std::path::PathBuf;

use pypic_device::DevicePack;
use pypic_frontend::analyze;
use pypic_ir::{lower, verify};
use pypic_pic14::codegen::Compiled;
use pypic_pic14::i2c_sim::I2cEvent;
use pypic_pic14::sim::Sim;
use pypic_pic14::{compile, Options};

fn pack() -> DevicePack {
    DevicePack::load(&PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../devices/pic16f887")).unwrap()
}

fn head() -> &'static str {
    "from pypic.device import PIC16F887\nfrom pypic.periph.i2c import I2C\n\nCLOCK: Internal[4 * MHz]\n"
}

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

fn run(src: &str, slaves: &[u8]) -> (Compiled, Sim) {
    let c = build(src);
    let mut s = Sim::new(&c.image);
    s.i2c.slaves.extend_from_slice(slaves);
    s.run_until(200_000, |_| false);
    (c, s)
}

fn global(c: &Compiled, s: &Sim, name: &str) -> u8 {
    let g = c.globals.iter().find(|g| g.name == name).unwrap_or_else(|| panic!("no hay la global {name}"));
    s.read(g.addr as usize)
}

const SSPADD: usize = 0x93;
const SSPCON: usize = 0x14;
const SSPSTAT: usize = 0x94;

// ---- el bus: eventos y ACK -----------------------------------------------------------------------------------------------

#[test]
fn a_transaction_is_start_address_data_stop_and_the_ack_reaches_the_program() {
    let src = format!(
        "{}acuse: uint8 = 0\nacuse2: uint8 = 0\n\n\ndef main() -> None:\n    global acuse, acuse2\n    I2C.start(freq=100000)\n    I2C.begin()\n    if I2C.write(0x4E):\n        acuse = 1\n    if I2C.write(0xA5):\n        acuse2 = 1\n    I2C.end()\n    while True:\n        pass\n",
        head()
    );
    let (c, s) = run(&src, &[0x27]);
    assert!(s.i2c.violations.is_empty(), "{:?}", s.i2c.violations);
    assert_eq!(s.i2c.events, vec![I2cEvent::Start, I2cEvent::Address { addr: 0x27, read: false, ack: true }, I2cEvent::Data { byte: 0xA5, ack: true }, I2cEvent::Stop]);
    assert_eq!((global(&c, &s, "acuse"), global(&c, &s, "acuse2")), (1, 1));
}

#[test]
fn an_absent_address_gets_no_ack() {
    let src = format!("{}acuse: uint8 = 7\n\n\ndef main() -> None:\n    global acuse\n    I2C.start(freq=100000)\n    I2C.begin()\n    if I2C.write(0x50):\n        acuse = 1\n    else:\n        acuse = 0\n    I2C.end()\n    while True:\n        pass\n", head());
    let (c, s) = run(&src, &[0x27]);
    assert_eq!(global(&c, &s, "acuse"), 0);
    assert_eq!(s.i2c.events[1], I2cEvent::Address { addr: 0x28, read: false, ack: false });
}

#[test]
fn restart_repeats_the_start_condition() {
    let src = format!(
        "{}\n\ndef main() -> None:\n    I2C.start(freq=100000)\n    I2C.begin()\n    I2C.write(0x4E)\n    I2C.restart()\n    I2C.write(0x4E)\n    I2C.end()\n    while True:\n        pass\n",
        head()
    );
    let (_, s) = run(&src, &[0x27]);
    assert!(s.i2c.violations.is_empty(), "{:?}", s.i2c.violations);
    assert_eq!(s.i2c.events.iter().filter(|e| **e == I2cEvent::RepeatedStart).count(), 1);
    assert_eq!(s.i2c.events.last(), Some(&I2cEvent::Stop));
}

#[test]
fn a_write_without_using_the_result_still_sends_the_byte() {
    let src = format!(
        "{}\n\ndef main() -> None:\n    I2C.start(freq=100000)\n    I2C.begin()\n    I2C.write(0x4E)\n    I2C.write(0x11)\n    I2C.write(0x22)\n    I2C.end()\n    while True:\n        pass\n",
        head()
    );
    let (_, s) = run(&src, &[0x27]);
    assert_eq!(s.i2c.received_by(0x27), vec![0x11, 0x22]);
}

// ---- la configuracion de los registros ---------------------------------------------------------------------------------------

#[test]
fn start_programs_sspadd_sspcon_and_the_slew_control_from_the_clock() {
    let prog = |freq: u32| format!("{}\n\ndef main() -> None:\n    I2C.start(freq={freq})\n    while True:\n        pass\n", head());
    // 4 MHz / (4 * (SSPADD + 1)): 100 kHz -> 9; 400 kHz -> 2, que da 333 kHz (SMP = 0, control de pendiente activado)
    let (_, s) = run(&prog(100_000), &[]);
    assert_eq!((s.read(SSPADD), s.read(SSPCON), s.read(SSPSTAT) & 0x80), (9, 0x28, 0x80));
    let (_, s) = run(&prog(400_000), &[]);
    assert_eq!((s.read(SSPADD), s.read(SSPCON), s.read(SSPSTAT) & 0x80), (2, 0x28, 0));
    // pide 80 kHz: SSPADD = 11 da 83,3 kHz (> 80), asi que se baja a SSPADD = 12 (76,9 kHz): nunca mas rapido que lo pedido
    let (_, s) = run(&prog(80_000), &[]);
    assert_eq!(s.read(SSPADD), 12);
}

// ---- reglas del lenguaje -----------------------------------------------------------------------------------------------------

#[test]
fn using_the_bus_without_start_is_an_error() {
    let (ok, msg) = diagnostics(&format!("{}\n\ndef main() -> None:\n    I2C.begin()\n    I2C.end()\n", head()));
    assert!(!ok && msg.contains("PYP2225"), "{msg}");
}

#[test]
fn two_speeds_are_an_error() {
    let (ok, msg) = diagnostics(&format!("{}\n\ndef main() -> None:\n    I2C.start(freq=100000)\n    I2C.start(freq=400000)\n    I2C.begin()\n", head()));
    assert!(!ok && msg.contains("PYP2228"), "{msg}");
}

#[test]
fn a_speed_the_clock_cannot_make_is_an_error() {
    // 4 MHz / (4 * 128) = 7,8 kHz es lo mas lento; 1 MHz necesita SSPADD = 0
    let (ok, msg) = diagnostics(&format!("{}\n\ndef main() -> None:\n    I2C.start(freq=5000)\n    I2C.begin()\n", head()));
    assert!(!ok && msg.contains("PYP2228") && msg.contains("7812"), "{msg}");
    let (ok, msg) = diagnostics(&format!("{}\n\ndef main() -> None:\n    I2C.start(freq=1000000)\n    I2C.begin()\n", head()));
    assert!(!ok && msg.contains("PYP2228"), "{msg}");
}

#[test]
fn bad_calls_are_reported() {
    for (body, code) in [
        ("I2C.start()", "PYP2226"),
        ("I2C.start(100000)", "PYP2226"),
        ("I2C.start(velocidad=100000)", "PYP2226"),
        ("I2C.start(freq=100000)\n    I2C.recv()", "PYP2227"),
        ("I2C.start(freq=100000)\n    I2C.write()", "PYP2055"),
        ("I2C.start(freq=100000)\n    I2C.begin(1)", "PYP2055"),
        ("I2C.start(freq=100000)\n    x: uint16 = 300\n    I2C.write(x)", ""),
    ] {
        let (ok, msg) = diagnostics(&format!("{}\n\ndef main() -> None:\n    {body}\n", head()));
        assert!(!ok, "{body}: se esperaba un error");
        if !code.is_empty() {
            assert!(msg.contains(code), "{body}: {msg}");
        }
    }
}

#[test]
fn the_module_needs_the_mssp_and_its_pins_are_taken() {
    let (ok, msg) = diagnostics("from pypic.device import PIC16F887\nfrom pypic.periph.i2c import SPI\n");
    assert!(!ok && msg.contains("PYP2224"), "{msg}");
    let (ok, msg) = diagnostics(&format!("{}from pypic.periph.gpio import RC3\n\ndef main() -> None:\n    RC3.output()\n    I2C.start(freq=100000)\n", head()));
    assert!(!ok && msg.contains("PYP2150"), "{msg}");
    // RC5 (SDO) no es del I2C: sigue libre
    let (ok, msg) = diagnostics(&format!("{}from pypic.periph.gpio import RC5\n\ndef main() -> None:\n    RC5.output()\n    I2C.start(freq=100000)\n", head()));
    assert!(ok, "{msg}");
}

// ---- el escaner de direcciones ---------------------------------------------------------------------------------------------

#[test]
fn the_scanner_example_reports_the_addresses_that_answer() {
    let src = std::fs::read_to_string(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/python/i2c/escaner.py")).unwrap().replace("\r\n", "\n");
    let c = build(&src);
    let mut s = Sim::new(&c.image);
    s.i2c.slaves.extend([0x27, 0x50]);
    s.run_until(300_000, |_| false);
    assert!(s.i2c.violations.is_empty(), "{:?}", s.i2c.violations);
    let text: String = s.uart_tx.iter().map(|(b, _)| *b as char).collect();
    assert!(text.starts_with("0x27\r\n0x50\r\n2 dispositivo(s)\r\n"), "{text:?}");
    // probo de la 0x01 a la 0x7F: 127 transacciones por vuelta, cada una START, direccion y STOP
    let starts = s.i2c.events.iter().filter(|e| **e == I2cEvent::Start).count();
    assert!(starts >= 127, "{starts}");
}

// ---- lectura -----------------------------------------------------------------------------------------------------------------

#[test]
fn read_returns_the_bytes_and_answers_ack_until_the_last() {
    let src = format!(
        "{}a: uint8 = 0\nb: uint8 = 0\n\n\ndef main() -> None:\n    global a, b\n    I2C.start(freq=100000)\n    I2C.begin()\n    I2C.write(0xA1)\n    a = I2C.read(ack=True)\n    b = I2C.read(ack=False)\n    I2C.end()\n    while True:\n        pass\n",
        head()
    );
    let c = build(&src);
    let mut s = Sim::new(&c.image);
    s.i2c.slaves.push(0x50);
    s.i2c.read_data.extend([0x12, 0x34]);
    s.run_until(200_000, |_| false);
    assert!(s.i2c.violations.is_empty(), "{:?}", s.i2c.violations);
    assert_eq!((global(&c, &s, "a"), global(&c, &s, "b")), (0x12, 0x34));
    assert_eq!(
        s.i2c.events,
        vec![I2cEvent::Start, I2cEvent::Address { addr: 0x50, read: true, ack: true }, I2cEvent::Received { byte: 0x12, ack: true }, I2cEvent::Received { byte: 0x34, ack: false }, I2cEvent::Stop]
    );
}

#[test]
fn the_ack_of_a_read_can_be_computed_at_run_time() {
    // tres bytes en un bucle: ACK en los dos primeros y NACK en el ultimo; el resultado de un byte se descarta
    let src = format!(
        "{}suma: uint8 = 0\n\n\ndef main() -> None:\n    global suma\n    I2C.start(freq=100000)\n    I2C.begin()\n    I2C.write(0xA1)\n    for i in range(3):\n        suma = suma + I2C.read(ack=i < 2)\n    I2C.restart()\n    I2C.write(0xA1)\n    I2C.read(ack=False)\n    I2C.end()\n    while True:\n        pass\n",
        head()
    );
    let c = build(&src);
    let mut s = Sim::new(&c.image);
    s.i2c.slaves.push(0x50);
    s.i2c.read_data.extend([1, 2, 4, 8]);
    s.run_until(200_000, |_| false);
    assert!(s.i2c.violations.is_empty(), "{:?}", s.i2c.violations);
    assert_eq!(global(&c, &s, "suma"), 7);
    let acks: Vec<bool> = s.i2c.events.iter().filter_map(|e| if let I2cEvent::Received { ack, .. } = e { Some(*ack) } else { None }).collect();
    assert_eq!(acks, vec![true, true, false, false]);
}

#[test]
fn ending_a_read_without_nack_is_flagged_by_the_simulator() {
    let src = format!("{}\n\ndef main() -> None:\n    I2C.start(freq=100000)\n    I2C.begin()\n    I2C.write(0xA1)\n    I2C.read(ack=True)\n    I2C.end()\n    while True:\n        pass\n", head());
    let (_, s) = run(&src, &[0x50]);
    assert!(s.i2c.violations.iter().any(|v| v.contains("NACK")), "{:?}", s.i2c.violations);
}

#[test]
fn read_needs_its_ack_argument() {
    for body in ["I2C.read()", "I2C.read(True)", "I2C.read(nack=True)", "I2C.read(ack=1)"] {
        let (ok, msg) = diagnostics(&format!("{}\n\ndef main() -> None:\n    I2C.start(freq=100000)\n    {body}\n", head()));
        assert!(!ok && (msg.contains("PYP2226") || msg.contains("PYP2041")), "{body}: {msg}");
    }
}

#[test]
fn the_pcf8574_example_shows_the_inputs_on_portd() {
    let src = std::fs::read_to_string(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/python/i2c/leer_pcf8574.py")).unwrap().replace("\r\n", "\n");
    let c = build(&src);
    let mut s = Sim::new(&c.image);
    s.i2c.slaves.push(0x20);
    s.i2c.read_data.push_back(0xB6);
    s.run_until(5_000, |_| false);
    assert!(s.i2c.violations.is_empty(), "{:?}", s.i2c.violations);
    assert_eq!(s.i2c.received_by(0x20), vec![0xFF]);
    assert_eq!(s.read(0x08), 0xB6);
}
