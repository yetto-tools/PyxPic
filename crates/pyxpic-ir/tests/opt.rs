//! Las optimizaciones de la IR (#6): cada pasada con `verify()` despues, lo que hace cada una antes y despues, lo que no debe tocar, la
//! idempotencia y, sobre todo, que el programa optimizado se comporte igual que el original (el interprete sobre la IR sin optimizar es el oraculo).

use pyxpic_frontend::{analyze, TargetProfile};
use pyxpic_ir::ir::{Inst, Terminator};
use pyxpic_ir::opt::{self, Stats};
use pyxpic_ir::{lower_unoptimized, print, verify, Machine, Module};

fn raw(src: &str) -> Module {
    let r = analyze(src, &TargetProfile::generic());
    lower_unoptimized(&r.program.unwrap_or_else(|| panic!("frontend:\n{}\n{src}", r.diagnostics.render(src, "t.py"))))
}

/// La IR tras cada pasada, comprobando `verify()` entre una y otra.
fn optimized_verifying_each_pass(src: &str) -> (Module, Stats) {
    let mut m = raw(src);
    let mut st = Stats::default();
    for _ in 0..16 {
        let before = st;
        opt::propagate(&mut m, &mut st);
        verify(&m).unwrap_or_else(|e| panic!("verify tras propagate: {e:?}\n{}", print(&m)));
        opt::simplify_cfg(&mut m, &mut st);
        verify(&m).unwrap_or_else(|e| panic!("verify tras simplify_cfg: {e:?}\n{}", print(&m)));
        opt::eliminate_dead(&mut m, &mut st);
        verify(&m).unwrap_or_else(|e| panic!("verify tras eliminate_dead: {e:?}\n{}", print(&m)));
        if st == before {
            break;
        }
    }
    opt::compact_temps(&mut m, &mut st);
    verify(&m).unwrap_or_else(|e| panic!("verify tras compact_temps: {e:?}\n{}", print(&m)));
    (m, st)
}

fn func<'a>(m: &'a Module, name: &str) -> &'a pyxpic_ir::ir::Function {
    m.functions.iter().find(|f| f.name == name).unwrap()
}

// ---- lo que hace cada pasada ------------------------------------------------------------------------------------------------------

#[test]
fn constants_propagate_fold_and_the_dead_stores_disappear() {
    let src = "g: uint8 = 0\n\n\ndef main() -> None:\n    global g\n    a: uint8 = 3\n    b: uint8 = a + 4\n    g = b * 2\n";
    let before = print(&raw(src));
    assert!(before.contains("+ ") && before.contains("*"), "la IR original calcula: {before}");
    let (m, st) = optimized_verifying_each_pass(src);
    let after = print(&m);
    assert!(after.contains("g = 14:uint8"), "{after}");
    assert!(!after.contains('+') && !after.contains('*'), "se plego todo: {after}");
    assert!(!after.contains("a ") || !after.contains("a = "), "los locales muertos se quitan: {after}");
    assert!(st.folded >= 2 && st.propagated >= 2 && st.insts_removed >= 2, "{st:?}");
}

#[test]
fn a_comparison_that_becomes_constant_removes_the_branch() {
    let src = "g: uint8 = 0\n\n\ndef main() -> None:\n    global g\n    a: uint8 = 3\n    if a > 2:\n        g = 1\n    else:\n        g = 2\n";
    assert!(print(&raw(src)).contains("branch"));
    let (m, st) = optimized_verifying_each_pass(src);
    let after = print(&m);
    assert!(!after.contains("branch") && after.contains("g = 1:uint8") && !after.contains("g = 2:uint8"), "{after}");
    assert_eq!(func(&m, "main").blocks.len(), 1, "los bloques se funden en uno: {after}");
    assert!(st.branches_simplified >= 1 && st.blocks_removed >= 2, "{st:?}");
}

