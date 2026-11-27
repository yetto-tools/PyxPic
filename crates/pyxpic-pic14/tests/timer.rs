//! Temporizadores de `pyxpic.periph.timer`: los periodos se miden en ciclos exactos del simulador
//! (`Sim::timer_events` guarda el ciclo en que cada bandera se activo).

use std::path::PathBuf;

use pyxpic_device::DevicePack;
use pyxpic_frontend::{analyze, TargetProfile};
use pyxpic_ir::{lower, verify};
use pyxpic_pic14::codegen::Options;
use pyxpic_pic14::compile;
use pyxpic_pic14::sim::Sim;

fn pack_target() -> TargetProfile {
    DevicePack::load(&PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../devices/pic16f887")).unwrap().target_profile()
}

fn run_c(src: &str, max_cycles: u64) -> (Sim, pyxpic_pic14::codegen::Compiled) {
    let r = analyze(src, &pack_target());
    let p = r.program.unwrap_or_else(|| panic!("frontend:\n{}\n{src}", r.diagnostics.render(src, "t.py")));
    let m = lower(&p);
    verify(&m).unwrap_or_else(|e| panic!("IR invalida: {e:?}"));
    let c = compile(&m, &Options::default()).unwrap_or_else(|e| panic!("codegen: {e}\n{src}"));
    let mut s = Sim::new(&c.image);
    s.run_until(max_cycles, |_| false);
    (s, c)
}

fn run(src: &str, max_cycles: u64) -> Sim {
    run_c(src, max_cycles).0
}

fn header(imports: &str, clock: u32) -> String {
    format!("from pyxpic.device import PIC16F887\nfrom pyxpic.periph.timer import {imports}\nCLOCK_HZ: Const[u32] = {clock}\n")
}

/// Espacios entre activaciones consecutivas de `flag`.
fn periods(s: &Sim, flag: &str) -> Vec<u64> {
    let t: Vec<u64> = s.timer_events.iter().filter(|e| e.0 == flag).map(|e| e.1).collect();
    t.windows(2).map(|w| w[1] - w[0]).collect()
}

/// Programa que arranca el temporizador y sondea/limpia la bandera para que se pueda volver a activar.
fn poll(imports: &str, clock: u32, start: &str, t: &str) -> String {
    format!("{}def main() -> None:\n    {start}\n    while True:\n        if {t}.expired():\n            {t}.clear()\n", header(imports, clock))
}

#[test]
fn timer2_every_ms_is_exact() {
    let s = run(&poll("TMR2", 4_000_000, "TMR2.every_ms(1)", "TMR2"), 20_000);
    let p = periods(&s, "TMR2IF");
    assert!(p.len() >= 15, "{p:?}");
    assert!(p.iter().all(|&c| c == 1000), "{p:?}");
}

#[test]
fn timer2_every_us_at_20mhz() {
    // 20 MHz -> 5 ciclos por microsegundo: 100 us = 500 ciclos
    let s = run(&poll("TMR2", 20_000_000, "TMR2.every_us(100)", "TMR2"), 10_000);
    assert!(periods(&s, "TMR2IF").iter().all(|&c| c == 500));
}

#[test]
fn timer2_start_prescaler_period_postscaler() {
    for (pre, per, post) in [(1, 249, 1), (4, 249, 1), (16, 99, 3), (16, 255, 16), (1, 0, 1), (4, 7, 5)] {
        let want = pre as u64 * (per as u64 + 1) * post as u64;
        let src = poll("TMR2", 4_000_000, &format!("TMR2.start(prescaler={pre}, period={per}, postscaler={post})"), "TMR2");
        let s = run(&src, want * 6 + 100);
        let p = periods(&s, "TMR2IF");
        assert!(p.len() >= 3 && p.iter().all(|&c| c == want), "pre={pre} per={per} post={post}: {p:?} (esperado {want})");
    }
}

#[test]
fn timer0_overflow_period_follows_prescaler() {
    for pre in [1u64, 2, 4, 8, 16, 32, 64, 128, 256] {
        let s = run(&poll("TMR0", 4_000_000, &format!("TMR0.start(prescaler={pre})"), "TMR0"), 256 * pre * 5 + 500);
        let p = periods(&s, "T0IF");
        assert!(p.len() >= 3 && p.iter().all(|&c| c == 256 * pre), "prescaler={pre}: {p:?}");
    }
}

#[test]
fn timer0_set_preloads_the_count() {
    // precarga 6: desborda a los 250 ciclos del preescalador 1:1 (preload = 256 - 250)
    let src =
        format!("{}def main() -> None:\n    TMR0.start(prescaler=1)\n    while True:\n        if TMR0.expired():\n            TMR0.clear()\n            TMR0.set(6)\n", header("TMR0", 4_000_000));
    let s = run(&src, 5_000);
    let p = periods(&s, "T0IF");
    assert!(p.len() >= 10, "{p:?}");
    // el primer periodo cuenta 256 desde cero; los demas, 250 mas la inhibicion de 2 ciclos y la latencia constante del sondeo (sin precarga serian 256 + latencia)
    assert!(p[1..].windows(2).all(|w| w[0] == w[1]) && p[1] > 250, "{p:?}");
}

#[test]
fn timer1_overflow_period_follows_prescaler() {
    for pre in [1u64, 2, 4, 8] {
        let s = run(&poll("TMR1", 4_000_000, &format!("TMR1.start(prescaler={pre})"), "TMR1"), 65_536 * pre * 3 + 1_000);
        let p = periods(&s, "TMR1IF");
        assert!(!p.is_empty() && p.iter().all(|&c| c == 65_536 * pre), "prescaler={pre}: {p:?}");
    }
}

#[test]
fn timer1_set_and_value_round_trip() {
    let src = format!(
        "{}v: u16 = 0
def main() -> None:
    global v
    TMR1.start(prescaler=1)
    TMR1.set(0x12FF)
    v = TMR1.value()
    while True:
        pass
",
        header("TMR1", 4_000_000)
    );
    let (s, c) = run_c(&src, 400);
    let g = c.globals.iter().find(|g| g.name == "v").unwrap();
    let v = u16::from(s.read(g.addr as usize + 1)) << 8 | u16::from(s.read(g.addr as usize));
    assert!((0x12FF..=0x1310).contains(&v), "{v:#06x}");
}

#[test]
fn timer2_stop_freezes_the_counter() {
    let src = format!("{}def main() -> None:\n    TMR2.every_ms(1)\n    TMR2.stop()\n    while True:\n        pass\n", header("TMR2", 4_000_000));
    let s = run(&src, 5_000);
    assert!(s.timer_events.is_empty(), "{:?}", s.timer_events);
}
