//! Rutinas de runtime (#27): `*`, `//`, `%` y desplazamientos por una cantidad variable.
//! Todas se prueban contra el interprete de la IR, que es la referencia de la semantica.

use std::path::PathBuf;

use pypic_device::DevicePack;
use pypic_frontend::{analyze, TargetProfile};
use pypic_ir::{lower, verify, Machine};
use pypic_pic14::codegen::Compiled;
use pypic_pic14::sim::Sim;
use pypic_pic14::{compile, Options};

fn pack_target() -> TargetProfile {
    DevicePack::load(&PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../devices/pic16f887")).unwrap().target_profile()
}

fn build(src: &str) -> (pypic_ir::Module, Compiled) {
    let r = analyze(src, &pack_target());
    let p = r.program.unwrap_or_else(|| panic!("frontend:\n{}\n{src}", r.diagnostics.render(src, "t.py")));
    let m = lower(&p);
    verify(&m).unwrap_or_else(|e| panic!("IR invalida: {e:?}"));
    let c = compile(&m, &Options::default()).unwrap_or_else(|e| panic!("codegen: {e}\n{src}"));
    (m, c)
}

/// Ejecuta en la IR y en el simulador, compara todas las globales y devuelve las del simulador.
fn differential(src: &str, max_cycles: u64) -> (std::collections::BTreeMap<String, i128>, Compiled, Sim) {
    let (m, c) = build(src);
    let mut mach = Machine::new(&m);
    mach.run_main().unwrap();
    let mut s = Sim::new(&c.image);
    s.run_until(max_cycles, |s| s.halted);
    assert!(s.halted, "el programa no termino en {max_cycles} ciclos\n{src}");
    let mut out = std::collections::BTreeMap::new();
    for g in c.globals.iter().filter(|g| !g.is_const) {
        let n = g.ty.bits().max(8) as usize / 8;
        let v: i128 = (0..n).map(|i| (s.read(g.addr as usize + i) as i128) << (8 * i)).sum();
        let v = g.ty.wrap(v);
        assert_eq!(v, mach.global(&g.name).unwrap(), "global {}\n{src}", g.name);
        out.insert(g.name.clone(), v);
    }
    (out, c, s)
}

const HEAD: &str = "from pypic.device import PIC16F887\n";

/// Todos los pares de un tipo de 8 bits: el resultado de `a OP b` se mezcla en un hash de 16 bits.
fn all_pairs_8(ty: &str, op: &str) -> String {
    let (lo, hi) = if ty == "i8" { (-128, 127) } else { (0, 255) };
    format!(
        "{HEAD}h: u16 = 0\ndef main() -> None:\n    global h\n    a: {ty} = {lo}\n    while True:\n        b: {ty} = {lo}\n        while True:\n            v: {ty} = a {op} b\n            h = (h << 3) ^ (h >> 5) ^ u16(v)\n            if b == {hi}:\n                break\n            b += 1\n        if a == {hi}:\n            break\n        a += 1\n"
    )
}

/// Un conjunto de valores de 16 bits con los casos de borde, en dos tablas (byte alto y bajo).
const SAMPLES_16: [u16; 24] = [0, 1, 2, 3, 7, 255, 256, 257, 511, 1000, 4095, 12345, 0x5555, 0x7FFE, 0x7FFF, 0x8000, 0x8001, 0xAAAA, 0xFF00, 0xFFFE, 0xFFFF, 40000, 65000, 300];

fn all_pairs_16(ty: &str, op: &str) -> String {
    let hi: Vec<String> = SAMPLES_16.iter().map(|v| (v >> 8).to_string()).collect();
    let lo: Vec<String> = SAMPLES_16.iter().map(|v| (v & 0xFF).to_string()).collect();
    let n = SAMPLES_16.len();
    let cast = |v: &str| if ty == "i16" { format!("i16({v})") } else { v.to_string() };
    format!(
        "{HEAD}HI: Rom[u8] = [{}]\nLO: Rom[u8] = [{}]\nh: u16 = 0\ndef main() -> None:\n    global h\n    for i in range({n}):\n        a: {ty} = {}\n        for j in range({n}):\n            b: {ty} = {}\n            v: {ty} = a {op} b\n            h = (h << 3) ^ (h >> 5) ^ u16(v)\n",
        hi.join(", "),
        lo.join(", "),
        cast("(u16(HI[i]) << 8) | u16(LO[i])"),
        cast("(u16(HI[j]) << 8) | u16(LO[j])"),
    )
}

// ---- multiplicar, dividir y modulo: todos los pares de 8 bits --------------------------------------

#[test]
fn u8_multiplication_matches_the_reference_for_every_pair() {
    let (g, _, s) = differential(&all_pairs_8("u8", "*"), 60_000_000);
    // las 65 536 multiplicaciones se ejecutaron de verdad en el simulador (y el hash no es trivial)
    assert!(s.cycles > 7_000_000 && g["h"] != 0, "{} ciclos", s.cycles);
}

#[test]
fn u8_division_and_modulo_match_the_reference_for_every_pair_including_zero() {
    differential(&all_pairs_8("u8", "//"), 120_000_000);
    differential(&all_pairs_8("u8", "%"), 120_000_000);
}

#[test]
fn i8_division_and_modulo_match_the_reference_for_every_pair() {
    differential(&all_pairs_8("i8", "//"), 160_000_000);
    differential(&all_pairs_8("i8", "%"), 160_000_000);
}

#[test]
fn i8_multiplication_matches_the_reference_for_every_pair() {
    differential(&all_pairs_8("i8", "*"), 60_000_000);
}

// ---- 16 bits ---------------------------------------------------------------------------------------

#[test]
fn sixteen_bit_operations_match_the_reference_on_edge_values() {
    for ty in ["u16", "i16"] {
        for op in ["*", "//", "%"] {
            differential(&all_pairs_16(ty, op), 60_000_000);
        }
    }
}

// ---- resultados concretos --------------------------------------------------------------------------

#[test]
fn known_results() {
    let src = format!("{HEAD}a: u8 = 0\nb: u8 = 0\nc: i8 = 0\nd: i8 = 0\ne: u16 = 0\nf: i16 = 0\nx8: u8 = 200\ny8: u8 = 2\nxs: i8 = -7\nys: i8 = 2\nx16: u16 = 1000\ny16: u16 = 7\nz: i16 = -1000\nw: i16 = 7\ndef main() -> None:\n    global a, b, c, d, e, f\n    a = x8 * y8\n    b = x8 // y8 + x8 % y8\n    c = xs // ys\n    d = xs % ys\n    e = x16 * y16\n    f = z // w\n");
    let (g, ..) = differential(&src, 5_000_000);
    assert_eq!(g["a"], (200 * 2) % 256); // 144: modular
    assert_eq!(g["b"], 100);
    assert_eq!((g["c"], g["d"]), (-3, -1)); // trunca hacia cero; el resto lleva el signo del dividendo
    assert_eq!(g["e"], 7000);
    assert_eq!(g["f"], -142);
}

#[test]
fn division_by_zero_at_run_time_is_defined_and_does_not_hang() {
    let src = format!("{HEAD}z8: u8 = 0\nz16: u16 = 0\nzs: i8 = 0\nq8: u8 = 0\nr8: u8 = 0\nq16: u16 = 0\nr16: u16 = 0\nqs: i8 = 0\nrs: i8 = 0\nn8: u8 = 77\nn16: u16 = 5000\nns: i8 = -9\ndef main() -> None:\n    global q8, r8, q16, r16, qs, rs\n    q8 = n8 // z8\n    r8 = n8 % z8\n    q16 = n16 // z16\n    r16 = n16 % z16\n    qs = ns // zs\n    rs = ns % zs\n");
    let (g, ..) = differential(&src, 5_000_000);
    // a // 0 es todo unos del ancho y a % 0 es a
    assert_eq!((g["q8"], g["r8"]), (255, 77));
    assert_eq!((g["q16"], g["r16"]), (65535, 5000));
    assert_eq!((g["qs"], g["rs"]), (-1, -9));
}

#[test]
fn powers_of_two_become_shifts_and_masks_without_a_routine() {
    let src = format!("{HEAD}x: u8 = 77\ny: u16 = 5001\nm8: u8 = 0\nd8: u8 = 0\nr8: u8 = 0\nm16: u16 = 0\nd16: u16 = 0\nr16: u16 = 0\nz: u8 = 0\ndef main() -> None:\n    global m8, d8, r8, m16, d16, r16, z\n    m8 = x * 4\n    m8 = 8 * m8\n    d8 = x // 8\n    r8 = x % 16\n    m16 = y * 2\n    d16 = y // 64\n    r16 = y % 256\n    z = x * 0\n");
    let (g, c, _) = differential(&src, 5_000_000);
    assert_eq!((g["m8"], g["d8"], g["r8"]), ((77 * 32) % 256, 9, 13));
    assert_eq!((g["m16"], g["d16"], g["r16"]), (10002, 78, 5001 % 256));
    assert_eq!(g["z"], 0);
    assert!(!c.listing.contains("rt."), "no hace falta ninguna rutina:\n{}", c.listing);
}

#[test]
fn only_the_requested_routines_are_generated() {
    let mul = format!("{HEAD}r: u8 = 0\na: u8 = 3\nb: u8 = 5\ndef main() -> None:\n    global r\n    r = a * b\n");
    let (_, c, _) = differential(&mul, 1_000_000);
    assert!(c.listing.contains("rt.mul8:") && !c.listing.contains("rt.divmod") && !c.listing.contains("rt.mul16"), "{}", c.listing);
    let sd = format!("{HEAD}r: i16 = 0\na: i16 = -300\nb: i16 = 7\ndef main() -> None:\n    global r\n    r = a // b\n");
    let (g, c, _) = differential(&sd, 1_000_000);
    assert_eq!(g["r"], -42);
    // la division con signo trae la de sin signo, de la que depende
    assert!(c.listing.contains("rt.sdivmod16:") && c.listing.contains("rt.divmod16:"), "{}", c.listing);
}

// ---- desplazamientos por una cantidad variable -------------------------------------------------------

#[test]
fn variable_shifts_match_the_reference_for_every_count() {
    for ty in ["u8", "i8", "u16", "i16"] {
        let v = match ty {
            "u8" => "0xB5",
            "i8" => "-75",
            "u16" => "0xC2B5",
            _ => "-15691",
        };
        let src = format!(
            "{HEAD}hl: u16 = 0\nhr: u16 = 0\ndef main() -> None:\n    global hl, hr\n    x: {ty} = {v}\n    n: u8 = 0\n    while n < 40:\n        l: {ty} = x << n\n        r: {ty} = x >> n\n        hl = (hl << 3) ^ (hl >> 5) ^ u16(l)\n        hr = (hr << 3) ^ (hr >> 5) ^ u16(r)\n        n += 1\n"
        );
        differential(&src, 20_000_000);
    }
}

#[test]
fn a_shift_by_the_width_or_more_clears_or_fills_with_the_sign() {
    let src = format!("{HEAD}a: u8 = 0\nb: i8 = 0\nc: u16 = 0\nn8: u8 = 8\nn200: u8 = 200\nx: u8 = 0xFF\ny: i8 = -2\nz: u16 = 0xFFFF\ndef main() -> None:\n    global a, b, c\n    a = x << n8\n    b = y >> n200\n    c = z >> n200\n");
    let (g, ..) = differential(&src, 2_000_000);
    assert_eq!((g["a"], g["b"], g["c"]), (0, -1, 0));
}

#[test]
fn a_sixteen_bit_shift_count_with_a_nonzero_high_byte_is_a_huge_shift() {
    let src = format!("{HEAD}r: u8 = 0\nn: u16 = 0x0100\nx: u8 = 0x80\ndef main() -> None:\n    global r\n    r = x >> n\n");
    let (g, ..) = differential(&src, 2_000_000);
    assert_eq!(g["r"], 0);
}

// ---- una copia por contexto ------------------------------------------------------------------------

#[test]
fn main_and_the_interrupt_get_their_own_copy_and_do_not_disturb_each_other() {
    let src = format!(
        "{HEAD}from pypic import interrupt\nr_main: u16 = 0\nr_isr: u16 = 0\nam: u16 = 1234\nbm: u16 = 567\nai: u16 = 4321\nbi: u16 = 89\n\n\n@interrupt\ndef isr() -> None:\n    global r_isr\n    r_isr = ai * bi\n\n\ndef main() -> None:\n    global r_main\n    r_main = am * bm\n"
    );
    let (m, c) = build(&src);
    assert!(c.listing.contains("rt.mul16:") && c.listing.contains("rt.mul16.isr:"), "una copia por contexto:\n{}", c.listing);
    // la interrupcion entra en cada instante de la multiplicacion principal: el resultado no cambia
    let mut mach = Machine::new(&m);
    mach.run_main().unwrap();
    let want = mach.global("r_main").unwrap();
    assert_eq!(want, (1234u32 * 567) as i128 % 65536);
    let isr_addr = c.functions.iter().find(|(n, _)| n == "isr").map(|x| x.1).unwrap();
    let _ = isr_addr;
    let mut hits = 0;
    for point in (20..1200).step_by(7) {
        let mut s = Sim::new(&c.image);
        // el simulador no genera interrupciones: se inyecta una en un ciclo concreto de la multiplicacion de main
        s.run_until(point, |_| false);
        // GIE puesto, como lo dejaria el programa; la ISR hace su multiplicacion y vuelve con RETFIE
        s.write(0x0B, 0x80);
        s.interrupt();
        s.run_until(5_000_000, |s| s.halted);
        let g = |name: &str| {
            let g = c.globals.iter().find(|g| g.name == name).unwrap();
            (s.read(g.addr as usize) as i128) | (s.read(g.addr as usize + 1) as i128) << 8
        };
        assert_eq!(g("r_main"), want, "interrupcion en el ciclo {point}");
        assert_eq!(g("r_isr"), ((4321u32 * 89) % 65536) as i128, "interrupcion en el ciclo {point}");
        hits += 1;
    }
    assert!(hits > 100);
}

// ---- pila de hardware ------------------------------------------------------------------------------

#[test]
fn a_runtime_call_counts_its_stack_levels() {
    // main (1) + f1..f6 (6) = 7; la multiplicacion del ultimo es el octavo; con signo (division) son dos
    let chain = |body: &str| {
        let mut s = format!("{HEAD}r: u8 = 0\nk: u8 = 3\n");
        s.push_str(&format!("def f6() -> None:\n    global r\n    {body}\n\n\n"));
        for i in (1..6).rev() {
            s.push_str(&format!("def f{i}() -> None:\n    f{}()\n\n\n", i + 1));
        }
        s.push_str("def main() -> None:\n    f1()\n");
        s
    };
    let ok = analyze(&chain("r = k * k"), &pack_target());
    assert!(ok.program.is_some(), "{}", ok.diagnostics.render(&chain("r = k * k"), "t.py"));
    let src = chain("r = u8(sk // sk)").replace("r: u8 = 0\n", "r: u8 = 0\nsk: i8 = 3\n");
    let bad = analyze(&src, &pack_target());
    assert!(bad.program.is_none() && bad.diagnostics.render(&src, "t.py").contains("PYP2061"), "{}", bad.diagnostics.render(&src, "t.py"));
}

#[test]
fn a_warning_names_each_routine_once() {
    let src = format!("{HEAD}r: u8 = 0\ns: u16 = 0\na: u8 = 3\nb: u16 = 5\ndef main() -> None:\n    global r, s\n    r = a * a\n    r = a * r\n    s = b * b\n    r = a // 3\n");
    let res = analyze(&src, &pack_target());
    let text = res.diagnostics.render(&src, "t.py");
    assert!(res.program.is_some(), "{text}");
    assert_eq!(text.matches("PYP3011").count(), 3, "{text}"); // mul8, mul16 y divmod8
    assert!(text.contains("mul8") && text.contains("mul16") && text.contains("divmod8"), "{text}");
}

// ---- costo -------------------------------------------------------------------------------------------

/// Ciclos del operador entre el BSF y el BCF de RD0 (sin el BCF final).
fn cycles(ty: &str, op: &str, a: i64, b: i64) -> u64 {
    let src = format!("{HEAD}from pypic.periph.gpio import RD0\nr: {ty} = 0\nx: {ty} = {a}\ny: {ty} = {b}\ndef main() -> None:\n    global r\n    RD0.output()\n    RD0.on()\n    r = x {op} y\n    RD0.off()\n");
    let (_, c) = build(&src);
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
    down.unwrap() - up.unwrap() - 1
}

#[test]
fn the_multiplication_takes_the_same_cycles_for_every_operand() {
    for (ty, bits) in [("u8", 8u64), ("u16", 16)] {
        let c: Vec<u64> = [(0, 0), (1, 1), (255, 255), (3, 200), (200, 3), (170, 85)].iter().map(|&(a, b)| cycles(ty, "*", a, b)).collect();
        assert!(c.windows(2).all(|w| w[0] == w[1]), "{ty}: {c:?}");
        println!("mul{bits}: {} ciclos", c[0]);
    }
}

#[test]
fn the_division_cycles_stay_inside_a_documented_range() {
    for (ty, hi) in [("u8", 255i64), ("u16", 65535)] {
        let all: Vec<u64> = [(0, 1), (hi, 1), (hi, hi), (100, 7), (1, hi), (hi, 2), (5, 0)].iter().map(|&(a, b)| cycles(ty, "//", a, b)).collect();
        println!("divmod {ty}: {} ..= {} ciclos", all.iter().min().unwrap(), all.iter().max().unwrap());
        assert!(all.iter().max().unwrap() - all.iter().min().unwrap() < 20 * (hi as u64 + 1).ilog2() as u64, "{all:?}");
    }
}

#[test]
fn the_documented_costs_are_what_the_simulator_measures() {
    // multiplicacion: constante (docs/sdk-runtime.md)
    assert_eq!(cycles("u8", "*", 3, 200), 116);
    assert_eq!(cycles("u16", "*", 3, 20000), 339);
    // division con signo: del camino del divisor 0 (el mas corto) al peor
    let range = |ty: &str, hi: i64| {
        let all: Vec<u64> = [(0, 1), (hi, 1), (-hi, 1), (hi, -hi), (100, -7), (-100, 7), (5, 0)].iter().map(|&(a, b)| cycles(ty, "//", a, b)).collect();
        (*all.iter().min().unwrap(), *all.iter().max().unwrap())
    };
    assert_eq!(range("i8", 127), (17, 195));
    assert_eq!(range("i16", 32767), (28, 543));
    // desplazamiento por una variable: base + un costo por bit, acotado al ancho del tipo
    let shl = |ty: &str| -> Vec<u64> { [0i64, 1, 2, 4, 7, 8, 9, 40].iter().map(|&n| cycles(ty, "<<", 1, n)).collect() };
    assert_eq!(shl("u8"), vec![13, 21, 29, 45, 69, 78, 78, 78]);
    assert_eq!(shl("u16"), vec![19, 28, 37, 55, 82, 91, 100, 164]);
}

#[test]
fn the_routines_are_small() {
    let src = format!("{HEAD}a: u8 = 3\nb: u16 = 5\nc: i8 = -3\nd: i16 = -5\nr: u16 = 0\ndef main() -> None:\n    global r\n    r = u16(a * a) + b * b + u16(c // c) + u16(d // d) + u16(a // a) + u16(b // b)\n");
    let (_, c) = build(&src);
    let mut starts: Vec<(String, u32)> = Vec::new();
    let (mut last, mut pending) = (0u32, None::<String>);
    for l in c.listing.lines() {
        if let Some(name) = l.strip_prefix("rt.").and_then(|n| n.strip_suffix(':')) {
            pending = Some(name.to_string());
        } else if l.len() > 5 && l.as_bytes()[4] == b':' {
            let addr = u32::from_str_radix(&l[..4], 16).unwrap();
            if let Some(n) = pending.take() {
                starts.push((n, addr));
            }
            last = addr + 1;
        }
    }
    let size = |name: &str| {
        let i = starts.iter().position(|x| x.0 == name).unwrap_or_else(|| panic!("falta rt.{name}: {starts:?}"));
        starts.get(i + 1).map_or(last, |x| x.1) - starts[i].1
    };
    assert_eq!((size("mul8"), size("mul16"), size("divmod8"), size("divmod16"), size("sdivmod8"), size("sdivmod16")), (16, 24, 23, 35, 31, 48));
}