/// Bloques vacios que solo saltan a otro, sin contar el de entrada (el 0 debe seguir siendo la entrada).
fn empty_jump_blocks(m: &Module) -> usize {
    m.functions.iter().flat_map(|f| f.blocks.iter().skip(1)).filter(|b| b.insts.is_empty() && matches!(b.term, Terminator::Jump(_))).count()
}

#[test]
fn empty_blocks_that_only_jump_are_skipped() {
    let src = "g: uint8 = 0\n\n\ndef main() -> None:\n    global g\n    while g < 10:\n        if g > 5:\n            g += 2\n        g += 1\n";
    let r = raw(src);
    assert!(empty_jump_blocks(&r) > 0, "la IR original tiene `bbN: jump bbM`: {}", print(&r));
    let (m, _) = optimized_verifying_each_pass(src);
    assert_eq!(empty_jump_blocks(&m), 0, "{}", print(&m));
    assert!(func(&m, "main").blocks.len() < func(&r, "main").blocks.len());
}

#[test]
fn dead_temporaries_are_renumbered_so_they_do_not_waste_ram() {
    let src = "g: uint8 = 0\n\n\ndef main() -> None:\n    global g\n    a: uint8 = 3\n    g = (a + 4) * 2\n";
    let r = raw(src);
    let (m, st) = optimized_verifying_each_pass(src);
    assert!(func(&m, "main").temps.len() < func(&r, "main").temps.len(), "{} frente a {}", func(&m, "main").temps.len(), func(&r, "main").temps.len());
    assert!(st.temps_removed > 0, "{st:?}");
}

// ---- lo que NO debe tocar ---------------------------------------------------------------------------------------------------------------

#[test]
fn side_effects_are_never_removed() {
    let src = "from pyxpic.periph.gpio import RD0\nfrom pyxpic.system.time import pause\n\nCLOCK_HZ: Const[uint32] = 4_000_000\ng: uint8 = 0\n\n\ndef f() -> uint8:\n    return 7\n\n\ndef main() -> None:\n    global g\n    RD0.output()\n    _unused: uint8 = f()\n    pause(1)\n    RD0.toggle()\n    g = 5\n    _dead: uint8 = g + 1\n";
    let (m, _) = optimized_verifying_each_pass(src);
    let after = print(&m);
    for kept in ["call f", "delay", "g = 5:uint8"] {
        assert!(after.contains(kept), "se quito {kept:?}: {after}");
    }
    assert!(func(&m, "main").blocks.iter().flat_map(|b| &b.insts).any(|i| matches!(i, Inst::PinToggle { .. } | Inst::PinCfg { .. })), "{after}");
}

#[test]
fn a_global_is_never_propagated_because_an_interrupt_may_change_it() {
    // t guarda el g de antes; si se propagara g en lugar de t, h valdria 9 y no 1
    let src = "g: uint8 = 1\nh: uint8 = 0\n\n\ndef main() -> None:\n    global g, h\n    t: uint8 = g\n    g = 9\n    h = t\n";
    let (m, _) = optimized_verifying_each_pass(src);
    let mut mach = Machine::new(&m);
    mach.run_main().unwrap();
    assert_eq!((mach.global("g"), mach.global("h")), (Some(9), Some(1)));
    // y la copia de un global a un local no se sustituye por el global en las lecturas que siguen
    let after = print(&m);
    assert!(after.contains("t = g") || after.contains("h = g") || after.contains("h = t"), "{after}");
}

#[test]
fn bits_intrinsics_that_modify_in_place_are_not_substituted() {
    let src = "from pyxpic.bits import bit_set, bit_clear, bit_test\n\ng: uint8 = 0\nok: bool = False\n\n\ndef main() -> None:\n    global g, ok\n    x: uint8 = 0\n    bit_set(x, 3)\n    bit_set(x, 0)\n    bit_clear(x, 0)\n    g = x\n    ok = bit_test(x, 3)\n";
    let (m, _) = optimized_verifying_each_pass(src);
    let mut mach = Machine::new(&m);
    mach.run_main().unwrap();
    assert_eq!((mach.global("g"), mach.global("ok")), (Some(8), Some(1)), "{}", print(&m));
}

