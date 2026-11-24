//! UART (EUSART asincrono 8N1, `pypic.periph.uart`, #45): el simulador modela TXREG/TSR, TXIF (valido dos ciclos despues),
//! el FIFO de recepcion de dos caracteres, OERR, FERR y los tiempos de trama que salen de SPBRG/BRGH/BRG16.

use std::path::PathBuf;

use pypic_device::DevicePack;
use pypic_frontend::analyze;
use pypic_ir::{lower, verify, Machine};
use pypic_pic14::codegen::Compiled;
use pypic_pic14::sim::Sim;
use pypic_pic14::{compile, Options};

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

fn head(clock: u32) -> String {
    format!("from pypic.device import PIC16F887\nfrom pypic.periph.uart import UART\nCLOCK_HZ: Const[u32] = {clock}\n")
}

fn sim_for(c: &Compiled, fosc: u32) -> Sim {
    let mut s = Sim::new(&c.image);
    s.fosc_hz = fosc;
    s
}

fn global(c: &Compiled, s: &Sim, name: &str) -> i128 {
    let g = c.globals.iter().find(|g| g.name == name).unwrap();
    let n = g.ty.bits().max(8) as usize / 8;
    g.ty.wrap((0..n).map(|i| (s.read(g.addr as usize + i) as i128) << (8 * i)).sum())
}

/// Ciclos de instruccion de una trama de 10 bits con ese reloj y baudios pedidos (de la tabla 12-3, calculado aparte del compilador).
fn frame_cycles(brg16: bool, brgh: bool, n: u64) -> u64 {
    let div = match (brg16, brgh) {
        (false, false) => 64,
        (false, true) | (true, false) => 16,
        (true, true) => 4,
    };
    (10 * div * (n + 1)).div_ceil(4)
}

// ---- generador de baudios: la velocidad que sale es la pedida, medida en el simulador ----------------------------

#[test]
fn the_measured_baud_rate_is_the_requested_one_within_the_reported_error() {
    // (reloj, baudios, error maximo esperado en %)
    for (fosc, baud) in [(4_000_000u32, 9600u32), (8_000_000, 9600), (20_000_000, 115_200), (16_000_000, 115_200), (1_000_000, 1200), (4_000_000, 2400), (20_000_000, 9600), (4_000_000, 19_200), (12_000_000, 57_600)] {
        let src = format!("{}def main() -> None:\n    UART.start(baud={baud})\n    UART.write(65)\n    UART.write(66)\n    UART.write(67)\n    while UART.sending():\n        pass\n", head(fosc));
        let (_, c) = build(&src);
        let rep = c.uart.expect("el informe de la UART");
        assert!(rep.error_pct.abs() <= 2.0, "{fosc} Hz, {baud}: error {:+.2} %", rep.error_pct);
        let mut s = sim_for(&c, fosc);
        s.run_until(5_000_000, |s| s.halted);
        assert!(s.halted && s.uart_violations.is_empty(), "{:?}", s.uart_violations);
        assert_eq!(s.uart_tx.iter().map(|t| t.0).collect::<Vec<_>>(), vec![65, 66, 67]);
        // entre dos bytes seguidos pasa una trama: los baudios medidos salen de los ciclos, no de la configuracion
        let delta = s.uart_tx[2].1 - s.uart_tx[1].1;
        let observed = 10.0 * fosc as f64 / (4.0 * delta as f64);
        let err = (observed - baud as f64) / baud as f64 * 100.0;
        assert!(err.abs() <= 2.0, "{fosc} Hz, {baud}: se midieron {observed:.1} baudios ({err:+.2} %)");
        assert!((err - rep.error_pct).abs() < 0.05, "{fosc} Hz, {baud}: medido {err:+.3} %, informado {:+.3} %", rep.error_pct);
    }
}

