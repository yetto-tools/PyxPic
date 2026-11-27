//! Programas de mas de 2048 palabras (#31): PCLATH en cada llamada, funciones dentro de una pagina, interrupcion y tablas.

use std::path::PathBuf;

use pyxpic_device::DevicePack;
use pyxpic_frontend::{analyze, TargetProfile};
use pyxpic_ir::{lower, verify, Machine};
use pyxpic_pic14::codegen::{CodegenError, Compiled};
use pyxpic_pic14::sim::Sim;
use pyxpic_pic14::{compile, Options};

fn pack_target() -> TargetProfile {
    DevicePack::load(&PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../devices/pic16f887")).unwrap().target_profile()
}

fn build(src: &str) -> (pyxpic_ir::Module, Result<Compiled, CodegenError>) {
    let r = analyze(src, &pack_target());
    let p = r.program.unwrap_or_else(|| panic!("frontend:\n{}", r.diagnostics.render(src, "t.py")));
    let m = lower(&p);
    verify(&m).unwrap_or_else(|e| panic!("IR invalida: {e:?}"));
    let c = compile(&m, &Options::default());
    (m, c)
}

fn build_ok(src: &str) -> (pyxpic_ir::Module, Compiled) {
    let (m, c) = build(src);
    (m, c.unwrap_or_else(|e| panic!("codegen: {e}")))
}

fn global(c: &Compiled, s: &Sim, name: &str) -> i128 {
    let g = c.globals.iter().find(|g| g.name == name).unwrap();
    let n = g.ty.bits().max(8) as usize / 8;
    g.ty.wrap((0..n).map(|i| (s.read(g.addr as usize + i) as i128) << (8 * i)).sum())
}

const HEAD: &str = "from pyxpic.device import PIC16F887\n";

/// `n` funciones de ~`stmts` sentencias cada una (cada `r += k` son unas 8 palabras), que `main` llama una tras otra.
fn big_program(n: usize, stmts: usize, extra_main: &str) -> String {
    let mut s = format!("{HEAD}r: u16 = 0\nticks: u8 = 0\n");
    for i in 0..n {
        s.push_str(&format!("\n\ndef f{i}() -> None:\n    global r\n"));
        for k in 0..stmts {
            s.push_str(&format!("    r += {}\n", (i * 7 + k) % 13 + 1));
        }
    }
    s.push_str("\n\ndef main() -> None:\n");
    for i in 0..n {
        s.push_str(&format!("    f{i}()\n"));
    }
    s.push_str(extra_main);
    s
}

fn differential(src: &str) -> (Compiled, Sim) {
    let (m, c) = build_ok(src);
    let mut mach = Machine::new(&m);
    mach.run_main().unwrap();
    let mut s = Sim::new(&c.image);
    s.run_until(50_000_000, |s| s.halted);
    assert!(s.halted, "no termino");
    for g in c.globals.iter().filter(|g| !g.is_const) {
        assert_eq!(global(&c, &s, &g.name), mach.global(&g.name).unwrap(), "global {}", g.name);
    }
    (c, s)
}

// ---- un programa de varias paginas -------------------------------------------------------------------

#[test]
fn a_program_larger_than_one_page_compiles_and_matches_the_reference() {
    let (c, s) = differential(&big_program(8, 110, ""));
    assert!(c.flash_words > 2048 * 2, "{} palabras", c.flash_words);
    assert!(global(&c, &s, "r") > 0);
    // alguna funcion quedo en una pagina distinta de la 0
    assert!(c.functions.iter().any(|(_, a)| *a >= 0x800), "{:?}", c.functions);
    assert!(c.functions.iter().any(|(_, a)| *a >= 0x1000), "{:?}", c.functions);
}

#[test]
fn every_function_sits_entirely_inside_one_page() {
    let (_, c) = build_ok(&big_program(8, 110, ""));
    let mut starts: Vec<u32> = c.functions.iter().map(|(_, a)| *a).collect();
    starts.sort_unstable();
    let top = c.image.iter().map(|(a, _)| a).filter(|a| *a < 0x2000).max().unwrap();
    for (i, a) in starts.iter().enumerate() {
        let end = starts.get(i + 1).map_or(top, |n| n - 1);
        // el hueco de relleno entre funciones puede ser mayor, pero ninguna palabra de la funcion cruza un limite de 0x800
        let last_word = (*a..=end).rev().find(|w| c.image.get(*w).is_some()).unwrap();
        assert_eq!(a >> 11, last_word >> 11, "la funcion que empieza en {a:#x} cruza una pagina (termina en {last_word:#x})");
    }
}

#[test]
fn functions_call_each_other_across_pages_in_both_directions_and_keep_returning_values() {
    let mut s = format!("{HEAD}r: u16 = 0\nout: u8 = 0\n\n\ndef low() -> u8:\n    return 7\n");
    // relleno para que `high` caiga en otra pagina
    for i in 0..4 {
        s.push_str(&format!("\n\ndef pad{i}() -> None:\n    global r\n"));
        for k in 0..130 {
            s.push_str(&format!("    r += {}\n", (k % 5) + 1));
        }
    }
    s.push_str("\n\ndef high(x: u8) -> u8:\n    return x + low()\n\n\ndef main() -> None:\n    global out\n    pad0()\n    pad1()\n    pad2()\n    pad3()\n    out = high(10) + low()\n");
    let (c, sim) = differential(&s);
    assert_eq!(global(&c, &sim, "out"), 10 + 7 + 7);
    let addr = |n: &str| c.functions.iter().find(|f| f.0 == n).unwrap().1;
    assert_ne!(addr("low") >> 11, addr("high") >> 11, "deben estar en paginas distintas: {:?}", c.functions);
}

#[test]
fn in_a_multi_page_program_every_call_costs_four_more_cycles() {
    // la misma llamada en un programa que cabe en una pagina y en uno que no: cada CALL lleva dos BCF/BSF antes y dos despues
    let src = |filler: usize| {
        let mut s = format!("{HEAD}from pyxpic.periph.gpio import RD0\nr: u16 = 0\n\n\ndef callee() -> None:\n    pass\n");
        for name in ["pad_a", "pad_b"] {
            s.push_str(&format!("\n\ndef {name}() -> None:\n    global r\n"));
            for k in 0..filler {
                s.push_str(&format!("    r += {}\n", (k % 5) + 1));
            }
        }
        s.push_str("\n\ndef caller() -> None:\n    callee()\n\n\ndef main() -> None:\n    RD0.output()\n    RD0.on()\n    caller()\n    RD0.off()\n    pad_a()\n    pad_b()\n");
        s
    };
    let measure = |filler: usize| -> (u64, bool) {
        let (_, c) = build_ok(&src(filler));
        let multi = c.flash_words > 2048;
        let mut s = Sim::new(&c.image);
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
        (down.unwrap() - up.unwrap(), multi)
    };
    let (small, multi_small) = measure(10);
    assert!(!multi_small, "el programa chico cabe en una pagina");
    let (big, multi_big) = measure(190);
    assert!(multi_big);
    // dos llamadas medidas (main -> caller -> callee), +4 ciclos cada una
    assert_eq!(big - small, 8, "{small} -> {big}");
}

// ---- interrupcion -------------------------------------------------------------------------------------

#[test]
fn the_interrupt_can_live_in_a_high_page_and_does_not_disturb_the_main_flow() {
    let mut s = format!("{HEAD}from pyxpic import interrupt\nr: u16 = 0\nticks: u8 = 0\n");
    for i in 0..6 {
        s.push_str(&format!("\n\ndef f{i}() -> None:\n    global r\n"));
        for k in 0..100 {
            s.push_str(&format!("    r += {}\n", (i * 3 + k) % 9 + 1));
        }
    }
    // la ISR se define al final: su cuerpo cae en una pagina alta
    s.push_str("\n\n@interrupt\ndef isr() -> None:\n    global ticks\n    ticks += 1\n\n\ndef main() -> None:\n");
    for i in 0..6 {
        s.push_str(&format!("    f{i}()\n"));
    }
    let (m, c) = build_ok(&s);
    let isr_at = c.functions.iter().find(|f| f.0 == "isr").unwrap().1;
    assert!(isr_at >= 0x800, "la ISR debe estar fuera de la pagina 0: {isr_at:#x}");
    let mut mach = Machine::new(&m);
    mach.run_main().unwrap();
    let want = mach.global("r").unwrap();
    // una interrupcion en instantes repartidos por todo el programa (cada una en un contexto de pagina distinto)
    let mut seen_pages = std::collections::BTreeSet::new();
    for point in (50..40_000u64).step_by(397) {
        let mut sim = Sim::new(&c.image);
        sim.run_until(point, |_| false);
        if sim.halted {
            break;
        }
        seen_pages.insert(sim.pc >> 11);
        sim.write(0x0B, 0x80); // GIE
        sim.interrupt();
        sim.run_until(50_000_000, |s| s.halted);
        assert!(sim.halted, "no termino con la interrupcion en el ciclo {point}");
        assert_eq!(global(&c, &sim, "r"), want, "r cambio por una interrupcion en el ciclo {point} (pc estaba en la pagina {})", sim.pc >> 11);
        assert_eq!(global(&c, &sim, "ticks"), 1, "ciclo {point}");
    }
    assert!(seen_pages.len() >= 3, "la prueba debe interrumpir codigo de varias paginas: {seen_pages:?}");
}

// ---- tablas y rutinas de runtime en paginas altas ------------------------------------------------------

#[test]
fn rom_lookups_and_runtime_routines_work_from_any_page() {
    let mut s = format!("{HEAD}T: Rom[u8] = [3, 1, 4, 1, 5, 9, 2, 6]\nr: u16 = 0\nlook: u16 = 0\nprod: u16 = 0\nq: u16 = 0\nk: u8 = 5\nm: u16 = 321\nn: u16 = 17\n");
    for i in 0..3 {
        s.push_str(&format!("\n\ndef pad{i}() -> None:\n    global r\n"));
        for j in 0..130 {
            s.push_str(&format!("    r += {}\n", (j % 7) + 1));
        }
    }
    s.push_str("\n\ndef work() -> None:\n    global look, prod, q\n    look += u16(T[k])\n    prod = m * n\n    q = m // n\n\n\ndef main() -> None:\n    global look\n    work()\n    pad0()\n    pad1()\n    work()\n    pad2()\n    work()\n");
    let (c, sim) = differential(&s);
    assert!(c.flash_words > 0x800);
    assert_eq!(global(&c, &sim, "look"), 3 * 9);
    assert_eq!(global(&c, &sim, "prod"), 321 * 17);
    assert_eq!(global(&c, &sim, "q"), 321 / 17);
}

// ---- limites ---------------------------------------------------------------------------------------------

#[test]
fn a_function_larger_than_a_page_is_a_clear_error() {
    let mut s = format!("{HEAD}r: u16 = 0\n\n\ndef huge() -> None:\n    global r\n");
    for k in 0..300 {
        s.push_str(&format!("    r += {}\n", (k % 9) + 1));
    }
    // 300 sentencias de 8 palabras = ~2400 palabras en una sola funcion
    s.push_str("\n\ndef main() -> None:\n    huge()\n");
    let (_, c) = build(&s);
    match c {
        Err(CodegenError::FunctionTooLarge { name, words }) => {
            assert_eq!(name, "huge");
            assert!(words > 2048, "{words}");
        }
        other => panic!("esperaba FunctionTooLarge, salio {:?}", other.map(|c| c.flash_words)),
    }
}

#[test]
fn a_program_that_does_not_fit_the_whole_flash_is_still_too_large() {
    // 8192 palabras es todo el dispositivo: mas que eso es TooLarge con el total
    let (_, c) = build(&big_program(40, 110, ""));
    assert!(matches!(c, Err(CodegenError::TooLarge { words }) if words > 8192), "{:?}", c.map(|c| c.flash_words));
}

#[test]
fn a_program_that_fits_one_page_is_unchanged() {
    // sin PageSet/PageHere: mismo codigo que antes (el blink sigue siendo byte a byte el probado en Proteus)
    let (_, c) = build_ok(&format!("{HEAD}r: u8 = 0\ndef f() -> None:\n    global r\n    r += 1\n\n\ndef main() -> None:\n    f()\n    f()\n"));
    assert!(!c.listing.contains("PCLATH") && !c.listing.contains("0x0a,"), "{}", c.listing);
}
