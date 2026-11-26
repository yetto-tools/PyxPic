//! PWM de los modulos CCP (`pypic.periph.pwm`, #45): periodos exactos medidos en ciclos del simulador, el doble bufer del ciclo
//! de trabajo, el nivel del pin, el arranque de DS40001291H 11.5.7, las reglas del lenguaje y los costos.

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

fn head(clock: u32) -> String {
    format!("from pypic.device import PIC16F887\nfrom pypic.periph.pwm import PWM1, PWM2\nCLOCK_HZ: Const[uint32] = {clock}\n")
}

fn diagnostics(src: &str) -> (bool, String) {
    let r = analyze(src, &pack().target_profile());
    (r.program.is_some(), r.diagnostics.render(src, "t.py"))
}

fn build(src: &str) -> (pypic_ir::Module, Compiled) {
    let r = analyze(src, &pack().target_profile());
    let p = r.program.unwrap_or_else(|| panic!("frontend:\n{}\n{src}", r.diagnostics.render(src, "t.py")));
    let m = lower(&p);
    verify(&m).unwrap_or_else(|e| panic!("IR invalida: {e:?}"));
    let c = compile(&m, &Options::default()).unwrap_or_else(|e| panic!("codegen: {e}\n{src}"));
    (m, c)
}

fn run(src: &str, cycles: u64) -> Sim {
    let (_, c) = build(src);
    let mut s = Sim::new(&c.image);
    s.run_until(cycles, |_| false);
    s
}

const CCP1CON: usize = 0x17;
const CCPR1L: usize = 0x15;
const CCP2CON: usize = 0x1D;
const CCPR2L: usize = 0x1B;
const T2CON: usize = 0x12;
const PR2: usize = 0x92;
const TRISC: usize = 0x87;

fn duty10(s: &Sim, con: usize, rl: usize) -> u16 {
    (u16::from(s.read(rl)) << 2) | u16::from((s.read(con) >> 4) & 3)
}

/// Ciclos entre inicios de periodo consecutivos de un modulo.
fn periods(s: &Sim, ccp: u8) -> Vec<u64> {
    let t: Vec<u64> = s.pwm_events.iter().filter(|e| e.ccp == ccp).map(|e| e.cycle).collect();
    t.windows(2).map(|w| w[1] - w[0]).collect()
}

fn prog(clock: u32, body: &str) -> String {
    format!("{}\n\ndef main() -> None:\n{body}    while True:\n        pass\n", head(clock))
}

// ---- el arranque deja los registros como el datasheet ---------------------------------------------------------------------

#[test]
fn start_follows_the_datasheet_setup_and_registers_match() {
    let s = run(&prog(4_000_000, "    PWM1.start(frequency=1000, duty=250)\n"), 6_000);
    assert_eq!(s.read(PR2), 249, "PR2 = (periodo / prescaler) - 1");
    assert_eq!(s.read(T2CON), 0x05, "TMR2ON y prescaler 1:4, postscaler 1:1");
    assert_eq!(s.read(CCP1CON), 0x0C | (250 & 3) << 4, "modo PWM de una salida con DC1B");
    assert_eq!(s.read(CCPR1L), 250 >> 2);
    assert_eq!(s.read(TRISC) & 0x04, 0, "RC2 es salida tras el arranque");
    assert!(periods(&s, 1).iter().all(|&p| p == 1000), "{:?}", periods(&s, 1));
    assert!(s.pwm_events.iter().all(|e| e.duty == 250));
}

#[test]
fn the_pin_is_an_input_until_the_first_period_starts() {
    // 11.5.7 paso 6: la salida se habilita despues del primer desbordamiento de Timer2, para no sacar un primer pulso incompleto
    let (_, c) = build(&prog(4_000_000, "    PWM1.start(frequency=1000, duty=500)\n"));
    let mut s = Sim::new(&c.image);
    let mut enabled_at = None;
    while s.cycles < 4_000 {
        s.step();
        if enabled_at.is_none() && s.read(TRISC) & 0x04 == 0 && s.read(CCP1CON) & 0x0C == 0x0C {
            enabled_at = Some(s.cycles);
        }
    }
    let first = s.pwm_events[0].cycle;
    let t = enabled_at.expect("el pin se habilita");
    assert!(t >= first, "la salida se habilita ({t}) despues del primer inicio de periodo ({first})");
}

// ---- periodos exactos: las tablas 11-3 y 11-4 del datasheet ---------------------------------------------------------------

