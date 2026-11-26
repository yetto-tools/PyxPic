//! UART de 9 bits (`UART.start(baud=..., bits=9)`, `write9`/`send9`/`read9`/`recv9`, #45): TX9D/RX9D, tramas de 11 bits y las reglas
//! que impiden mezclar 8 y 9 bits.

use std::path::PathBuf;

use pypic_device::DevicePack;
use pypic_frontend::analyze;
use pypic_ir::{lower, verify, Machine};
use pypic_pic14::codegen::Compiled;
use pypic_pic14::sim::Sim;
use pypic_pic14::{compile, Options};

const FOSC: u32 = 4_000_000;
/// SPBRG = 25 con BRGH = 1 a 4 MHz: 16 * 26 = 416 ciclos... por cuatro: 104 ciclos por bit.
const BIT: u64 = 104;

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

fn sim_for(c: &Compiled) -> Sim {
    let mut s = Sim::new(&c.image);
    s.fosc_hz = FOSC;
    s
}

/// Los caracteres de 9 bits que salieron: byte | noveno bit << 8.
fn sent9(s: &Sim) -> Vec<u16> {
    s.uart_tx.iter().zip(&s.uart_tx9).map(|(t, b9)| t.0 as u16 | (*b9 as u16) << 8).collect()
}

#[test]
fn start_sets_tx9_and_rx9_and_the_frame_has_eleven_bits() {
    let (_, c) = build(&format!("{}def main() -> None:\n    UART.start(baud=9600, bits=9)\n    UART.write9(0x1A5)\n    UART.write9(0x0FF)\n    while UART.sending():\n        pass\n", head()));
    let mut s = sim_for(&c);
    s.run_until(200_000, |s| s.halted);
    assert_eq!(s.read(0x98) & 0x64, 0x64); // TXSTA: TX9, TXEN, BRGH
    assert_eq!(s.read(0x18) & 0xD0, 0xD0); // RCSTA: SPEN, RX9, CREN
    assert_eq!(sent9(&s), vec![0x1A5, 0x0FF]);
    // entre dos caracteres seguidos pasa una trama de 11 bits (y no de 10)
    assert_eq!(s.uart_tx[1].1 - s.uart_tx[0].1, 11 * BIT);
    assert!(s.uart_violations.is_empty(), "{:?}", s.uart_violations);
}

#[test]
fn write9_matches_the_reference_interpreter_for_every_edge_value() {
    let src = format!(
        "{}def main() -> None:
    UART.start(baud=9600, bits=9)
    UART.write9(0)
    UART.write9(1)
    UART.write9(255)
    UART.write9(256)
    UART.write9(257)
    UART.write9(511)
    UART.write9(427)
    while UART.sending():
        pass
",
        head()
    );
    let (m, c) = build(&src);
    let mut mach = Machine::new(&m);
    mach.run_main().unwrap();
    let mut s = sim_for(&c);
    s.run_until(2_000_000, |s| s.halted);
    assert!(s.halted && s.uart_violations.is_empty(), "{:?}", s.uart_violations);
    assert_eq!(sent9(&s), vec![0, 1, 255, 256, 257, 511, 0x1AB]);
    assert_eq!(sent9(&s), mach.uart_tx9);
}

#[test]
fn only_bit_8_of_the_value_is_the_ninth_bit_and_higher_bits_are_ignored() {
    // 0x3FF = 0b11_1111_1111: byte bajo 0xFF y bit 8 = 1; 0x200 tiene el bit 9 y el bit 8 en 0
    let src = format!(
        "{}a: uint16 = 0x3FF\nb: uint16 = 0x200\ndef main() -> None:\n    UART.start(baud=9600, bits=9)\n    UART.write9(a)\n    UART.write9(b)\n    while UART.sending():\n        pass\n",
        head()
    );
    let (m, c) = build(&src);
    let mut mach = Machine::new(&m);
    mach.run_main().unwrap();
    let mut s = sim_for(&c);
    s.run_until(500_000, |s| s.halted);
    assert_eq!(sent9(&s), vec![0x1FF, 0x000]);
    assert_eq!(sent9(&s), mach.uart_tx9);
}