#[test]
fn the_best_of_the_four_baud_generator_modes_is_chosen() {
    // 20 MHz y 9600: 8 bits y BRGH = 0 da n = 31 (error 0,16 %), pero BRG16 = 1 con BRGH = 1 (FOSC/4) da n = 520: error 0,01 %
    let (_, c) = build(&format!("{}def main() -> None:\n    UART.start(baud=9600)\n", head(20_000_000)));
    let r = c.uart.unwrap();
    assert!(r.brg16 && r.brgh, "{r:?}");
    assert!(r.error_pct.abs() < 0.05, "{r:?}");
    // 4 MHz y 9600: BRGH = 1 con 8 bits (n = 25, error 0,16 %) gana a BRGH = 0 (n = 5, error 4,2 %)
    let (_, c) = build(&format!("{}def main() -> None:\n    UART.start(baud=9600)\n", head(4_000_000)));
    let r = c.uart.unwrap();
    assert!(!r.brg16 && r.brgh && r.n == 25, "{r:?}");
    assert!((r.error_pct - 0.16).abs() < 0.01, "{r:?}"); // el ejemplo 12-1 del datasheet a 16 MHz da lo mismo con otro n; aqui n = 25 a 4 MHz
}

#[test]
fn the_registers_are_what_the_assembler_example_programs() {
    let (_, c) = build(&format!("{}def main() -> None:\n    UART.start(baud=9600)\n", head(4_000_000)));
    let mut s = sim_for(&c, 4_000_000);
    s.run_until(100_000, |s| s.halted);
    assert_eq!(s.read(0x99), 25); // SPBRG
    assert_eq!(s.read(0x9A), 0); // SPBRGH
    assert_eq!(s.read(0x98) & 0x24, 0x24); // TXSTA: TXEN y BRGH, SYNC = 0
    assert_eq!(s.read(0x18) & 0x90, 0x90); // RCSTA: SPEN y CREN
    assert_eq!(s.read(0x187) & 0x08, 0); // BAUDCTL: BRG16 = 0
}

#[test]
fn a_high_error_warns_and_an_impossible_one_is_an_error() {
    // 4 MHz y 115200: el mejor modo da 111111 baudios (-3,55 %): aviso
    let (ok, t) = diagnostics(&format!("{}def main() -> None:\n    UART.start(baud=115200)\n", head(4_000_000)));
    assert!(ok && t.contains("PYP3014") && t.contains("-3.55"), "{t}");
    // 4 MHz y 250000: n = 3 da 250000 exactos (FOSC/4)... y 230400 da 250000 (+8,5 %): error
    let (ok, t) = diagnostics(&format!("{}def main() -> None:\n    UART.start(baud=230400)\n", head(4_000_000)));
    assert!(!ok && t.contains("PYP2182") && t.contains("+8.51"), "{t}");
    // mas rapido que FOSC/4
    let (ok, t) = diagnostics(&format!("{}def main() -> None:\n    UART.start(baud=2000000)\n", head(4_000_000)));
    assert!(!ok && t.contains("PYP2182"), "{t}");
    // exacto: sin aviso
    let (ok, t) = diagnostics(&format!("{}def main() -> None:\n    UART.start(baud=250000)\n", head(4_000_000)));
    assert!(ok && !t.contains("PYP3014"), "{t}");
}

// ---- transmitir ---------------------------------------------------------------------------------------------------

#[test]
fn write_waits_for_the_transmitter_and_every_byte_goes_out_in_order() {
    let src = format!("{}def main() -> None:\n    UART.start(baud=9600)\n    for i in range(8):\n        UART.write(u8(i + 48))\n    while UART.sending():\n        pass\n", head(4_000_000));
    let (m, c) = build(&src);
    let mut mach = Machine::new(&m);
    mach.run_main().unwrap();
    let mut s = sim_for(&c, 4_000_000);
    s.run_until(5_000_000, |s| s.halted);
    let got: Vec<u8> = s.uart_tx.iter().map(|t| t.0).collect();
    assert_eq!(got, (48..56).collect::<Vec<u8>>());
    assert_eq!(got, mach.uart_tx);
    assert!(s.uart_violations.is_empty(), "{:?}", s.uart_violations);
    // una trama entera entre byte y byte
    let frame = frame_cycles(false, true, 25);
    assert!(s.uart_tx.windows(2).all(|w| w[1].1 - w[0].1 == frame), "{:?}", s.uart_tx);
}