#[test]
fn raw_timings_reproduce_the_datasheet_tables() {
    // TABLA 11-4 (FOSC = 8 MHz): prescaler, PR2 y frecuencia
    for (pre, pr2, hz) in [(16u32, 0x65u32, 1_225.0f64), (4, 0x65, 4_901.0), (1, 0x65, 19_608.0), (1, 0x19, 76_923.0), (1, 0x0C, 153_846.0), (1, 0x09, 200_000.0)] {
        let src = prog(8_000_000, &format!("    PWM1.start(prescaler={pre}, period={pr2}, duty=1)\n"));
        let cycles = u64::from((pr2 + 1) * pre);
        let s = run(&src, cycles * 6 + 400);
        assert!(periods(&s, 1).len() >= 3, "{pre} {pr2}: {:?}", periods(&s, 1));
        assert!(periods(&s, 1).iter().all(|&p| p == cycles), "{pre} {pr2}: {:?}", periods(&s, 1));
        let f = 8_000_000.0 / (4.0 * cycles as f64);
        assert!((f - hz).abs() / hz < 0.01, "{f} Hz contra {hz} Hz de la tabla");
    }
    // TABLA 11-3 (FOSC = 20 MHz), 1,22 kHz con prescaler 16 y PR2 = 0xFF
    let s = run(&prog(20_000_000, "    PWM1.start(prescaler=16, period=255)\n"), 4096 * 5);
    assert!(periods(&s, 1).iter().all(|&p| p == 4096));
}

#[test]
fn frequency_resolves_to_the_smallest_prescaler_with_an_exact_period() {
    for (clock, hz, pre, pr2) in [(4_000_000u32, 1000u32, 4u8, 249u8), (4_000_000, 20_000, 1, 49), (4_000_000, 5_000, 1, 199), (20_000_000, 10_000, 4, 124), (8_000_000, 2_000, 4, 249)] {
        let s = run(&prog(clock, &format!("    PWM1.start(frequency={hz})\n")), 8 * u64::from(clock) / 4 / u64::from(hz));
        assert_eq!((s.read(PR2), s.read(T2CON) & 3), (pr2, [1u8, 4, 16].iter().position(|&p| p == pre).unwrap() as u8), "{clock} Hz, {hz} Hz");
        assert!(periods(&s, 1).iter().all(|&p| p == u64::from(clock) / 4 / u64::from(hz)), "{hz}: {:?}", periods(&s, 1));
    }
}

#[test]
fn a_frequency_that_is_not_exact_is_an_error_with_the_nearest_ones() {
    let (ok, t) = diagnostics(&prog(4_000_000, "    PWM1.start(frequency=3000)\n"));
    assert!(!ok && t.contains("PYP2207") && t.contains("no cae en un periodo exacto") && t.contains("mas cercanas"), "{t}");
    // fuera de rango: mas de 4096 ciclos (el prescaler 16 y PR2 = 255 son el limite)
    let (ok, t) = diagnostics(&prog(4_000_000, "    PWM1.start(frequency=100)\n"));
    assert!(!ok && t.contains("PYP2207") && t.contains("mas cercana"), "{t}");
}

// ---- el ciclo de trabajo: doble bufer, 10 bits y el nivel del pin --------------------------------------------------------

#[test]
fn duty_registers_and_the_double_buffer() {
    // el valor entra a los registros al llamar y pasa al esclavo (CCPRxH) solo al terminar el periodo
    let (_, c) = build(&prog(4_000_000, "    PWM1.start(frequency=1000, duty=100)\n    n: uint8 = 0\n    while n < 200:\n        n += 1\n    PWM1.duty(515)\n"));
    let mut s = Sim::new(&c.image);
    s.run_until(8_000, |_| false);
    assert_eq!(duty10(&s, CCP1CON, CCPR1L), 515);
    let last = s.pwm_events.last().unwrap();
    assert_eq!(last.duty, 515, "el ultimo periodo ya engancho el valor nuevo");
    let before: Vec<_> = s.pwm_events.iter().map(|e| e.duty).collect();
    assert_eq!(before.first(), Some(&100), "{before:?}");
    assert!(before.windows(2).all(|w| w[0] == 100 && w[1] == 100 || w[0] == 100 && w[1] == 515 || w[0] == 515 && w[1] == 515), "solo hay un cambio, a un inicio de periodo: {before:?}");
}