#[test]
fn read9_pairs_each_character_with_its_own_ninth_bit() {
    // RX9D es del caracter que esta a punto de salir del FIFO: si se leyera despues de RCREG daria el del siguiente
    let src = format!("{}a: uint16 = 0\nb: uint16 = 0\nc: uint16 = 0\ndef main() -> None:\n    global a, b, c\n    UART.start(baud=9600, bits=9)\n    n: uint16 = 0\n    while n < 20000:\n        n += 1\n    a = UART.read9()\n    b = UART.read9()\n    c = UART.recv9()\n", head());
    let (m, c) = build(&src);
    let mut s = sim_for(&c);
    // tres caracteres con la direccion (bit 9 = 1) en medio, separados por mas de una trama
    s.uart_feed9(0x11, false, 1_500);
    s.uart_feed9(0x22, true, 3_000);
    s.run_until(2_000_000, |s| s.halted);
    // el tercero no llega: recv9 con el FIFO vacio es un uso incorrecto que el simulador registra
    let g = |n: &str| c.globals.iter().find(|g| g.name == n).map(|g| s.read(g.addr as usize) as u16 | (s.read(g.addr as usize + 1) as u16) << 8).unwrap();
    assert_eq!((g("a"), g("b")), (0x011, 0x122));
    assert!(s.uart_violations.iter().any(|v| v.contains("RCREG leido")), "{:?}", s.uart_violations);
    let mut mach = Machine::new(&m);
    mach.uart_rx9.extend([0x011u16, 0x122]);
    assert!(mach.run_main().is_err()); // el intérprete traba igual en el tercero
}

#[test]
fn an_echo_of_nine_bit_characters_matches_the_interpreter() {
    let src = format!(
        "{}def main() -> None:\n    UART.start(baud=9600, bits=9)\n    for i in range(4):\n        v: uint16 = UART.read9()\n        UART.write9(v ^ 0x100)\n    while UART.sending():\n        pass\n",
        head()
    );
    let (m, c) = build(&src);
    let input = [0x000u16, 0x1FF, 0x055, 0x1AA];
    let mut mach = Machine::new(&m);
    mach.uart_rx9.extend(input);
    mach.run_main().unwrap();
    let mut s = sim_for(&c);
    for (i, v) in input.iter().enumerate() {
        s.uart_feed9(*v as u8, v & 0x100 != 0, 3_000 + i as u64 * 4_000);
    }
    s.run_until(5_000_000, |s| s.halted);
    assert!(s.halted && s.uart_violations.is_empty(), "{:?}", s.uart_violations);
    assert_eq!(sent9(&s), vec![0x100, 0x0FF, 0x155, 0x0AA]);
    assert_eq!(sent9(&s), mach.uart_tx9);
}

#[test]
fn rx_ready_sees_nine_bit_characters_and_overrun_works_the_same() {
    let src = format!("{}got: uint16 = 0\nover: bool = False\ndef main() -> None:\n    global got, over\n    UART.start(baud=9600, bits=9)\n    n: uint16 = 0\n    while n < 20000:\n        n += 1\n    over = UART.overrun()\n    got = UART.recv9()\n    UART.clear_overrun()\n", head());
    let (_, c) = build(&src);
    let mut s = sim_for(&c);
    for (i, v) in [0x101u16, 0x002, 0x1FF].iter().enumerate() {
        s.uart_feed9(*v as u8, v & 0x100 != 0, 1_500 + i as u64 * 1_000);
    }
    s.run_until(2_000_000, |s| s.halted);
    let at = |n: &str| c.globals.iter().find(|g| g.name == n).unwrap().addr as usize;
    assert_eq!(s.read(at("over")), 1);
    assert_eq!(s.read(at("got")) as u16 | (s.read(at("got") + 1) as u16) << 8, 0x101);
}