#[test]
fn send_without_waiting_loses_a_byte_and_the_simulator_says_so() {
    // tres envios seguidos sin esperar TXIF: el tercero pisa al segundo, que seguia en TXREG
    let src = format!("{}def main() -> None:\n    UART.start(baud=9600)\n    UART.send(1)\n    UART.send(2)\n    UART.send(3)\n    while UART.sending():\n        pass\n", head(4_000_000));
    let (_, c) = build(&src);
    let mut s = sim_for(&c, 4_000_000);
    s.run_until(2_000_000, |s| s.halted);
    assert!(s.uart_violations.iter().any(|v| v.contains("TXREG escrito antes")), "{:?}", s.uart_violations);
    assert_eq!(s.uart_tx.iter().map(|t| t.0).collect::<Vec<_>>(), vec![1, 3]);
}

#[test]
fn tx_ready_is_valid_right_after_send_because_send_includes_the_required_nop() {
    // TXIF solo vale dos ciclos despues de escribir TXREG: send termina con un NOP para que tx_ready() inmediato diga la verdad
    let src = format!("{}a: bool = False\nb: bool = False\nc: bool = False\ndef main() -> None:\n    global a, b, c\n    UART.start(baud=9600)\n    UART.send(1)\n    a = UART.tx_ready()\n    UART.send(2)\n    b = UART.tx_ready()\n    while not UART.tx_ready():\n        pass\n    c = UART.tx_ready()\n", head(4_000_000));
    let (_, c) = build(&src);
    let mut s = sim_for(&c, 4_000_000);
    s.run_until(2_000_000, |s| s.halted);
    // el primer byte pasa al registro de desplazamiento enseguida (TXIF = 1); con el segundo en TXREG y el primero saliendo, TXIF = 0
    assert_eq!((global(&c, &s, "a"), global(&c, &s, "b"), global(&c, &s, "c")), (1, 0, 1));
    assert!(s.uart_violations.is_empty(), "{:?}", s.uart_violations);
}

#[test]
fn sending_stays_true_until_the_last_bit_has_left() {
    let src = format!("{}busy: u8 = 0\nafter: bool = True\ndef main() -> None:\n    global busy, after\n    UART.start(baud=9600)\n    UART.send(85)\n    while UART.sending():\n        busy += 1\n    after = UART.sending()\n", head(4_000_000));
    let (_, c) = build(&src);
    let mut s = sim_for(&c, 4_000_000);
    s.run_until(2_000_000, |s| s.halted);
    assert!(global(&c, &s, "busy") > 0 && global(&c, &s, "after") == 0);
    assert!(s.halted && s.uart_violations.is_empty());
}

#[test]
fn writing_before_start_is_caught_by_the_simulator() {
    let src = format!("{}def main() -> None:\n    UART.send(65)\n    UART.start(baud=9600)\n", head(4_000_000));
    let (ok, t) = diagnostics(&src);
    assert!(ok, "{t}"); // el compilador comprueba que start exista y sea alcanzable, no el orden
    let (_, c) = build(&src);
    let mut s = sim_for(&c, 4_000_000);
    s.run_until(100_000, |s| s.halted);
    assert!(s.uart_violations.iter().any(|v| v.contains("transmisor apagado")), "{:?}", s.uart_violations);
}

// ---- recibir ------------------------------------------------------------------------------------------------------