#[test]
fn asm_functions_are_left_alone() {
    let src = "from pyxpic.device import PIC16F887\n\n\n@asm_function\ndef f(a: uint8) -> uint8:\n    \"\"\"\n    MOVF a,W\n    RETURN\n    \"\"\"\n\n\ndef main() -> None:\n    _r: uint8 = f(1)\n";
    let r = raw(src);
    let (m, _) = optimized_verifying_each_pass(src);
    assert_eq!(print(&r).split("fn main").next(), print(&m).split("fn main").next(), "la funcion asm queda igual");
}

#[test]
fn optimizing_twice_changes_nothing_more() {
    for src in [
        "g: uint8 = 0\n\n\ndef main() -> None:\n    global g\n    for i in range(10):\n        if i > 3:\n            g += i\n",
        "g: uint16 = 0\n\n\ndef main() -> None:\n    global g\n    a: uint16 = 300\n    g = a * 3 + 1\n",
    ] {
        let (mut m, _) = optimized_verifying_each_pass(src);
        let once = print(&m);
        let again = opt::optimize(&mut m);
        assert_eq!(again.total(), 0, "una segunda vez no cambia nada: {again:?}\n{once}");
        assert_eq!(print(&m), once);
    }
}

// ---- el programa optimizado se comporta igual -----------------------------------------------------------------------------------------

/// Lo que se observa de una ejecucion: los globales, los bytes enviados por la UART y los arreglos al terminar `main`.
type Observed = (Vec<(String, i128)>, Vec<u8>, Vec<Vec<u8>>);

fn observe(m: &Module) -> Observed {
    let mut mach = Machine::new(m);
    mach.uart_rx.extend(*b"42\n0xFF\n");
    let r = mach.run_main();
    assert!(r.is_ok(), "el programa termina: {r:?}");
    (m.globals.iter().filter(|g| !g.is_const).map(|g| (g.name.clone(), mach.global(&g.name).unwrap())).collect(), mach.uart_tx.clone(), mach.arrays.clone())
}

const CORPUS: &[&str] = &[
    // aritmetica de 8 y 16 bits, desplazamientos, division por una constante y por una variable
    "a: uint8 = 0\nb: uint16 = 0\nc: int8 = 0\nd: uint8 = 0\n\n\ndef main() -> None:\n    global a, b, c, d\n    x: uint8 = 200\n    y: uint8 = 100\n    a = x + y\n    b = uint16(x) * 300\n    p: int8 = -100\n    q: int8 = 50\n    c = p - q\n    z: uint8 = 7\n    d = (x // z) + (x % z) + (x << 2) + (y >> 3) + (x ^ y) + (x & 15) + (y | 3)\n",
    // bucles, ramas y salidas anticipadas
    "s: uint16 = 0\nr: uint8 = 0\n\n\ndef suma(n: uint8) -> uint16:\n    t: uint16 = 0\n    for i in range(n):\n        if i == 7:\n            continue\n        if i > 20:\n            break\n        t += uint16(i)\n    return t\n\n\ndef main() -> None:\n    global s, r\n    s = suma(30)\n    k: uint8 = 0\n    while k < 50:\n        k += 3\n        if k > 40:\n            r = k\n",
    // arreglos con indice constante, variable y fuera de rango
    "buf: uint8[8]\ntotal: uint8 = 0\nlast: uint8 = 0\n\n\ndef main() -> None:\n    global total, last\n    for i in range(8):\n        buf[i] = i * 3\n    k: uint8 = 12\n    buf[k] = 99\n    buf[2] += 5\n    for i in range(8):\n        total += buf[i]\n    last = buf[7]\n",
    // llamadas con varios argumentos y retornos
    "o: uint8 = 0\n\n\ndef mezcla(a: uint8, b: uint8, c: uint8) -> uint8:\n    if a > b:\n        return a - b + c\n    return b - a + c\n\n\ndef main() -> None:\n    global o\n    o = mezcla(3, 9, 2) + mezcla(9, 3, 1)\n",
    // comparaciones con signo y booleanos
    "ok: bool = False\nn: uint8 = 0\n\n\ndef main() -> None:\n    global ok, n\n    a: int8 = -5\n    b: int8 = 3\n    ok = a < b and not (a == b)\n    if a < 0 or b < 0:\n        n = 1\n    if a >= b:\n        n = 2\n",
    // UART: enviar, formato, leer una linea y convertir
    "from pyxpic.periph.uart import UART\n\nCLOCK_HZ: Const[uint32] = 4_000_000\nbuf: uint8[8]\nv: uint16 = 0\nbad: bool = True\n\n\ndef main() -> None:\n    global v, bad\n    UART.start(baud=9600)\n    n: uint8 = UART.read_line(buf)\n    v = parse_dec(buf, n, bad)\n    UART.write_text(f\"{v} 0x{v:04X}\\r\\n\")\n",
    // varias lecturas de una misma variable que cambia entre medias
    "g: uint8 = 1\nh: uint8 = 0\nk: uint8 = 0\n\n\ndef main() -> None:\n    global g, h, k\n    t: uint8 = g\n    g = t + 4\n    u: uint8 = g\n    g = u + t\n    h = t\n    k = u\n",
    // constantes plegables con tipos distintos
    "a: uint16 = 0\nb: int16 = 0\n\n\ndef main() -> None:\n    global a, b\n    x: uint8 = 255\n    a = uint16(x) + 1\n    b = int16(-300) + int16(x)\n",
];

