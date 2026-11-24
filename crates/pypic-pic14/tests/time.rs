//! Esperas de `pypic.system.time`: la duracion se mide en ciclos reales del simulador entre dos cambios de nivel de RD0.
//!
//! `RD0.on(); <espera>; RD0.off()`: el evento de subida ocurre al terminar `BSF`; el de bajada, al terminar `BCF`.
//! Por eso el intervalo entre ambos es `ciclos de la espera + 1`.

use std::path::PathBuf;

use pypic_device::DevicePack;
use pypic_frontend::{analyze, TargetProfile};
use pypic_ir::{lower, verify, Machine, Module};
use pypic_pic14::codegen::{CodegenError, Compiled, Options};
use pypic_pic14::compile;
use pypic_pic14::sim::Sim;

fn pack_target() -> TargetProfile {
    DevicePack::load(&PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../devices/pic16f887")).unwrap().target_profile()
}

fn build(src: &str) -> (Module, Compiled) {
    let r = analyze(src, &pack_target());
    let p = r.program.unwrap_or_else(|| panic!("frontend:\n{}\n{src}", r.diagnostics.render(src, "t.py")));
    let m = lower(&p);
    verify(&m).unwrap_or_else(|e| panic!("IR invalida: {e:?}"));
    let c = compile(&m, &Options::default()).unwrap_or_else(|e| panic!("codegen: {e}\n{src}"));
    (m, c)
}

/// Programa de medida: sube RD0, ejecuta `body`, baja RD0.
fn program(clock: u32, imports: &str, globals: &str, body: &str) -> String {
    format!(
        "from pypic.device import PIC16F887\nfrom pypic.periph.gpio import RD0\nfrom pypic.system.time import {imports}\n\
         CLOCK_HZ: Const[u32] = {clock}\n{globals}\n\
         def main() -> None:\n    RD0.output()\n    RD0.on()\n    {body}\n    RD0.off()\n"
    )
}

/// Ciclos de la espera (sin el BCF final) medidos en el simulador.
fn measured(src: &str) -> u64 {
    let (_, c) = build(src);
    let mut s = Sim::new(&c.image);
    let (mut last, mut up, mut down): (Option<bool>, Option<u64>, Option<u64>) = (None, None, None);
    while !s.halted && s.cycles < 400_000_000 {
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
    down.expect("RD0 no bajo") - up.expect("RD0 no subio") - 1
}

fn delay_cycles(n: u64) -> u64 {
    measured(&program(4_000_000, "delay_cycles", "", &format!("delay_cycles({n})")))
}

// ---- delay_cycles: exacto para CUALQUIER cantidad -----------------------------------------------------

#[test]
fn delay_cycles_is_exact_for_every_amount_up_to_400() {
    for n in 0..=400 {
        assert_eq!(delay_cycles(n), n, "delay_cycles({n})");
    }
}

#[test]
fn delay_cycles_is_exact_at_every_structural_boundary() {
    // umbrales entre 1, 2 y 3 contadores, y entre bloques de 50 millones
    let mut values: Vec<u64> = vec![767, 768, 769, 770, 771, 772, 773, 774, 775, 776, 777, 778, 779, 780, 1535, 1536, 1537];
    values.extend(197_110..=197_140);
    values.extend(197_880..=197_910);
    values.extend([394_240, 394_241, 394_242, 591_362, 1_000_000, 4_999_999, 12_345_678, 50_000_000, 50_000_001]);
    for n in values {
        assert_eq!(delay_cycles(n), n, "delay_cycles({n})");
    }
}

#[test]
fn delay_cycles_is_exact_for_pseudorandom_amounts() {
    let mut x: u64 = 0x9E3779B97F4A7C15;
    for _ in 0..120 {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        let n = x % 3_000_000;
        assert_eq!(delay_cycles(n), n, "delay_cycles({n})");
    }
}

#[test]
fn delay_cycles_beyond_one_block_is_still_exact() {
    assert_eq!(delay_cycles(100_000_003), 100_000_003);
}

// ---- tiempo real -> ciclos, a varios relojes -----------------------------------------------------------

#[test]
fn time_units_become_exact_cycle_counts_at_several_clocks() {
    // (reloj, funcion, llamada, ciclos esperados = tiempo * reloj / 4)
    let cases: &[(u32, &str, &str, u64)] = &[
        (4_000_000, "delay_us", "delay_us(100)", 100),
        (4_000_000, "pause", "pause(500)", 500_000),
        (4_000_000, "wait", "wait(1)", 1_000_000),
        (8_000_000, "delay_us", "delay_us(100)", 200),
        (8_000_000, "pause", "pause(10)", 20_000),
        (8_000_000, "wait", "wait(1)", 2_000_000),
        (20_000_000, "delay_us", "delay_us(7)", 35),
        (20_000_000, "pause", "pause(3)", 15_000),
        (16_000_000, "delay_us", "delay_us(1)", 4),
        (32_768, "pause", "pause(1000)", 8192),
        (32_768, "wait", "wait(1)", 8192),
    ];
    for &(hz, import, call, cycles) in cases {
        assert_eq!(measured(&program(hz, import, "", call)), cycles, "{call} a {hz} Hz");
    }
}

#[test]
fn a_time_that_is_not_a_whole_number_of_cycles_rounds_to_the_nearest_cycle() {
    // 32.768 kHz: un ciclo dura 122.07 us; 1000 us = 8.19 ciclos -> 8
    assert_eq!(measured(&program(32_768, "delay_us", "", "delay_us(1000)")), 8);
    // 1 us a 32.768 kHz = 0.008 ciclos -> 0 ciclos (el compilador lo avisa)
    assert_eq!(measured(&program(32_768, "delay_us", "", "delay_us(1)")), 0);
}

// ---- cantidad variable (u16) ---------------------------------------------------------------------------

#[test]
fn variable_pause_is_n_times_one_millisecond_plus_8_fixed_cycles() {
    for n in [1u64, 2, 3, 7, 255, 256, 257, 300, 1000] {
        let src = program(4_000_000, "pause", &format!("n: u16 = {n}"), "pause(n)");
        assert_eq!(measured(&src), 8 + n * 1000, "pause({n}) variable");
    }
}

#[test]
fn variable_pause_with_zero_does_not_wait() {
    assert_eq!(measured(&program(4_000_000, "pause", "n: u16 = 0", "pause(n)")), 9);
}

#[test]
fn variable_wait_counts_whole_seconds() {
    // 20 MHz: 1 s = 5 000 000 ciclos
    assert_eq!(measured(&program(20_000_000, "wait", "n: u16 = 2", "wait(n)")), 8 + 2 * 5_000_000);
}

#[test]
fn variable_waits_on_a_clock_too_slow_for_the_loop_are_refused() {
    let src = program(32_768, "pause", "n: u16 = 5", "pause(n)"); // 1 ms = 8 ciclos < 9
    let r = analyze(&src, &pack_target());
    let m = lower(&r.program.unwrap());
    let e = compile(&m, &Options::default()).unwrap_err();
    assert!(matches!(&e, CodegenError::Unsupported(s) if s.contains("al menos 9") && s.contains("8 ciclos")), "{e}");
}

// ---- informe de tiempos ---------------------------------------------------------------------------------

#[test]
fn timing_report_shows_requested_cycles_and_error() {
    let (_, c) = build(&program(4_000_000, "pause", "", "pause(500)"));
    let r = &c.timing[0];
    assert_eq!((r.function.as_str(), r.requested, r.cycles, r.fixed_cycles), ("main", Some(500), 500_000, 0));
    assert!((r.achieved_ns() - 5e8).abs() < 1e-6 && (r.achieved_ns() - r.requested_ns().unwrap()).abs() < 1e-6);

    // 32.768 kHz: 1000 us pedidos -> 8 ciclos de 122070.3 ns = 976562.5 ns (error -23437.5 ns)
    let (_, c) = build(&program(32_768, "delay_us", "", "delay_us(1000)"));
    let r = &c.timing[0];
    assert_eq!(r.cycles, 8);
    assert!((r.achieved_ns() - 976_562.5).abs() < 0.01, "{}", r.achieved_ns());
    assert!((r.achieved_ns() - r.requested_ns().unwrap() + 23_437.5).abs() < 0.01);

    let (_, c) = build(&program(4_000_000, "pause", "n: u16 = 3", "pause(n)"));
    let r = &c.timing[0];
    assert_eq!((r.requested, r.cycles, r.fixed_cycles), (None, 1000, 8));
}

// ---- oraculo de IR, memoria y contexto ---------------------------------------------------------------------

#[test]
fn the_ir_oracle_records_the_waits_in_order() {
    let src = program(4_000_000, "pause, delay_us, wait", "", "pause(5)\n    delay_us(20)\n    wait(1)");
    let (m, _) = build(&src);
    let mut mach = Machine::new(&m);
    mach.run_main().unwrap();
    use pypic_ir::ir::DelayUnit::*;
    assert_eq!(mach.delays, vec![(Ms, 5), (Us, 20), (S, 1)]);
}

#[test]
fn delay_counters_take_ram_only_when_the_program_waits() {
    let with = build(&program(4_000_000, "pause", "", "pause(1)")).1.ram_bytes;
    let without = build("from pypic.device import PIC16F887\ndef main() -> None:\n    pass\n").1.ram_bytes;
    assert_eq!((without, with), (0, 5));
}

#[test]
fn waits_inside_helper_functions_work() {
    let src = "from pypic.device import PIC16F887\nfrom pypic.periph.gpio import RD0\nfrom pypic.system.time import pause\nCLOCK_HZ: Const[u32] = 4_000_000\n\
               def blink_once() -> None:\n    RD0.on()\n    pause(2)\n    RD0.off()\n\
               def main() -> None:\n    RD0.output()\n    blink_once()\n    blink_once()\n";
    assert_eq!(measured(src), 2000); // la primera espera: 2 ms exactos dentro de la funcion
}