#[test]
fn an_echo_matches_the_reference_interpreter() {
    let src = format!("{}def main() -> None:\n    UART.start(baud=9600)\n    for i in range(5):\n        b: u8 = UART.read()\n        UART.write(b + 1)\n    while UART.sending():\n        pass\n", head(4_000_000));
    let (m, c) = build(&src);
    let input = [10u8, 20, 30, 40, 250];
    let mut mach = Machine::new(&m);
    mach.uart_rx.extend(input);
    mach.run_main().unwrap();
    let mut s = sim_for(&c, 4_000_000);
    for (i, b) in input.iter().enumerate() {
        s.uart_feed(*b, 3_000 + i as u64 * 4_000, true);
    }
    s.run_until(5_000_000, |s| s.halted);
    assert!(s.halted && s.uart_violations.is_empty(), "{:?}", s.uart_violations);
    let got: Vec<u8> = s.uart_tx.iter().map(|t| t.0).collect();
    assert_eq!(got, vec![11, 21, 31, 41, 251]);
    assert_eq!(got, mach.uart_tx);
}

#[test]
fn rx_ready_and_recv_read_without_blocking() {
    let src = format!("{}got: u8 = 0\nidle: u8 = 0\ndef main() -> None:\n    global got, idle\n    UART.start(baud=9600)\n    while not UART.rx_ready():\n        idle += 1\n    got = UART.recv()\n", head(4_000_000));
    let (_, c) = build(&src);
    let mut s = sim_for(&c, 4_000_000);
    s.uart_feed(77, 2_000, true);
    s.run_until(100_000, |s| s.halted);
    assert_eq!(global(&c, &s, "got"), 77);
    assert!(global(&c, &s, "idle") > 0, "esperaba sondeando hasta que llego el byte");
    assert!(s.uart_violations.is_empty(), "{:?}", s.uart_violations);
}

#[test]
fn the_receive_fifo_holds_two_characters_and_a_third_is_an_overrun() {
    let src = format!("{}a: u8 = 0\nb: u8 = 0\nover: bool = False\nafter: bool = True\nsave: u8 = 0\ndef main() -> None:\n    global a, b, over, after, save\n    UART.start(baud=9600)\n    while UART.sending():\n        pass\n    n: u16 = 0\n    while n < 20000:\n        n += 1\n    over = UART.overrun()\n    a = UART.recv()\n    b = UART.recv()\n    UART.clear_overrun()\n    after = UART.overrun()\n", head(4_000_000));
    let (_, c) = build(&src);
    let mut s = sim_for(&c, 4_000_000);
    // tres caracteres completos antes de que el programa lea nada
    for (i, b) in [1u8, 2, 3].iter().enumerate() {
        s.uart_feed(*b, 1_500 + i as u64 * 1_000, true);
    }
    s.run_until(2_000_000, |s| s.halted);
    assert_eq!((global(&c, &s, "a"), global(&c, &s, "b")), (1, 2)); // el tercero se perdio
    assert_eq!((global(&c, &s, "over"), global(&c, &s, "after")), (1, 0));
    assert!(s.uart_violations.is_empty(), "{:?}", s.uart_violations);
}

#[test]
fn a_framing_error_belongs_to_the_character_it_arrived_with() {
    let src = format!("{}f1: bool = False\nf2: bool = True\nx: u8 = 0\ny: u8 = 0\ndef main() -> None:\n    global f1, f2, x, y\n    UART.start(baud=9600)\n    n: u16 = 0\n    while n < 20000:\n        n += 1\n    f1 = UART.framing_error()\n    x = UART.recv()\n    f2 = UART.framing_error()\n    y = UART.recv()\n", head(4_000_000));
    let (_, c) = build(&src);
    let mut s = sim_for(&c, 4_000_000);
    s.uart_feed(0x55, 1_500, false); // sin bit de parada
    s.uart_feed(0x66, 2_500, true);
    s.run_until(2_000_000, |s| s.halted);
    // FERR se lee antes que RCREG y vale para el primer caracter sin leer
    assert_eq!((global(&c, &s, "f1"), global(&c, &s, "x"), global(&c, &s, "f2"), global(&c, &s, "y")), (1, 0x55, 0, 0x66));
}