// ---- reglas -----------------------------------------------------------------------------------------------------

#[test]
fn eight_bit_and_nine_bit_operations_cannot_be_mixed() {
    let h = head();
    let (ok, t) = diagnostics(&format!("{h}def main() -> None:\n    UART.start(baud=9600, bits=9)\n    UART.write(65)\n"));
    assert!(!ok && t.contains("PYP2187") && t.contains("write9"), "{t}");
    let (ok, t) = diagnostics(&format!("{h}def main() -> None:\n    UART.start(baud=9600, bits=9)\n    x: uint8 = UART.read()\n"));
    assert!(!ok && t.contains("PYP2187"), "{t}");
    let (ok, t) = diagnostics(&format!("{h}def main() -> None:\n    UART.start(baud=9600)\n    UART.write9(65)\n"));
    assert!(!ok && t.contains("PYP2187") && t.contains("bits=9"), "{t}");
    let (ok, t) = diagnostics(&format!("{h}def main() -> None:\n    UART.start(baud=9600, bits=8)\n    UART.write(65)\n"));
    assert!(ok, "{t}");
    // las banderas valen en los dos modos
    let (ok, t) = diagnostics(&format!("{h}def main() -> None:\n    UART.start(baud=9600, bits=9)\n    while UART.sending():\n        pass\n    b: bool = UART.rx_ready()\n"));
    assert!(ok, "{t}");
}

#[test]
fn bits_and_types_are_checked() {
    let h = head();
    for bits in ["7", "10", "True", "x"] {
        let (ok, t) = diagnostics(&format!("{h}x: uint8 = 8\ndef main() -> None:\n    UART.start(baud=9600, bits={bits})\n"));
        assert!(!ok && t.contains("PYP2"), "bits={bits}: {t}");
    }
    let (ok, t) = diagnostics(&format!("{h}def main() -> None:\n    UART.start(baud=9600, bits=9)\n    v: uint8 = 1\n    UART.write9(v)\n"));
    assert!(!ok, "un uint8 no es un valor de 9 bits sin convertir: {t}");
    let (ok, t) = diagnostics(&format!("{h}def main() -> None:\n    UART.start(baud=9600, bits=9)\n    v: uint8 = UART.read9()\n"));
    assert!(!ok, "{t}");
    let (ok, t) = diagnostics(&format!("{h}def a() -> None:\n    UART.start(baud=9600, bits=9)\n\n\ndef main() -> None:\n    UART.start(baud=9600)\n    a()\n"));
    assert!(!ok && t.contains("PYP2186"), "{t}");
}

// ---- costo --------------------------------------------------------------------------------------------------------

fn cycles_of(stmt: &str, prelude: &str, feed: bool) -> u64 {
    let src = format!("{}from pypic.periph.gpio import RD0\nvar: uint16 = 0x1A5\nv: uint16 = 0\ndef main() -> None:\n    global v\n    RD0.output()\n    UART.start(baud=9600, bits=9)\n{prelude}    RD0.on()\n    {stmt}\n    RD0.off()\n", head());
    let (_, c) = build(&src);
    let mut s = sim_for(&c);
    if feed {
        s.uart_feed9(1, true, 100);
    }
    let (mut up, mut down, mut last) = (None, None, None);
    while !s.halted && s.cycles < 200_000 {
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
fn the_documented_costs_are_what_the_simulator_measures() {
    assert_eq!(cycles_of("UART.send9(0x1A5)", "", false), COST_SEND9_CONST);
    assert_eq!(cycles_of("UART.send9(var)", "", false), COST_SEND9_VAR);
    assert_eq!(cycles_of("v = UART.recv9()", "    while not UART.rx_ready():\n        pass\n", true), COST_RECV9);
}

const COST_SEND9_CONST: u64 = 6;
const COST_SEND9_VAR: u64 = 9;
const COST_RECV9: u64 = 5;
