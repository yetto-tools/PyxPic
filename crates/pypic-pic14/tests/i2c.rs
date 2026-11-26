//! MSSP en modo I2C maestro (docs/sdk-i2c.md): el modelo del simulador con esclavos virtuales y el ejemplo de la pantalla LCD 16x2 con mochila PCF8574.
//! Sin pruebas en Proteus ni en hardware.

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

fn build(src: &str) -> Compiled {
    let r = analyze(src, &pack().target_profile());
    let p = r.program.unwrap_or_else(|| panic!("frontend:\n{}", r.diagnostics.render(src, "t.py")));
    let m = lower(&p);
    verify(&m).unwrap_or_else(|e| panic!("IR invalida: {e:?}"));
    compile(&m, &Options::default()).unwrap_or_else(|e| panic!("codegen: {e}"))
}

fn lcd_source() -> String {
    std::fs::read_to_string(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/python/i2c/lcd_i2c.py")).unwrap().replace("\r\n", "\n")
}

/// La direccion de 7 bits que declara el ejemplo del LCD (`DIRECCION` ya viene desplazada: 0x4E es la 0x27).
fn lcd_addr() -> u8 {
    let src = lcd_source();
    let line = src.lines().find(|l| l.starts_with("DIRECCION")).expect("el ejemplo declara DIRECCION");
    let value = line.split('=').nth(1).unwrap().split('#').next().unwrap().trim().trim_start_matches("0x");
    u8::from_str_radix(value, 16).unwrap() >> 1
}

/// Lo que el HD44780 entiende de lo que llega a la mochila: cada subida y bajada de E (P2) toma el nibble de P7..P4 con RS en P0. Devuelve
/// (RS, nibble) en orden.
fn nibbles(bytes: &[u8]) -> Vec<(u8, u8)> {
    let mut out = Vec::new();
    let mut prev_e = false;
    for b in bytes {
        let e = b & 0x04 != 0;
        if prev_e && !e {
            out.push((b & 1, b >> 4));
        }
        prev_e = e;
    }
    out
}

#[test]
fn the_lcd_example_writes_both_lines_through_the_i2c_bus() {
    let c = build(&lcd_source());
    let mut s = Sim::new(&c.image);
    s.i2c.slaves.push(lcd_addr());
    s.run_until(2_000_000, |_| false);
    assert!(s.i2c.violations.is_empty(), "{:?}", s.i2c.violations);

    // cada transaccion es START, la direccion del ejemplo + escritura con ACK, un dato con ACK y STOP (4 eventos)
    let ev = &s.i2c.events;
    assert!(ev.len().is_multiple_of(4) && !ev.is_empty());
    for t in ev.chunks(4) {
        assert_eq!(t[0], I2cEvent::Start);
        assert_eq!(t[1], I2cEvent::Address { addr: lcd_addr(), read: false, ack: true });
        assert!(matches!(t[2], I2cEvent::Data { ack: true, .. }));
        assert_eq!(t[3], I2cEvent::Stop, "{t:?}");
    }
}

#[test]
fn the_lcd_example_sends_the_hd44780_init_and_the_text() {
    let c = build(&lcd_source());
    let mut s = Sim::new(&c.image);
    s.i2c.slaves.push(lcd_addr());
    s.run_until(2_000_000, |_| false);
    let bytes = s.i2c.received_by(lcd_addr());
    assert!(bytes.iter().all(|b| b & 0x08 != 0), "la luz de fondo (P3) va encendida en todos los bytes");
    let n = nibbles(&bytes);
    // arranque a 4 bits: 3, 3, 3 y 2 como nibbles sueltos
    assert_eq!(&n[..4], &[(0, 3), (0, 3), (0, 3), (0, 2)]);
    // lo demas son bytes de dos nibbles (alto primero)
    let sent: Vec<(u8, u8)> = n[4..].chunks(2).map(|p| (p[0].0, p[0].1 << 4 | p[1].1)).collect();
    let commands: Vec<u8> = sent.iter().filter(|(rs, _)| *rs == 0).map(|(_, b)| *b).collect();
    assert_eq!(commands, vec![0x28, 0x0C, 0x01, 0x06, 0x80, 0xC0], "iniciar, borrar, y las dos direcciones de linea");
    let text: String = sent.iter().filter(|(rs, _)| *rs == 1).map(|(_, b)| *b as char).collect();
    assert_eq!(text, "Hola PyPICI2C 100 kHz");
    // el texto de cada linea va tras su direccion: 0x80, 10 caracteres, 0xC0, 11 caracteres
    let order: Vec<(u8, u8)> = sent.iter().skip(4).cloned().collect();
    assert_eq!(order[0], (0, 0x80));
    assert_eq!(order[11], (0, 0xC0));
}

#[test]
fn an_absent_address_gets_no_ack_and_acknowledges_nothing() {
    let c = build(&lcd_source());
    let mut s = Sim::new(&c.image);
    s.i2c.slaves.push(lcd_addr() + 1); // la mochila esta en otra direccion
    s.run_until(300_000, |_| false);
    assert!(s.i2c.events.iter().any(|e| matches!(e, I2cEvent::Address { addr, ack: false, .. } if *addr == lcd_addr())));
    assert!(s.i2c.received_by(lcd_addr()).is_empty());
    assert!(s.i2c.events.iter().all(|e| !matches!(e, I2cEvent::Data { ack: true, .. })));
}

#[test]
fn a_probe_reads_acks_from_the_ack_status_bit() {
    let src = r#"from pypic.device import PIC16F887
from pypic.regs import SSPADD, SSPBUF, SSPCON, SSPCON2, SSPSTAT

CLOCK: Internal[4 * MHz]

visto: uint8 = 0
ausente: uint8 = 0


def esperar() -> None:
    while (SSPCON2.read() & 0x1F) != 0 or SSPSTAT.R_W.value() != 0:
        pass


def sondear(direccion: uint8) -> uint8:
    SSPCON2.SEN.set()
    esperar()
    SSPBUF.write(direccion << 1)
    esperar()
    nack: uint8 = SSPCON2.ACKSTAT.value()
    SSPCON2.PEN.set()
    esperar()
    return nack


def main() -> None:
    SSPADD.write(9)
    SSPCON.write(0x28)
    global visto
    global ausente
    visto = sondear(0x20)
    ausente = sondear(0x21)
    while True:
        pass
"#;
    let c = build(src);
    let mut s = Sim::new(&c.image);
    s.i2c.slaves.push(0x20);
    s.run_until(20_000, |_| false);
    let g = |n: &str| s.read(c.globals.iter().find(|g| g.name == n).unwrap().addr as usize);
    assert_eq!((g("visto"), g("ausente")), (0, 1), "ACKSTAT: 0 si el esclavo respondio, 1 si no");
    assert!(s.i2c.violations.is_empty(), "{:?}", s.i2c.violations);
}

#[test]
fn writing_sspbuf_during_a_bus_event_sets_wcol_and_is_reported() {
    let src = r#"from pypic.device import PIC16F887
from pypic.regs import SSPADD, SSPBUF, SSPCON, SSPCON2

CLOCK: Internal[4 * MHz]


def main() -> None:
    SSPADD.write(9)
    SSPCON.write(0x28)
    SSPCON2.SEN.set()
    SSPBUF.write(0x40)
    while True:
        pass
"#;
    let c = build(src);
    let mut s = Sim::new(&c.image);
    s.i2c.slaves.push(0x20);
    s.run_until(2_000, |_| false);
    assert!(s.read(0x14) & 0x80 != 0, "WCOL");
    assert!(s.i2c.violations.iter().any(|v| v.contains("WCOL")), "{:?}", s.i2c.violations);
    assert!(s.i2c.events.iter().all(|e| !matches!(e, I2cEvent::Address { .. })), "la escritura no se hizo");
}

#[test]
fn a_bus_event_without_i2c_start_is_a_violation() {
    let src = r#"from pypic.device import PIC16F887
from pypic.regs import SSPCON2

CLOCK: Internal[4 * MHz]


def main() -> None:
    SSPCON2.SEN.set()
    while True:
        pass
"#;
    let c = build(src);
    let mut s = Sim::new(&c.image);
    s.run_until(500, |_| false);
    assert!(s.i2c.violations.iter().any(|v| v.contains("falta I2C.start")), "{:?}", s.i2c.violations);
    assert!(s.i2c.events.is_empty());
}

#[test]
fn bus_timing_follows_sspadd() {
    // un STOP dura un periodo de SCL (SSPADD + 1 ciclos de instruccion) y un byte con su ACK, nueve
    let mk = |add: u8| {
        format!("from pypic.device import PIC16F887\nfrom pypic.regs import SSPADD, SSPBUF, SSPCON, SSPCON2, SSPSTAT\n\nCLOCK: Internal[4 * MHz]\n\n\ndef main() -> None:\n    SSPADD.write({add})\n    SSPCON.write(0x28)\n    SSPCON2.SEN.set()\n    while SSPCON2.SEN.value() != 0:\n        pass\n    SSPBUF.write(0x40)\n    while SSPSTAT.R_W.value() != 0:\n        pass\n    SSPCON2.PEN.set()\n    while SSPCON2.PEN.value() != 0:\n        pass\n    while True:\n        pass\n")
    };
    let cycles = |add: u8| {
        let c = build(&mk(add));
        let mut s = Sim::new(&c.image);
        s.i2c.slaves.push(0x20);
        s.run_until(100_000, |sim| sim.i2c.events.len() == 3);
        s.cycles
    };
    let (slow, fast) = (cycles(39), cycles(9)); // 25 kHz y 100 kHz a 4 MHz
                                                // 11 periodos de SCL (START, nueve del byte y STOP) mas el codigo del programa: la diferencia es 11 x (40 - 10)
                                                // (el programa sondea los bits de fin de operacion: cada espera puede terminar unos ciclos despues del evento)
    let diff = (slow - fast) as i64 - 11 * 30;
    assert!(diff.abs() <= 12, "slow={slow} fast={fast}");
}

#[test]
fn the_ack_led_diagnostic_shows_the_bus_answer_on_rd0() {
    let src = std::fs::read_to_string(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/python/i2c/ack_led.py")).unwrap().replace("\r\n", "\n");
    let c = build(&src);
    // con la mochila: tras el destello inicial, RD0 queda en alto y fijo
    let mut s = Sim::new(&c.image);
    s.i2c.slaves.push(0x27);
    s.run_until(400_000, |_| false);
    let levels: Vec<Option<bool>> = (0..5)
        .map(|_| {
            s.run_until(30_000, |_| false);
            s.pin('D', 0)
        })
        .collect();
    assert!(levels.iter().all(|l| *l == Some(true)), "{levels:?}");
    // sin la mochila: parpadea
    let mut s = Sim::new(&c.image);
    s.run_until(400_000, |_| false);
    let levels: Vec<Option<bool>> = (0..8)
        .map(|_| {
            s.run_until(100_000, |_| false);
            s.pin('D', 0)
        })
        .collect();
    assert!(levels.contains(&Some(true)) && levels.contains(&Some(false)), "{levels:?}");
}

#[test]
fn the_mapping_diagnostic_writes_its_text_under_both_pcf8574_layouts() {
    let src = std::fs::read_to_string(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/python/i2c/lcd_mapas.py")).unwrap().replace("\r\n", "\n");
    let c = build(&src);
    let mut s = Sim::new(&c.image);
    s.i2c.slaves.push(0x27);
    // la primera disposicion termina antes de 0,5 s; la segunda empieza tras los 3 s de pausa
    s.run_until(1_500_000, |_| false);
    let first = s.i2c.received_by(0x27);
    s.run_until(2_500_000, |_| false);
    let all = s.i2c.received_by(0x27);
    let second = all[first.len()..].to_vec();
    assert!(s.i2c.violations.is_empty(), "{:?}", s.i2c.violations);
    // lo que entiende cada disposicion: de donde sale el nibble, el bit de RS y el de E
    let decode = |bytes: &[u8], nib: &dyn Fn(u8) -> u8, rs: u8, e: u8| -> String {
        let mut out = Vec::new();
        let mut prev = false;
        for b in bytes {
            let high = b & e != 0;
            if prev && !high {
                out.push((u8::from(b & rs != 0), nib(*b)));
            }
            prev = high;
        }
        let pairs: Vec<(u8, u8)> = out[4..].chunks(2).filter(|p| p.len() == 2).map(|p| (p[0].0, p[0].1 << 4 | p[1].1)).collect();
        pairs.iter().filter(|(r, _)| *r == 1).map(|(_, b)| *b as char).collect()
    };
    assert_eq!(decode(&first, &|b| b >> 4, 0x01, 0x04), "MAPA 1");
    assert_eq!(decode(&second, &|b| b & 0x0F, 0x10, 0x40), "MAPA 2");
}

#[test]
fn the_address_search_shows_each_responding_address_on_its_own_screen_and_on_the_uart() {
    let src = std::fs::read_to_string(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/python/i2c/lcd_buscar.py")).unwrap().replace("\r\n", "\n");
    let c = build(&src);
    let mut s = Sim::new(&c.image);
    s.fosc_hz = 4_000_000;
    s.i2c.slaves.extend([0x26, 0x27, 0x3F]);
    s.run_until(2_000_000, |_| false);
    assert!(s.i2c.violations.is_empty(), "{:?}", s.i2c.violations);
    let uart: String = s.uart_tx.iter().map(|(b, _)| *b as char).collect();
    // recorre de menor a mayor y avisa de cada direccion que responde
    let found: Vec<u8> = vec![0x26, 0x27, 0x3F];
    let expected: String = found.iter().map(|a| format!("ACK {a:02X}\r\n")).collect();
    assert_eq!(uart, expected);
    // cada pantalla recibe su propia direccion en hexadecimal tras "DIR "
    for a in found {
        let n = nibbles(&s.i2c.received_by(a));
        let pairs: Vec<(u8, u8)> = n[4..].chunks(2).filter(|p| p.len() == 2).map(|p| (p[0].0, p[0].1 << 4 | p[1].1)).collect();
        let text: String = pairs.iter().filter(|(r, _)| *r == 1).map(|(_, b)| *b as char).collect();
        assert_eq!(text, format!("DIR {a:02X}"));
    }
    // solo las direcciones declaradas responden ACK, y el recorrido llega a las 112 posibles (0x08 a 0x77)
    let probed: std::collections::BTreeSet<u8> = s.i2c.events.iter().filter_map(|e| if let I2cEvent::Address { addr, .. } = e { Some(*addr) } else { None }).collect();
    assert!((0x08..=0x77).all(|a| probed.contains(&a)));
    assert!(s.i2c.events.iter().all(|e| match e {
        I2cEvent::Address { addr, ack, .. } => *ack == [0x26, 0x27, 0x3F].contains(addr),
        _ => true,
    }));
}