#[test]
fn reading_with_nothing_received_is_caught_by_the_simulator_and_traps_in_the_reference() {
    let src = format!("{}x: u8 = 0\ndef main() -> None:\n    global x\n    UART.start(baud=9600)\n    x = UART.recv()\n", head(4_000_000));
    let (m, c) = build(&src);
    let mut s = sim_for(&c, 4_000_000);
    s.run_until(100_000, |s| s.halted);
    assert!(s.uart_violations.iter().any(|v| v.contains("RCREG leido")), "{:?}", s.uart_violations);
    assert!(Machine::new(&m).run_main().is_err());
}

#[test]
fn a_discarded_read_still_pops_the_fifo() {
    let src = format!("{}last: u8 = 0\ndef main() -> None:\n    global last\n    UART.start(baud=9600)\n    UART.read()\n    last = UART.read()\n", head(4_000_000));
    let (_, c) = build(&src);
    let mut s = sim_for(&c, 4_000_000);
    s.uart_feed(1, 2_000, true);
    s.uart_feed(2, 9_000, true);
    s.run_until(200_000, |s| s.halted);
    assert_eq!(global(&c, &s, "last"), 2);
}

// ---- reglas del lenguaje ------------------------------------------------------------------------------------------

#[test]
fn the_uart_must_be_started_and_the_start_must_be_reachable() {
    let h = head(4_000_000);
    let (ok, t) = diagnostics(&format!("{h}def main() -> None:\n    UART.write(65)\n"));
    assert!(!ok && t.contains("PYP2181") && t.contains("UART.start"), "{t}");
    let (ok, t) = diagnostics(&format!("{h}def never() -> None:\n    UART.start(baud=9600)\n\n\ndef main() -> None:\n    UART.write(65)\n"));
    assert!(!ok && t.contains("PYP2181") && t.contains("ni main ni @init_device"), "{t}");
    let (ok, t) = diagnostics(&format!("{h}def setup() -> None:\n    UART.start(baud=9600)\n\n\ndef main() -> None:\n    setup()\n    UART.write(65)\n"));
    assert!(ok, "{t}");
}

#[test]
fn start_arguments_are_checked() {
    let h = head(4_000_000);
    for (call, code) in [("UART.start()", "PYP2182"), ("UART.start(9600)", "PYP2127"), ("UART.start(baud=0)", "PYP2182"), ("UART.start(baud=True)", "PYP2182"), ("UART.start(speed=9600)", "PYP2182"), ("UART.start(baud=9600, parity=1)", "PYP2182")] {
        let (ok, t) = diagnostics(&format!("{h}def main() -> None:\n    {call}\n"));
        assert!(!ok && t.contains(code), "{call}: {t}");
    }
    let (ok, t) = diagnostics(&format!("{h}def a() -> None:\n    UART.start(baud=19200)\n\n\ndef main() -> None:\n    UART.start(baud=9600)\n    a()\n"));
    assert!(!ok && t.contains("PYP2186"), "{t}");
}

#[test]
fn method_errors_are_specific() {
    let h = head(4_000_000);
    let run = |body: &str| diagnostics(&format!("{h}def main() -> None:\n    UART.start(baud=9600)\n{body}"));
    assert!(run("    UART.nope()\n").1.contains("PYP2183"));
    assert!(run("    UART.write()\n").1.contains("PYP2055"));
    assert!(run("    x: u8 = UART.read(1)\n").1.contains("PYP2055"));
    assert!(run("    x = UART\n").1.contains("PYP2185"));
    assert!(run("    UART.write(300)\n").1.contains("PYP2"));
    // el resultado de read es u8; mezclarlo con u16 pide conversion
    assert!(run("    x: u16 = UART.read()\n").1.contains("PYP2041"));
    assert!(diagnostics("from pypic.device import PIC16F887\nfrom pypic.periph.uart import SERIAL\n").1.contains("PYP2180"));
}