#[test]
fn the_pin_is_high_for_exactly_the_duty_counts() {
    // 4 MHz, 1 kHz: prescaler 4 -> una cuenta por ciclo de instruccion: 250 cuentas = 250 ciclos en alto de 1000
    for duty in [0u16, 1, 250, 500, 999, 1000] {
        let (_, c) = build(&prog(4_000_000, &format!("    PWM1.start(frequency=1000, duty={duty})\n")));
        let mut s = Sim::new(&c.image);
        let mut samples: Vec<(u64, bool)> = Vec::new();
        while s.cycles < 6_500 {
            let before = s.cycles;
            s.step();
            samples.push((before, s.read(0x07) & 0x04 != 0));
        }
        // cuatro periodos completos y estables, desde el segundo inicio de periodo
        let starts: Vec<u64> = s.pwm_events.iter().map(|e| e.cycle).collect();
        let (a, b) = (starts[1], starts[5]);
        let high: u64 = samples.windows(2).filter(|w| w[0].0 >= a && w[1].0 <= b && w[0].1).map(|w| w[1].0 - w[0].0).sum();
        let expect = u64::from(duty.min(1000)) * 4;
        assert!(high.abs_diff(expect) <= 4, "duty {duty}: {high} ciclos en alto en 4 periodos, se esperaban {expect}");
    }
}

#[test]
fn duty_zero_never_rises_and_above_the_period_stays_high() {
    for (duty, level) in [(0u16, false), (1023, true)] {
        let (_, c) = build(&prog(4_000_000, &format!("    PWM1.start(frequency=20000, duty={duty})\n")));
        let mut s = Sim::new(&c.image);
        s.run_until(2_000, |_| false);
        let mut seen = std::collections::BTreeSet::new();
        for _ in 0..400 {
            s.step();
            seen.insert(s.read(0x07) & 0x04 != 0);
        }
        assert_eq!(seen.into_iter().collect::<Vec<_>>(), vec![level], "duty {duty}");
    }
}

// ---- el interprete de referencia y el simulador coinciden en lo que se escribe --------------------------------------------

#[test]
fn variable_duty_matches_the_reference_interpreter() {
    for v in [0u16, 1, 2, 3, 4, 5, 127, 255, 256, 257, 511, 512, 515, 1000, 1023, 1024, 1027, 4095, 65535] {
        // globales: el optimizador no las propaga, asi que `duty(g)` recorre la ruta de un valor variable
        let src = format!("{}g: uint16 = 0\nh: uint16 = 0\n\n\ndef main() -> None:\n    global g, h\n    PWM1.start(frequency=1000)\n    g = {v}\n    PWM1.duty(g)\n    PWM2.start(frequency=1000)\n    h = {}\n    PWM2.duty(h)\n    while True:\n        pass\n", head(4_000_000), v / 2);
        let (m, c) = build(&src);
        let mut mach = Machine::new(&m);
        let _ = mach.run_main();
        let mut s = Sim::new(&c.image);
        s.run_until(8_000, |_| false);
        assert_eq!(mach.pwm_duty[0], v & 0x3FF, "interprete: {v}");
        assert_eq!(mach.pwm_duty[1], (v / 2) & 0x3FF, "interprete: {v}/2");
        assert_eq!(duty10(&s, CCP1CON, CCPR1L), v & 0x3FF, "simulador PWM1: {v}");
        assert_eq!(duty10(&s, CCP2CON, CCPR2L), (v / 2) & 0x3FF, "simulador PWM2: {v}/2");
        assert_eq!(s.read(CCP1CON) & 0xCF, 0x0C, "duty no toca el modo");
        assert_eq!(s.read(CCP2CON) & 0xCF, 0x0C);
    }
}

#[test]
fn constant_duty_gives_the_same_registers_as_a_variable() {
    for v in [0u16, 3, 250, 513, 1023] {
        let a = run(&prog(4_000_000, &format!("    PWM1.start(frequency=1000)\n    PWM1.duty({v})\n")), 3_000);
        assert_eq!(duty10(&a, CCP1CON, CCPR1L), v, "constante {v}");
    }
}

// ---- dos modulos comparten Timer2 --------------------------------------------------------------------------------------------

#[test]
fn two_modules_share_timer2_without_disturbing_each_others_period() {
    let src = prog(4_000_000, "    PWM1.start(frequency=1000, duty=200)\n    PWM2.start(frequency=1000, duty=700)\n");
    let s = run(&src, 9_000);
    assert_eq!(s.read(T2CON), 0x05);
    assert_eq!(duty10(&s, CCP1CON, CCPR1L), 200);
    assert_eq!(duty10(&s, CCP2CON, CCPR2L), 700);
    assert_eq!(s.read(TRISC) & 0x06, 0, "RC1 y RC2 son salidas");
    // el segundo start no reinicia el prescaler de Timer2: el periodo de PWM1 se mantiene
    let p = periods(&s, 1);
    assert!(p.len() >= 5 && p.iter().all(|&c| c == 1000), "{p:?}");
    let last1 = s.pwm_events.iter().rev().find(|e| e.ccp == 1).unwrap().cycle;
    let last2 = s.pwm_events.iter().rev().find(|e| e.ccp == 2).unwrap().cycle;
    assert_eq!(last1, last2, "los dos modulos empiezan el periodo en el mismo ciclo (mismo Timer2)");
}

