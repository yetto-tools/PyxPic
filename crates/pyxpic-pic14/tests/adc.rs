//! Convertidor analogico-digital (`pyxpic.periph.adc`, #45): el simulador modela GO/DONE, ADRES, TAD y la adquisicion.

use std::path::PathBuf;

use pyxpic_device::DevicePack;
use pyxpic_frontend::analyze;
use pyxpic_ir::{lower, verify, Machine};
use pyxpic_pic14::codegen::{builtin_analog_pins, Compiled};
use pyxpic_pic14::sim::Sim;
use pyxpic_pic14::{compile, Options};

fn pack() -> DevicePack {
    DevicePack::load(&PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../devices/pic16f887")).unwrap()
}

fn build(src: &str) -> (pyxpic_ir::Module, Compiled) {
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

fn global(c: &Compiled, s: &Sim, name: &str) -> i128 {
    let g = c.globals.iter().find(|g| g.name == name).unwrap();
    let n = g.ty.bits().max(8) as usize / 8;
    g.ty.wrap((0..n).map(|i| (s.read(g.addr as usize + i) as i128) << (8 * i)).sum())
}

/// Ejecuta en la IR y en el simulador con las mismas entradas analogicas y compara todas las globales.
fn differential(src: &str, inputs: &[(usize, u16)], fosc_hz: u32) -> (Compiled, Sim) {
    let (m, c) = build(src);
    let mut mach = Machine::new(&m);
    let mut s = Sim::new(&c.image);
    s.fosc_hz = fosc_hz;
    for &(ch, v) in inputs {
        mach.adc_inputs[ch] = v as i128;
        s.adc_inputs[ch] = v;
    }
    mach.run_main().unwrap();
    s.run_until(5_000_000, |s| s.halted);
    assert!(s.halted, "el programa no termino\n{src}");
    for g in c.globals.iter().filter(|g| !g.is_const) {
        assert_eq!(global(&c, &s, &g.name), mach.global(&g.name).unwrap(), "global {}\n{src}", g.name);
    }
    (c, s)
}

fn head(clock: u32) -> String {
    format!("from pyxpic.device import PIC16F887\nfrom pyxpic.periph.adc import ADC, AN0, AN5, AN13, FOSC_2, FOSC_8, FOSC_32, FRC\nCLOCK_HZ: Const[u32] = {clock}\n")
}

// ---- lectura -----------------------------------------------------------------------------------------

#[test]
fn read_returns_the_ten_bit_value_right_justified() {
    let src = format!(
        "{}a: u16 = 0\nb: u16 = 0\nc: u16 = 0\ndef main() -> None:\n    global a, b, c\n    ADC.start()\n    a = ADC.read(AN0)\n    b = ADC.read(AN5)\n    c = ADC.read(AN13)\n",
        head(4_000_000)
    );
    let (c, s) = differential(&src, &[(0, 1023), (5, 513), (13, 0)], 4_000_000);
    assert_eq!((global(&c, &s, "a"), global(&c, &s, "b"), global(&c, &s, "c")), (1023, 513, 0));
    assert!(s.adc_violations.is_empty(), "{:?}", s.adc_violations);
}

/// Solo en el simulador: el interprete de referencia convierte al instante, asi que no puede dar la misma cuenta de sondeos.
fn simulate(src: &str, inputs: &[(usize, u16)], fosc_hz: u32) -> (Compiled, Sim) {
    let (_, c) = build(src);
    let mut s = Sim::new(&c.image);
    s.fosc_hz = fosc_hz;
    for &(ch, v) in inputs {
        s.adc_inputs[ch] = v;
    }
    s.run_until(5_000_000, |s| s.halted);
    assert!(s.halted, "el programa no termino\n{src}");
    (c, s)
}

#[test]
fn the_primitives_give_the_assembler_level_control() {
    // select, esperar la adquisicion, go, sondear done y leer el resultado: lo que hace read, paso a paso
    let body = "    ADC.start()\n    ADC.select(AN5)\n    delay_us(12)\n    ADC.go()\n    while not ADC.done():\n        spins += 1\n    v = ADC.result()\n";
    let src = format!("{}from pyxpic.system.time import delay_us\nv: u16 = 0\nspins: u8 = 0\ndef main() -> None:\n    global v, spins\n{body}", head(4_000_000));
    let (c, s) = simulate(&src, &[(5, 777)], 4_000_000);
    assert_eq!(global(&c, &s, "v"), 777);
    // la conversion de 11 TAD (FOSC/8 a 4 MHz: 2 us cada uno = 22 us) tarda: done() da False al principio
    assert!(global(&c, &s, "spins") > 0);
    assert!(s.adc_violations.is_empty(), "{:?}", s.adc_violations);
    // y el resultado coincide con el de referencia
    let (m, _) = build(&src);
    let mut mach = Machine::new(&m);
    mach.adc_inputs[5] = 777;
    mach.run_main().unwrap();
    assert_eq!(mach.global("v").unwrap(), 777);
}

#[test]
fn a_conversion_can_overlap_other_work() {
    let src = format!("{}from pyxpic.system.time import delay_us\nv: u16 = 0\nwork: u8 = 0\ndef main() -> None:\n    global v, work\n    ADC.start()\n    ADC.select(AN0)\n    delay_us(12)\n    ADC.go()\n    work = 7\n    work += 1\n    while not ADC.done():\n        pass\n    v = ADC.result()\n", head(4_000_000));
    let (c, s) = differential(&src, &[(0, 300)], 4_000_000);
    assert_eq!((global(&c, &s, "v"), global(&c, &s, "work")), (300, 8));
}

#[test]
fn select_makes_the_pin_an_analog_input() {
    let src = format!("{}v: u16 = 0\ndef main() -> None:\n    global v\n    ADC.start()\n    v = ADC.read(AN5)\n", head(4_000_000));
    let (_, s) = differential(&src, &[(5, 1)], 4_000_000);
    // AN5 es RE0: TRISE<0> (89h) = 1 y ANSEL<5> (188h) = 1
    assert_eq!(s.read(0x89) & 1, 1);
    assert_eq!(s.read(0x188) >> 5 & 1, 1);
    // ADCON0 conserva el reloj y ADON tras elegir canal (CHS = 5)
    assert_eq!(s.read(0x1F) >> 2 & 0xF, 5);
    assert_eq!(s.read(0x1F) & 1, 1);
}

// ---- reloj de conversion automatico ------------------------------------------------------------------------

#[test]
fn the_automatic_clock_keeps_tad_inside_the_datasheet_range_at_every_frequency() {
    // frecuencia -> reloj elegido (ADCS): TAD entre 1,6 y 6,0 us con el mas rapido que lo cumple
    for (hz, adcs) in [(1_000_000u32, 0u8), (2_000_000, 1), (4_000_000, 1), (8_000_000, 2), (16_000_000, 2), (20_000_000, 2)] {
        let src = format!("{}v: u16 = 0\ndef main() -> None:\n    global v\n    ADC.start()\n    v = ADC.read(AN0)\n", head(hz));
        let (_, s) = differential(&src, &[(0, 600)], hz);
        assert_eq!(s.read(0x1F) >> 6, adcs, "a {hz} Hz");
        assert!(s.adc_violations.is_empty(), "a {hz} Hz: {:?}", s.adc_violations);
    }
}

#[test]
fn a_very_slow_clock_falls_back_to_the_internal_rc() {
    // a 32 kHz ningun FOSC/n da un TAD de 1,6..6 us: se usa el RC propio del ADC
    let src = format!("{}v: u16 = 0\ndef main() -> None:\n    global v\n    ADC.start()\n    v = ADC.read(AN0)\n", head(32_768));
    let (_, s) = differential(&src, &[(0, 600)], 32_768);
    assert_eq!(s.read(0x1F) >> 6, 3);
}

#[test]
fn an_explicit_clock_is_honoured_and_a_bad_one_is_warned() {
    let ok = format!("{}v: u16 = 0\ndef main() -> None:\n    global v\n    ADC.start(clock=FOSC_32)\n    v = ADC.read(AN0)\n", head(8_000_000));
    let (_, s) = differential(&ok, &[(0, 5)], 8_000_000);
    assert_eq!(s.read(0x1F) >> 6, 2);
    // FOSC/8 a 8 MHz son 1,0 us: viola el minimo de TAD; el compilador avisa y el simulador lo registra
    let bad = format!("{}v: u16 = 0\ndef main() -> None:\n    global v\n    ADC.start(clock=FOSC_8)\n    v = ADC.read(AN0)\n", head(8_000_000));
    let (ok_compiles, text) = diagnostics(&bad);
    assert!(ok_compiles && text.contains("PYP3012") && text.contains("1000 ns"), "{text}");
    let (_, s) = differential(&bad, &[(0, 5)], 8_000_000);
    assert!(s.adc_violations.iter().any(|v| v.contains("TAD")), "{:?}", s.adc_violations);
}

// ---- adquisicion --------------------------------------------------------------------------------------------

#[test]
fn read_waits_the_acquisition_time_after_changing_channel_at_any_clock() {
    for hz in [1_000_000u32, 4_000_000, 8_000_000, 20_000_000] {
        let src = format!("{}v: u16 = 0\ndef main() -> None:\n    global v\n    ADC.start()\n    v = ADC.read(AN0)\n    v = ADC.read(AN5)\n    v = ADC.read(AN0)\n", head(hz));
        let (_, s) = differential(&src, &[(0, 1), (5, 2)], hz);
        assert!(s.adc_violations.is_empty(), "a {hz} Hz: {:?}", s.adc_violations);
    }
}

#[test]
fn going_without_the_wait_is_caught_by_the_simulator() {
    // select + go inmediato: la adquisicion es de unos pocos ciclos (menos de 5 us)
    let src = format!(
        "{}v: u16 = 0\ndef main() -> None:\n    global v\n    ADC.start()\n    ADC.select(AN5)\n    ADC.go()\n    while not ADC.done():\n        pass\n    v = ADC.result()\n",
        head(4_000_000)
    );
    let (_, s) = differential(&src, &[(5, 9)], 4_000_000);
    assert!(s.adc_violations.iter().any(|v| v.contains("adquisicion")), "{:?}", s.adc_violations);
}

#[test]
fn the_timing_report_lists_the_acquisition_wait() {
    let src = format!("{}v: u16 = 0\ndef main() -> None:\n    global v\n    ADC.start()\n    v = ADC.read(AN0)\n", head(4_000_000));
    let (_, c) = build(&src);
    // 11,5 us (AD132 tipico) a 4 MHz son 12 ciclos de instruccion
    assert!(c.timing.iter().any(|t| t.cycles == 12), "{:?}", c.timing.iter().map(|t| t.cycles).collect::<Vec<_>>());
}

// ---- reglas del lenguaje ------------------------------------------------------------------------------------

#[test]
fn the_adc_must_be_started() {
    let src = format!("{}v: u16 = 0\ndef main() -> None:\n    global v\n    v = ADC.read(AN0)\n", head(4_000_000));
    let (ok, text) = diagnostics(&src);
    assert!(!ok && text.contains("PYP2171") && text.contains("ADC.start()"), "{text}");
}

#[test]
fn a_pin_used_as_an_analog_channel_cannot_also_be_digital_io() {
    let src = format!("{}from pyxpic.periph.gpio import RA0\nv: u16 = 0\ndef main() -> None:\n    global v\n    RA0.output()\n    ADC.start()\n    v = ADC.read(AN0)\n", head(4_000_000));
    let (ok, text) = diagnostics(&src);
    assert!(!ok && text.contains("PYP2150") && text.contains("RA0") && text.contains("AN0") && text.contains("ADC"), "{text}");
}

#[test]
fn usage_errors_are_specific() {
    let h = head(4_000_000);
    let run = |body: &str| diagnostics(&format!("{h}v: u16 = 0\ndef main() -> None:\n    global v\n    ADC.start()\n{body}"));
    assert!(run("    v = ADC.read(7)\n").1.contains("PYP2174"));
    assert!(run("    v = ADC.read()\n").1.contains("PYP2055"));
    assert!(run("    ADC.nope()\n").1.contains("PYP2173"));
    assert!(run("    x = ADC\n").1.contains("PYP2175"));
    assert!(run("    x: u8 = AN0\n").1.contains("PYP2175"));
    assert!(run("    ADC.go(1)\n").1.contains("PYP2055"));
    let (ok, t) = diagnostics(&format!("{h}def main() -> None:\n    ADC.start(speed=1)\n"));
    assert!(!ok && t.contains("PYP2172"), "{t}");
    let (ok, t) = diagnostics("from pyxpic.device import PIC16F887\nfrom pyxpic.periph.adc import AN14\n");
    assert!(!ok && t.contains("PYP2170") && t.contains("AN13"), "{t}");
}

#[test]
fn the_result_types_are_checked() {
    let h = head(4_000_000);
    let (ok, t) = diagnostics(&format!("{h}def main() -> None:\n    ADC.start()\n    x: u8 = ADC.read(AN0)\n"));
    assert!(!ok && t.contains("PYP2041"), "{t}"); // el resultado es u16: u8 pide una conversion explicita
    let (ok, t) = diagnostics(&format!("{h}def main() -> None:\n    ADC.start()\n    x: u8 = u8(ADC.read(AN0) >> 2)\n"));
    assert!(ok, "{t}");
}

// ---- el backend ------------------------------------------------------------------------------------------------

#[test]
fn the_builtin_analog_pin_table_matches_the_device_pack() {
    let p = pack();
    let mut from_pack = std::collections::BTreeMap::new();
    for pin in &p.pins.packages[0].pins {
        if let (Some(port), Some(bit)) = (&pin.port, pin.bit) {
            for f in &pin.functions {
                if let Some(n) = f.strip_prefix("AN").and_then(|n| n.parse::<u8>().ok()) {
                    from_pack.insert(n, (port.chars().next().unwrap(), bit as u8));
                }
            }
        }
    }
    assert_eq!(builtin_analog_pins(), from_pack);
}

#[test]
fn the_adc_in_an_interrupt_uses_the_primitives_and_read_is_rejected() {
    let isr = |body: &str| format!("{}from pyxpic import interrupt\nv: u16 = 0\n\n\n@interrupt\ndef isr() -> None:\n    global v\n{body}\n\n\ndef main() -> None:\n    ADC.start()\n", head(4_000_000));
    // done() y result() no esperan: valen en la interrupcion
    let (_, c) = build(&isr("    if ADC.done():\n        v = ADC.result()\n"));
    assert!(c.flash_words > 0);
    // read incluye la espera de adquisicion, que no se admite en la interrupcion
    let src = isr("    v = ADC.read(AN0)\n");
    let r = analyze(&src, &pack().target_profile());
    let m = lower(&r.program.unwrap());
    let e = compile(&m, &Options::default()).unwrap_err();
    assert!(e.to_string().contains("esperas dentro de la interrupcion"), "{e}");
}

// ---- costo ---------------------------------------------------------------------------------------------------

/// Ciclos de una sentencia entre el BSF y el BCF de RD0 (sin el BCF).
fn cycles_of(stmt: &str, fosc: u32) -> u64 {
    let src = format!("{}from pyxpic.periph.gpio import RD0\nv: u16 = 0\ndef main() -> None:\n    global v\n    RD0.output()\n    ADC.start()\n    RD0.on()\n    {stmt}\n    RD0.off()\n", head(fosc));
    let (_, c) = build(&src);
    let mut s = Sim::new(&c.image);
    s.fosc_hz = fosc;
    s.adc_inputs[0] = 100;
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
    // primitivas: cuestan lo mismo con cualquier reloj
    for hz in [1_000_000u32, 4_000_000, 20_000_000] {
        assert_eq!(cycles_of("ADC.select(AN0)", hz), 12, "select a {hz} Hz");
        assert_eq!(cycles_of("ADC.go()", hz), 1);
        assert_eq!(cycles_of("ADC.start()", hz), 6);
        assert_eq!(cycles_of("v = ADC.result()", hz), 6);
    }
    // read = select + adquisicion de 11,5 us (AD132 tipico) + go + espera de 11 TAD + result
    assert_eq!(cycles_of("v = ADC.read(AN0)", 1_000_000), 30);
    assert_eq!(cycles_of("v = ADC.read(AN0)", 4_000_000), 57);
    assert_eq!(cycles_of("v = ADC.read(AN0)", 8_000_000), 134);
    assert_eq!(cycles_of("v = ADC.read(AN0)", 20_000_000), 169);
}

// ---- el contrato electrico (revision: las condiciones no son universales) -------------------------------------

#[test]
fn the_default_acquisition_is_the_typical_datasheet_value_not_a_universal_minimum() {
    // AD132: 11,5 us tipicos. A 4 MHz (TCY = 1 us) son 12 ciclos; a 20 MHz (TCY = 200 ns), 58
    let timing = |hz: u32| {
        let src = format!("{}v: u16 = 0\ndef main() -> None:\n    global v\n    ADC.start()\n    v = ADC.read(AN0)\n", head(hz));
        build(&src).1.timing.iter().map(|t| t.cycles).collect::<Vec<_>>()
    };
    assert_eq!(timing(4_000_000), vec![12]);
    assert_eq!(timing(20_000_000), vec![58]);
}

#[test]
fn the_acquisition_can_be_set_for_a_high_impedance_source() {
    let src = format!("{}v: u16 = 0\ndef main() -> None:\n    global v\n    ADC.start(acquisition_us=40)\n    v = ADC.read(AN0)\n", head(4_000_000));
    let (_, c) = build(&src);
    assert_eq!(c.timing.iter().map(|t| t.cycles).collect::<Vec<_>>(), vec![40]);
    // el simulador vigila lo que el programa declaro: con 40 us y 40 pedidos no hay violacion
    let mut s = Sim::new(&c.image);
    s.adc_tacq_ns = 40_000.0;
    s.adc_inputs[0] = 5;
    s.run_until(100_000, |s| s.halted);
    assert!(s.adc_violations.is_empty(), "{:?}", s.adc_violations);
}

#[test]
fn an_acquisition_below_the_amplifier_settling_time_warns() {
    let src = format!("{}v: u16 = 0\ndef main() -> None:\n    global v\n    ADC.start(acquisition_us=3)\n    v = ADC.read(AN0)\n", head(4_000_000));
    let (ok, t) = diagnostics(&src);
    assert!(ok && t.contains("PYP3013") && t.contains("TAMP") && t.contains("11,5"), "{t}");
    for bad in ["0", "2000", "True", "x"] {
        let (ok, t) = diagnostics(&format!("{}x: u8 = 3\ndef main() -> None:\n    ADC.start(acquisition_us={bad})\n", head(4_000_000)));
        assert!(!ok && t.contains("PYP2172"), "acquisition_us={bad}: {t}");
    }
}

#[test]
fn tad_is_judged_against_the_electrical_table_not_the_recommended_window() {
    // FOSC/32 a 4 MHz son 8 us: fuera de la ventana recomendada (1,6-6) pero dentro de AD130 (1,6-9): sin aviso
    let src = format!("{}v: u16 = 0\ndef main() -> None:\n    global v\n    ADC.start(clock=FOSC_32)\n    v = ADC.read(AN0)\n", head(4_000_000));
    let (ok, t) = diagnostics(&src);
    assert!(ok && !t.contains("PYP3012"), "{t}");
    let (_, s) = differential(&src, &[(0, 5)], 4_000_000);
    assert!(s.adc_violations.is_empty(), "{:?}", s.adc_violations);
    // 10 us (FOSC/32 a 3,2 MHz) pasa de 9 us: aviso y violacion
    let slow = format!("{}v: u16 = 0\ndef main() -> None:\n    global v\n    ADC.start(clock=FOSC_32)\n    v = ADC.read(AN0)\n", head(3_200_000));
    let (ok, t) = diagnostics(&slow);
    assert!(ok && t.contains("PYP3012") && t.contains("9000"), "{t}");
    let (_, s) = differential(&slow, &[(0, 5)], 3_200_000);
    assert!(s.adc_violations.iter().any(|v| v.contains("TAD")), "{:?}", s.adc_violations);
}

#[test]
fn the_automatic_clock_prefers_the_recommended_window_then_the_electrical_limits_then_the_rc() {
    let adcs = |hz: u32| {
        let src = format!("{}v: u16 = 0\ndef main() -> None:\n    global v\n    ADC.start()\n    v = ADC.read(AN0)\n", head(hz));
        let (_, c) = build(&src);
        let mut s = Sim::new(&c.image);
        s.fosc_hz = hz;
        s.run_until(5_000_000, |s| s.halted);
        s.read(0x1F) >> 6
    };
    // 250 kHz: FOSC/2 son 8 us, fuera de lo recomendado pero valido: se usa antes que el RC
    assert_eq!(adcs(250_000), 0);
    // 100 kHz: FOSC/2 son 20 us (> 9): ninguno vale, RC interno
    assert_eq!(adcs(100_000), 3);
}

#[test]
fn low_voltage_moves_the_tad_window_to_3_9_us() {
    let run = |hz: u32, lv: bool| {
        let src = format!("{}v: u16 = 0\ndef main() -> None:\n    global v\n    ADC.start(low_voltage={})\n    v = ADC.read(AN0)\n", head(hz), if lv { "True" } else { "False" });
        let (_, c) = build(&src);
        let mut s = Sim::new(&c.image);
        s.fosc_hz = hz;
        s.adc_low_voltage = lv;
        s.adc_inputs[0] = 7;
        s.run_until(1_000_000, |s| s.halted);
        (s.read(0x1F) >> 6, s.adc_violations.clone())
    };
    // 4 MHz: con VDD > 3 V es FOSC/8 (2 us); con VDD < 3 V el minimo es 3 us: FOSC/32 (8 us)
    assert_eq!(run(4_000_000, false), (1, vec![]));
    assert_eq!(run(4_000_000, true), (2, vec![]));
    // 1 MHz: FOSC/2 (2 us) vale con VDD alta y no con VDD baja (FOSC/8 = 8 us)
    assert_eq!(run(1_000_000, false), (0, vec![]));
    assert_eq!(run(1_000_000, true), (1, vec![]));
}

#[test]
fn every_start_call_must_ask_for_the_same_conditions() {
    let src = format!("{}def a() -> None:\n    ADC.start(acquisition_us=20)\n\n\ndef main() -> None:\n    ADC.start()\n    a()\n", head(4_000_000));
    let (ok, t) = diagnostics(&src);
    assert!(!ok && t.contains("PYP2176"), "{t}");
    // varias llamadas iguales (con distinto clock) son correctas
    let ok_src = format!("{}def a() -> None:\n    ADC.start(clock=FOSC_8)\n\n\ndef main() -> None:\n    ADC.start()\n    a()\n", head(4_000_000));
    assert!(diagnostics(&ok_src).0);
}

// ---- ADC.start() debe poder ejecutarse (no solo existir) ---------------------------------------------------------

#[test]
fn a_start_in_a_function_nobody_calls_does_not_count() {
    let src = format!("{}v: u16 = 0\ndef never() -> None:\n    ADC.start()\n\n\ndef main() -> None:\n    global v\n    v = ADC.read(AN0)\n", head(4_000_000));
    let (ok, t) = diagnostics(&src);
    assert!(!ok && t.contains("PYP2171") && t.contains("ni main ni @init_device"), "{t}");
}

#[test]
fn a_start_reachable_from_main_or_init_device_counts() {
    let via_call = format!("{}v: u16 = 0\ndef setup() -> None:\n    ADC.start()\n\n\ndef main() -> None:\n    global v\n    setup()\n    v = ADC.read(AN0)\n", head(4_000_000));
    assert!(diagnostics(&via_call).0);
    let via_init = format!("{}v: u16 = 0\n\n\n@init_device\ndef setup() -> None:\n    ADC.start()\n\n\ndef main() -> None:\n    global v\n    v = ADC.read(AN0)\n", head(4_000_000));
    let (ok, t) = diagnostics(&via_init);
    assert!(ok, "{t}");
}

#[test]
fn the_limit_of_the_start_check_is_stated_and_the_simulator_catches_the_order() {
    // el compilador comprueba que exista un start() alcanzable, no el orden: aqui main lee ANTES de iniciar
    let src = format!("{}v: u16 = 0\ndef main() -> None:\n    global v\n    v = ADC.read(AN0)\n    ADC.start()\n", head(4_000_000));
    let (ok, t) = diagnostics(&src);
    assert!(ok, "{t}"); // el compilador no puede verlo
                        // sin ADON la conversion nunca termina (como en el chip): el programa se queda sondeando GO y no llega a parar
    let (_, c) = build(&src);
    let mut s = Sim::new(&c.image);
    s.adc_inputs[0] = 5;
    s.run_until(200_000, |_| false);
    assert!(!s.halted, "debe quedarse esperando una conversion que no ocurre");
    assert!(s.adc_violations.iter().any(|v| v.contains("ADON")), "el simulador debe verlo: {:?}", s.adc_violations);
    let (_, t) = diagnostics(&format!("{}def main() -> None:\n    x: u16 = ADC.read(AN0)\n", head(4_000_000)));
    assert!(t.contains("se comprueba que exista, no el orden"), "{t}");
}