#[test]
fn the_pins_of_the_uart_cannot_also_be_digital_io() {
    let src = format!("{}from pypic.periph.gpio import RC6\ndef main() -> None:\n    RC6.output()\n    UART.start(baud=9600)\n", head(4_000_000));
    let (ok, t) = diagnostics(&src);
    assert!(!ok && t.contains("PYP2150") && t.contains("RC6") && t.contains("TX") && t.contains("EUSART"), "{t}");
    let src = format!("{}from pypic.periph.gpio import RC7\ndef main() -> None:\n    RC7.input()\n    UART.start(baud=9600)\n", head(4_000_000));
    let (ok, t) = diagnostics(&src);
    assert!(!ok && t.contains("PYP2150") && t.contains("RC7") && t.contains("RX"), "{t}");
}

#[test]
fn the_uart_can_be_read_in_an_interrupt() {
    // no usa memoria compartida ni esperas: rx_ready y recv valen en la ISR
    let src = format!("{}from pypic import interrupt\ngot: u8 = 0\n\n\n@interrupt\ndef isr() -> None:\n    global got\n    if UART.rx_ready():\n        got = UART.recv()\n\n\ndef main() -> None:\n    UART.start(baud=9600)\n", head(4_000_000));
    let (_, c) = build(&src);
    assert!(c.flash_words > 0);
}

// ---- costo --------------------------------------------------------------------------------------------------------

fn cycles_of(stmt: &str, prelude: &str, feed: bool) -> u64 {
    let src = format!("{}from pypic.periph.gpio import RD0\nv: u8 = 0\ndef main() -> None:\n    global v\n    RD0.output()\n    UART.start(baud=9600)\n{prelude}    RD0.on()\n    {stmt}\n    RD0.off()\n", head(4_000_000));
    let (_, c) = build(&src);
    let mut s = sim_for(&c, 4_000_000);
    if feed {
        s.uart_feed(1, 100, true);
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
    // con el transmisor libre y un byte esperando
    assert_eq!(cycles_of("UART.send(65)", "", false), 3);
    assert_eq!(cycles_of("UART.write(65)", "", false), 5); // un sondeo de TXIF (2) + 3 de send
    assert_eq!(cycles_of("v = UART.recv()", "    while not UART.rx_ready():\n        pass\n", true), 2);
    assert_eq!(cycles_of("v = UART.read()", "    while not UART.rx_ready():\n        pass\n", true), 4);
    assert_eq!(cycles_of("UART.start(baud=9600)", "", false), 20);
}

#[test]
fn polling_txif_immediately_after_writing_txreg_reads_a_stale_value_which_is_why_send_has_a_nop() {
    // TXIF solo es valido en el segundo ciclo tras escribir TXREG (p.150): una instruccion en medio lo arregla
    let prog = |gap: &str| {
        format!("{}from pypic.regs import TXREG, PIR1\nfrom pypic.system.time import delay_cycles\nseen: u8 = 7\ndef main() -> None:\n    global seen\n    UART.start(baud=9600)\n    UART.send(1)\n    TXREG.write(2)\n{gap}    raw: u8 = PIR1.read()\n    seen = (raw >> 4) & 1\n    while UART.sending():\n        pass\n", head(4_000_000))
    };
    let run = |src: String| {
        let (_, c) = build(&src);
        let mut s = sim_for(&c, 4_000_000);
        s.run_until(2_000_000, |s| s.halted);
        global(&c, &s, "seen")
    };
    // TXREG ya tiene el byte 2 y el registro de desplazamiento esta ocupado con el 1: TXIF real = 0
    assert_eq!(run(prog("    delay_cycles(1)\n")), 0, "con un ciclo de por medio TXIF es valido");
    assert_eq!(run(prog("")), 1, "sin nada en medio se lee el valor viejo: parece listo y no lo esta");
}

#[test]
fn the_flag_primitives_have_a_fixed_cost() {
    for (call, want) in [("b: bool = UART.tx_ready()", 3u64), ("b: bool = UART.rx_ready()", 3), ("b: bool = UART.overrun()", 3), ("b: bool = UART.framing_error()", 3), ("UART.clear_overrun()", 2), ("b: bool = UART.sending()", 7)] {
        assert_eq!(cycles_of(call, "", false), want, "{call}");
    }
}
