//! Los ejemplos de `examples/uart/` compilan y se comportan como dicen, en el simulador (caso normal y un caso de error).
//! Nada de esto se ha probado con un puerto serie real.

use std::path::PathBuf;

use pypic_device::DevicePack;
use pypic_frontend::analyze;
use pypic_ir::{lower, verify};
use pypic_pic14::codegen::Compiled;
use pypic_pic14::sim::Sim;
use pypic_pic14::{compile, Options};

const FOSC: u32 = 4_000_000;
/// Ciclos de una trama de 10 bits a 9600 baudios con SPBRG = 25 y BRGH = 1: 16 * 26 * 10 / 4.
const FRAME: u64 = 1040;

fn build(name: &str) -> Compiled {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let pack = DevicePack::load(&root.join("devices/pic16f887")).unwrap();
    let src = std::fs::read_to_string(root.join("examples/uart").join(format!("{name}.py"))).unwrap();
    let r = analyze(&src, &pack.target_profile());
    let p = r.program.unwrap_or_else(|| panic!("{name}:\n{}", r.diagnostics.render(&src, "t.py")));
    let m = lower(&p);
    verify(&m).unwrap();
    compile(&m, &Options::default()).unwrap_or_else(|e| panic!("{name}: {e}"))
}

fn sim(c: &Compiled) -> Sim {
    let mut s = Sim::new(&c.image);
    s.fosc_hz = FOSC;
    s
}

/// Da los bytes uno tras otro, separados por una trama y un margen, empezando tras el arranque.
fn feed(s: &mut Sim, bytes: &[u8], gap: u64) {
    for (i, b) in bytes.iter().enumerate() {
        s.uart_feed(*b, 2_000 + i as u64 * (FRAME + gap), true);
    }
}

fn run(s: &mut Sim, cycles: u64) {
    while s.cycles < cycles {
        s.step();
    }
}

fn sent(s: &Sim) -> Vec<u8> {
    s.uart_tx.iter().map(|t| t.0).collect()
}

fn global(c: &Compiled, s: &Sim, name: &str) -> u8 {
    s.read(c.globals.iter().find(|g| g.name == name).unwrap().addr as usize)
}

#[test]
fn saludo_sends_the_message_in_order_every_second() {
    let c = build("saludo");
    let mut s = sim(&c);
    run(&mut s, 2_300_000);
    let msg = b"Hola PyPIC\r\n";
    let got = sent(&s);
    assert_eq!(got.len(), 36, "tres mensajes en 2,3 s (en 0, 1 y 2 s)");
    assert!(got.chunks(12).all(|m| m == msg));
    // el segundo mensaje sale un segundo despues del primero (mas lo que tarda en enviarse)
    let gap = s.uart_tx[12].1 - s.uart_tx[0].1;
    assert!((1_000_000..1_020_000).contains(&gap), "{gap}");
    assert!(s.uart_violations.is_empty(), "{:?}", s.uart_violations);
}

#[test]
fn eco_returns_every_byte_unchanged_including_00_and_ff() {
    let c = build("eco");
    let mut s = sim(&c);
    let input = [0x00u8, 0xFF, 0x41, 0x80, 0x7F];
    feed(&mut s, &input, 2_000);
    run(&mut s, 30_000);
    assert_eq!(sent(&s), input);
    assert!(s.uart_violations.is_empty(), "{:?}", s.uart_violations);
}

#[test]
fn eco_recovers_from_an_overrun_and_keeps_receiving() {
    let c = build("eco");
    let mut s = sim(&c);
    // el eco del 3 se queda esperando TXIF (TXREG ocupado con el 2) unos 1040 ciclos, y mientras llegan 4, 5 y 6:
    // el FIFO guarda dos y el sexto desborda
    s.uart_feed(1, 100, true);
    s.uart_feed(2, 300, true);
    for (i, b) in [3u8, 4, 5, 6].iter().enumerate() {
        s.uart_feed(*b, 400 + i as u64 * 10, true);
    }
    s.uart_feed(9, 20_000, true);
    run(&mut s, 40_000);
    let got = sent(&s);
    assert_eq!(got, vec![1, 2, 3, 4, 5, 9], "el 6 se perdio y el eco no se quedo mudo tras el desbordamiento");
}

#[test]
fn led_comandos_turns_rd0_on_and_off_and_ignores_the_rest() {
    let c = build("led_comandos");
    let mut s = sim(&c);
    // A, un byte invalido, B, otro invalido
    feed(&mut s, &[65, 90, 66, 0], 3_000);
    let step = FRAME + 3_000;
    run(&mut s, 2_000 + step / 2);
    assert_eq!(s.pin('D', 0), Some(true), "tras A");
    run(&mut s, 2_000 + step + step / 2);
    assert_eq!(s.pin('D', 0), Some(true), "un byte invalido no cambia el LED");
    run(&mut s, 2_000 + 2 * step + step / 2);
    assert_eq!(s.pin('D', 0), Some(false), "tras B");
    run(&mut s, 2_000 + 3 * step + step / 2);
    assert_eq!(s.pin('D', 0), Some(false), "un byte invalido no cambia el LED");
    assert!(sent(&s).is_empty(), "no contesta nada");
}