#[test]
fn stop_clears_the_module_and_duty_does_not_turn_it_on_again() {
    let s = run(&prog(4_000_000, "    PWM1.start(frequency=1000, duty=100)\n    PWM1.stop()\n"), 4_000);
    assert_eq!(s.read(CCP1CON), 0);
    let s = run(&prog(4_000_000, "    PWM1.start(frequency=1000, duty=100)\n    PWM1.stop()\n    PWM1.duty(700)\n"), 4_000);
    assert_eq!(s.read(CCP1CON) & 0x0C, 0, "duty despues de stop no vuelve a poner el modo PWM");
}

// ---- las reglas del lenguaje ----------------------------------------------------------------------------------------------

#[test]
fn language_rules_are_located_errors() {
    let h = head(4_000_000);
    let wrap = |body: &str| format!("{h}\n\ndef main() -> None:\n{body}    pass\n");
    for (src, code, frag) in [
        (wrap("    PWM1.duty(10)\n"), "PYP2204", "nunca se llama a PWM1.start()"),
        (wrap("    PWM1.start(frequency=1000)\n    PWM2.duty(10)\n"), "PYP2204", "PWM2"),
        (wrap("    PWM1.start()\n"), "PYP2201", "necesita el periodo"),
        (wrap("    PWM1.start(1000)\n"), "PYP2201", "argumentos con nombre"),
        (wrap("    PWM1.start(frequency=1000, prescaler=4, period=249)\n"), "PYP2201", "una sola forma"),
        (wrap("    PWM1.start(prescaler=4)\n"), "PYP2201", "van juntos"),
        (wrap("    PWM1.start(prescaler=2, period=10)\n"), "PYP2202", "1, 4 o 16"),
        (wrap("    PWM1.start(prescaler=4, period=300)\n"), "PYP2202", "0 a 255"),
        (wrap("    PWM1.start(frequency=1000, duty=2000)\n"), "PYP2202", "0 a 1023"),
        (wrap("    PWM1.start(frequency=1000, voltaje=3)\n"), "PYP2201", "no tiene el argumento"),
        (wrap("    PWM1.start(frequency=1000)\n    PWM1.duty(1024)\n"), "PYP2202", "10 bits"),
        (wrap("    PWM1.start(frequency=1000)\n    PWM1.apagar()\n"), "PYP2203", "disponibles"),
        (wrap("    PWM1.start(frequency=1000)\n    x: uint8 = 0\n    PWM1.duty(x)\n"), "PYP2041", "uint16"),
        (wrap("    PWM1.start(frequency=1000)\n    x = PWM1\n"), "PYP2205", "modulo PWM"),
        (wrap("    PWM1.start(frequency=1000)\n    PWM2.start(frequency=2000)\n"), "PYP2208", "un solo periodo"),
        (wrap("    PWM1.start(frequency=1000)\n    PWM2.start(prescaler=1, period=99)\n"), "PYP2208", "comparten Timer2"),
    ] {
        let (ok, t) = diagnostics(&src);
        assert!(!ok && t.contains(code) && t.contains(frag), "{src}\n{t}");
    }
    let (ok, t) = diagnostics("from pypic.device import PIC16F887\nfrom pypic.periph.pwm import PWM3\n\ndef main() -> None:\n    pass\n");
    assert!(!ok && t.contains("PYP2200") && t.contains("PWM1, PWM2"), "{t}");
}

#[test]
fn the_pwm_pin_conflicts_with_gpio_and_timer2_is_the_pwms() {
    let (ok, t) = diagnostics(&format!("{}from pypic.periph.gpio import RC2\n\n\ndef main() -> None:\n    PWM1.start(frequency=1000)\n", head(4_000_000)));
    assert!(!ok && t.contains("PYP2150") && t.contains("RC2") && t.contains("CCP1"), "{t}");
    let (ok, t) = diagnostics(&format!("{}from pypic.periph.gpio import RC1\n\n\ndef main() -> None:\n    PWM2.start(frequency=1000)\n", head(4_000_000)));
    assert!(!ok && t.contains("PYP2150") && t.contains("RC1"), "{t}");
    let src = format!("{}from pypic.periph.timer import TMR2\n\n\ndef main() -> None:\n    PWM1.start(frequency=1000)\n    TMR2.every_ms(1)\n", head(4_000_000));
    let (ok, t) = diagnostics(&src);
    assert!(!ok && t.contains("PYP2206") && t.contains("Timer2"), "{t}");
    // RC1 libre si solo se usa PWM1
    let (ok, t) = diagnostics(&format!("{}from pypic.periph.gpio import RC1\n\n\ndef main() -> None:\n    RC1.output()\n    PWM1.start(frequency=1000)\n", head(4_000_000)));
    assert!(ok, "{t}");
}