#[test]
fn the_optimized_program_behaves_exactly_like_the_original() {
    for src in CORPUS {
        let original = observe(&raw(src));
        let (opt, _) = optimized_verifying_each_pass(src);
        assert_eq!(observe(&opt), original, "difiere en:\n{src}\n--- optimizada ---\n{}", print(&opt));
    }
}

#[test]
fn every_shipped_example_still_verifies_after_each_pass_and_shrinks_or_stays_the_same() {
    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples");
    let mut files = Vec::new();
    let mut stack = vec![root];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(d).unwrap().flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().is_some_and(|x| x == "py") {
                files.push(p);
            }
        }
    }
    assert!(files.len() > 10);
    let (mut before, mut after) = (0usize, 0usize);
    for f in files {
        let src = std::fs::read_to_string(&f).unwrap();
        // los ejemplos con un dispositivo declarado se analizan con el perfil generico: basta para la IR
        let r = analyze(&src, &TargetProfile::generic());
        let Some(p) = r.program else { continue };
        let m0 = lower_unoptimized(&p);
        let n0: usize = m0.functions.iter().flat_map(|f| &f.blocks).map(|b| b.insts.len() + 1).sum();
        let mut m = lower_unoptimized(&p);
        let mut st = Stats::default();
        for _ in 0..16 {
            let b = st;
            opt::propagate(&mut m, &mut st);
            verify(&m).unwrap_or_else(|e| panic!("{}: verify tras propagate: {e:?}", f.display()));
            opt::simplify_cfg(&mut m, &mut st);
            verify(&m).unwrap_or_else(|e| panic!("{}: verify tras simplify_cfg: {e:?}", f.display()));
            opt::eliminate_dead(&mut m, &mut st);
            verify(&m).unwrap_or_else(|e| panic!("{}: verify tras eliminate_dead: {e:?}", f.display()));
            if st == b {
                break;
            }
        }
        opt::compact_temps(&mut m, &mut st);
        verify(&m).unwrap_or_else(|e| panic!("{}: verify tras compact_temps: {e:?}", f.display()));
        let n1: usize = m.functions.iter().flat_map(|f| &f.blocks).map(|b| b.insts.len() + 1).sum();
        assert!(n1 <= n0, "{}: la IR no puede crecer ({n0} -> {n1})", f.display());
        before += n0;
        after += n1;
    }
    assert!(after < before, "algo se optimiza en los ejemplos: {before} -> {after}");
}