#[test]
fn no_bloqueante_keeps_blinking_without_data_and_echoes_when_it_arrives() {
    let c = build("no_bloqueante");
    let mut s = sim(&c);
    s.uart_feed(0x5A, 300_000, true);
    let mut toggles = 0;
    let mut last = None;
    while s.cycles < 600_000 {
        s.step();
        let p = s.pin('D', 0);
        if last.is_some() && p != last {
            toggles += 1;
        }
        last = p;
    }
    assert!(toggles >= 4, "el LED siguio cambiando sin datos: {toggles}");
    assert_eq!(sent(&s), vec![0x5A]);
}

#[test]
fn errores_discards_a_framing_error_and_reports_an_overrun() {
    let c = build("errores");
    let mut s = sim(&c);
    s.uart_feed(0x41, 2_000, true);
    s.uart_feed(0x42, 12_000, false); // sin bit de parada
    // tres bytes juntos mientras el programa duerme 5 ms: el tercero desborda
    for (i, b) in [0x51u8, 0x52, 0x53].iter().enumerate() {
        s.uart_feed(*b, 40_000 + i as u64 * 5, true);
    }
    run(&mut s, 80_000);
    let got = sent(&s);
    assert_eq!(&got[..2], &[0x41, 63], "normal y luego error de trama descartado: {got:?}");
    assert!(got.contains(&79), "aviso de desbordamiento: {got:?}");
    assert!(!got.contains(&0x42), "el byte con error no se devuelve: {got:?}");
    assert!(s.uart_violations.is_empty(), "{:?}", s.uart_violations);
}

fn nmea(body: &str) -> Vec<u8> {
    let sum = body.bytes().fold(0u8, |a, b| a ^ b);
    format!("${body}*{sum:02X}\r\n").into_bytes()
}

fn gps_run(frames: &[Vec<u8>]) -> (Compiled, Sim) {
    let c = build("gps_nmea");
    let mut s = sim(&c);
    let bytes: Vec<u8> = frames.concat();
    // el bucle de recepcion tarda decenas de ciclos por byte: una trama mas un margen holgado entre bytes
    feed(&mut s, &bytes, 500);
    let end = 2_000 + bytes.len() as u64 * (FRAME + 500) + 5_000;
    run(&mut s, end);
    (c, s)
}

#[test]
fn gps_counts_valid_frames_and_the_gga_ones() {
    let gga = nmea("GPGGA,123519,4807.038,N,01131.000,E,1,08,0.9,545.4,M,46.9,M,,");
    let rmc = nmea("GPRMC,123519,A,4807.038,N,01131.000,E,022.4,084.4,230394,003.1,W");
    let (c, s) = gps_run(&[gga, rmc]);
    assert_eq!((global(&c, &s, "validas"), global(&c, &s, "invalidas"), global(&c, &s, "gga")), (2, 0, 1));
    assert!(s.uart_violations.is_empty(), "{:?}", s.uart_violations);
}

#[test]
fn gps_rejects_a_bad_checksum_and_resynchronizes_after_a_truncated_frame() {
    let mut malo = nmea("GPGGA,1,2,3");
    let n = malo.len();
    malo[n - 4] = b'0'; // checksum alterado
    malo[n - 3] = b'0';
    let truncada = b"$GPGGA,1,2,".to_vec(); // se corta sin '*'
    let buena = nmea("GPGGA,9,9,9");
    let (c, s) = gps_run(&[malo, truncada, buena]);
    // el checksum alterado cuenta como invalido (salvo que XOR sea 00); la truncada se descarta al ver el siguiente '$'
    assert_eq!(global(&c, &s, "validas"), 1);
    assert_eq!(global(&c, &s, "gga"), 1);
    assert!(global(&c, &s, "invalidas") >= 1);
}

#[test]
fn direccion_9bits_only_obeys_data_after_its_own_address() {
    let c = build("direccion_9bits");
    let mut s = sim(&c);
    // dato sin atender ('A' ignorada), direccion ajena, dato ignorado, direccion propia (5), 'A', 'Z' (ignorada), direccion ajena, 'B' ignorada
    let seq: [(u8, bool); 8] = [(65, false), (9, true), (65, false), (5, true), (65, false), (90, false), (9, true), (66, false)];
    for (i, (b, b9)) in seq.iter().enumerate() {
        s.uart_feed9(*b, *b9, 2_000 + i as u64 * (FRAME * 11 / 10 + 3_000));
    }
    let step = FRAME * 11 / 10 + 3_000;
    run(&mut s, 2_000 + 4 * step + step / 2);
    assert_eq!(s.pin('D', 0), Some(true), "tras la direccion propia y 'A'");
    assert!(sent(&s).len() == 1 && sent(&s)[0] == 65);
    run(&mut s, 2_000 + 9 * step);
    assert_eq!(s.pin('D', 0), Some(true), "'B' tras una direccion ajena no apaga el LED");
    assert_eq!(sent(&s), vec![65], "solo contesto la 'A' atendida");
    assert!(s.uart_violations.is_empty(), "{:?}", s.uart_violations);
}

#[test]
fn adc_serial_sends_the_converted_value_as_text_for_two_different_readings() {
    let c = build("adc_serial");
    for (v, text) in [(512u16, "ADC: 0512 (0x0200)\r\n"), (1023, "ADC: 1023 (0x03FF)\r\n"), (7, "ADC: 0007 (0x0007)\r\n")] {
        let mut s = sim(&c);
        s.adc_inputs[0] = v;
        run(&mut s, 300_000);
        assert_eq!(String::from_utf8(sent(&s)).unwrap(), text, "con {v} en AN0");
        assert!(s.uart_violations.is_empty() && s.adc_violations.is_empty(), "{:?} {:?}", s.uart_violations, s.adc_violations);
    }
}