#[test]
fn a_duty_above_the_period_warns() {
    // 20 kHz a 4 MHz: 50 ciclos = 200 cuentas; 500 supera el periodo y la salida queda en alto
    let (ok, t) = diagnostics(&prog(4_000_000, "    PWM1.start(frequency=20000, duty=500)\n"));
    assert!(ok && t.contains("PYP3015") && t.contains("100 %"), "{t}");
    let (ok, t) = diagnostics(&prog(4_000_000, "    PWM1.start(frequency=20000, duty=200)\n"));
    assert!(ok && !t.contains("PYP3015"), "{t}");
}

#[test]
fn a_device_without_the_module_rejects_the_import() {
    // el modulo existe solo si el dispositivo lo trae (peripherals.json)
    let mut target = pack().target_profile();
    target.peripherals = Some(["ADC".to_string()].into_iter().collect());
    let r = analyze("from pypic.periph.pwm import PWM1\n\ndef main() -> None:\n    pass\n", &target);
    let t = r.diagnostics.render("", "t.py");
    assert!(r.program.is_none() && t.contains("PYP2200") && t.contains("no trae"), "{t}");
}

// ---- costos fijos (medidos en el simulador) -----------------------------------------------------------------------------------

/// Ciclos de las instrucciones entre dos marcas: el programa pone RD0 en alto antes y en bajo despues.
fn cycles_between(body: &str, init: u16) -> u64 {
    // `g` es una global: el optimizador no la propaga, asi que `duty(g)` es de verdad un valor variable
    let src = format!("{}from pypic.periph.gpio import RD0\n\ng: uint16 = {init}\n\n\ndef main() -> None:\n    RD0.output()\n    PWM1.start(frequency=1000)\n    RD0.on()\n{body}    RD0.off()\n    while True:\n        pass\n", head(4_000_000));
    let (_, c) = build(&src);
    let mut s = Sim::new(&c.image);
    let (mut on, mut off) = (None, None);
    while s.cycles < 6_000 && off.is_none() {
        s.step();
        let rd0 = s.read(0x08) & 1 != 0;
        if rd0 && on.is_none() {
            on = Some(s.cycles);
        }
        if !rd0 && on.is_some() && off.is_none() {
            off = Some(s.cycles);
        }
    }
    off.unwrap() - on.unwrap()
}

#[test]
fn duty_and_stop_costs_are_fixed() {
    let base = cycles_between("", 515);
    let cost = |body: &str, init: u16| cycles_between(body, init) - base;
    let c_const = cost("    PWM1.duty(515)\n", 515);
    let c_var = cost("    PWM1.duty(g)\n", 515);
    let c_stop = cost("    PWM1.stop()\n", 515);
    assert_eq!(c_stop, 1, "stop es un CLRF");
    assert_eq!(c_const, 6, "constante: MOVLW/MOVWF + MOVLW/ANDWF + MOVLW/IORWF");
    assert!(c_var > c_const && c_var <= 60, "variable: {c_var} ciclos");
    eprintln!("COSTOS pwm: constante {c_const}, variable {c_var}, stop {c_stop}");
    // el costo de la variable no depende del valor
    for v in [0u16, 3, 4, 255, 256, 1023] {
        assert_eq!(cost("    PWM1.duty(g)\n", v), c_var, "duty(g) con g = {v}");
    }
}

// ---- el ejemplo ------------------------------------------------------------------------------------------------------------------

#[test]
fn the_potentiometer_example_maps_one_adc_reading_to_one_duty() {
    let src = std::fs::read_to_string(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/python/pwm/pwm.py")).unwrap();
    let (_, c) = build(&src);
    for reading in [0u16, 1, 300, 512, 1023] {
        let mut s = Sim::new(&c.image);
        s.adc_inputs[0] = reading;
        s.run_until(30_000, |_| false);
        assert_eq!((s.read(PR2), s.read(T2CON) & 3), (255, 0), "prescaler 1 y PR2 = 255");
        assert_eq!(duty10(&s, CCP1CON, CCPR1L), reading, "una lectura del ADC es un ciclo de trabajo");
        assert!(periods(&s, 1).iter().all(|&p| p == 256), "{:?}", periods(&s, 1));
    }
}
