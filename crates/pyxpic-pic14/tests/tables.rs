//! Tablas constantes en la memoria de programa (#44): `T: Rom[u8] = [..]`, `T[i]`, `len(T)`.

use std::path::PathBuf;

use pyxpic_device::DevicePack;
use pyxpic_frontend::{analyze, TargetProfile};
use pyxpic_ir::{lower, verify, Machine};
use pyxpic_pic14::codegen::Compiled;
use pyxpic_pic14::sim::Sim;
use pyxpic_pic14::{compile, Options};

fn pack_target() -> TargetProfile {
    DevicePack::load(&PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../devices/pic16f887")).unwrap().target_profile()
}

fn build(src: &str) -> (pyxpic_ir::Module, Compiled) {
    let r = analyze(src, &pack_target());
    let p = r.program.unwrap_or_else(|| panic!("frontend:\n{}\n{src}", r.diagnostics.render(src, "t.py")));
    let m = lower(&p);
    verify(&m).unwrap_or_else(|e| panic!("IR invalida: {e:?}"));
    let c = compile(&m, &Options::default()).unwrap_or_else(|e| panic!("codegen: {e}\n{src}"));
    (m, c)
}

/// Ejecuta en la IR y en el simulador, compara todas las globales y devuelve las del simulador.
fn differential(src: &str) -> (std::collections::BTreeMap<String, i128>, Compiled, Sim) {
    let (m, c) = build(src);
    let mut mach = Machine::new(&m);
    mach.run_main().unwrap();
    let mut s = Sim::new(&c.image);
    s.run_until(20_000_000, |s| s.halted);
    assert!(s.halted, "el programa no termino\n{src}");
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

fn errors(src: &str) -> Vec<String> {
    let r = analyze(src, &pack_target());
    assert!(r.program.is_none(), "debia rechazarse:\n{src}");
    r.diagnostics.render(src, "t.py").lines().filter_map(|l| l.split("error[").nth(1).map(String::from)).collect()
}

const HEAD: &str = "from pyxpic.device import PIC16F887\n";

/// Direccion de la primera entrada de la tabla `name` segun el listado.
fn first_entry(c: &Compiled, name: &str) -> u32 {
    let l = c.listing.lines().collect::<Vec<_>>();
    let i = l.iter().position(|x| *x == format!("rom.{name}:")).unwrap_or_else(|| panic!("no hay Rom {name}:\n{}", c.listing));
    u32::from_str_radix(&l[i + 1][..4], 16).unwrap() + 1
}

// ---- semantica -------------------------------------------------------------------------------------

#[test]
fn a_table_is_read_with_a_runtime_index() {
    let src =
        format!("{HEAD}T: Rom[u8] = [10, 20, 30, 40, 50]\nsum: u16 = 0\nx: u8 = 0\ndef main() -> None:\n    global sum, x\n    for i in range(5):\n        sum += u16(T[i])\n        x = x ^ T[i]\n");
    let (g, ..) = differential(&src);
    assert_eq!((g["sum"], g["x"]), (150, 26)); // 26 = 10 ^ 20 ^ 30 ^ 40 ^ 50
}

#[test]
fn the_table_data_takes_no_ram() {
    let small = format!("{HEAD}T: Rom[u8] = [1]\nv: u8 = 0\nk: u8 = 0\ndef main() -> None:\n    global v\n    v = T[k]\n");
    let big_vals = (0..200).map(|i| i.to_string()).collect::<Vec<_>>().join(", ");
    let big = format!("{HEAD}T: Rom[u8] = [{big_vals}]\nv: u8 = 0\nk: u8 = 0\ndef main() -> None:\n    global v\n    v = T[k]\n");
    assert_eq!(build(&small).1.ram_bytes, build(&big).1.ram_bytes);
}

#[test]
fn an_index_past_the_end_returns_the_last_entry() {
    let src = format!("{HEAD}T: Rom[u8] = [1, 2, 3]\na: u8 = 0\nb: u8 = 0\nc: u8 = 0\nidx: u8 = 200\ndef main() -> None:\n    global a, b, c, idx\n    a = T[idx]\n    idx = 3\n    b = T[idx]\n    idx = 2\n    c = T[idx]\n");
    let (g, ..) = differential(&src);
    assert_eq!((g["a"], g["b"], g["c"]), (3, 3, 3));
}

#[test]
fn a_constant_index_is_resolved_at_compile_time() {
    let src = format!("{HEAD}T: Rom[u8] = [7, 8, 9]\nv: u8 = 0\ndef main() -> None:\n    global v\n    v = T[1]\n");
    let (g, c, _) = differential(&src);
    assert_eq!(g["v"], 8);
    assert!(!c.listing.contains("rom."), "un indice constante no necesita la rutina\n{}", c.listing);
}

#[test]
fn len_is_a_constant() {
    let src = format!("{HEAD}T: Rom[u8] = [1, 2, 3, 4]\nn: u8 = 0\ndef main() -> None:\n    global n\n    n = len(T)\n    k: u8 = len(T) + 1\n    n += k\n");
    let (g, ..) = differential(&src);
    assert_eq!(g["n"], 9);
}

#[test]
fn a_table_of_256_entries_needs_no_clamp_and_is_aligned() {
    let vals: Vec<String> = (0..256).map(|i| ((i * 7 + 3) % 256).to_string()).collect();
    let src = format!("{HEAD}T: Rom[u8] = [{}]\nsum: u16 = 0\ndef main() -> None:\n    global sum\n    i: u8 = 0\n    while True:\n        sum += u16(T[i])\n        if i == 255:\n            break\n        i += 1\n", vals.join(", "));
    let (g, c, _) = differential(&src);
    let want: i128 = (0..256).map(|i| ((i * 7 + 3) % 256) as i128).sum();
    assert_eq!(g["sum"], want);
    assert_eq!(first_entry(&c, "T") & 0xFF, 0, "256 entradas solo caben alineadas a 256");
}

#[test]
fn several_tables_and_a_lookup_inside_a_function_with_parameters() {
    let src = format!("{HEAD}A: Rom[u8] = [1, 2, 3]\nB: Rom[u8] = [100, 101, 102, 103]\nr: u16 = 0\ndef f(i: u8, j: u8) -> u8:\n    return A[i] + B[j]\n\n\ndef main() -> None:\n    global r\n    r = u16(f(2, 3))\n    r += u16(f(9, 9))\n");
    let (g, ..) = differential(&src);
    assert_eq!(g["r"], (3 + 103) + (3 + 103));
}

#[test]
fn the_lookup_costs_the_same_with_every_index() {
    // el intervalo entre BSF y BCF de RD0 es la consulta + 1 (el BCF)
    let measure = |idx: u8| -> u64 {
        let src = format!("{HEAD}from pyxpic.periph.gpio import RD0\nT: Rom[u8] = [1, 2, 3, 4, 5, 6, 7, 8, 9]\nv: u8 = 0\nk: u8 = {idx}\ndef main() -> None:\n    global v\n    RD0.output()\n    RD0.on()\n    v = T[k]\n    RD0.off()\n");
        let (_, c) = build(&src);
        let mut s = Sim::new(&c.image);
        let (mut up, mut down, mut last) = (None, None, None);
        while !s.halted && s.cycles < 100_000 {
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
        down.unwrap() - up.unwrap()
    };
    let costs: Vec<u64> = [0u8, 1, 4, 8, 9, 100, 255].iter().map(|&i| measure(i)).collect();
    assert!(costs.windows(2).all(|w| w[0] == w[1]), "{costs:?}");
    // MOVF + 4 de comprobacion + MOVWF + MOVLW + MOVWF + MOVF + CALL(2) + ADDWF(2) + RETLW(2) + MOVWF + CLRF + el BCF final
    assert_eq!(costs[0], 1 + 4 + 1 + 1 + 1 + 1 + 2 + 2 + 2 + 1 + 1 + 1, "{costs:?}");
}

#[test]
fn a_table_that_would_cross_a_256_word_boundary_is_moved_and_still_correct() {
    // se barre el tamano del codigo anterior para que la tabla caiga a ambos lados del limite
    let mut aligned = 0;
    let mut starts = Vec::new();
    for filler in 0..90 {
        let body: String = (0..filler).map(|_| "    pad += 1\n").collect();
        let src = format!(
            "{HEAD}T: Rom[u8] = [{}]\npad: u8 = 0\nsum: u16 = 0\ndef main() -> None:\n    global pad, sum\n{body}    for i in range(40):\n        sum += u16(T[i])\n",
            (1..=40).map(|v| v.to_string()).collect::<Vec<_>>().join(", ")
        );
        let (g, c, _) = differential(&src);
        assert_eq!(g["sum"], (1..=40).sum::<i128>(), "relleno {filler}");
        let first = first_entry(&c, "T");
        assert!((first & 0xFF) + 40 <= 256, "relleno {filler}: la tabla cruza un limite de 256 ({first:#x})");
        if first & 0xFF == 0 {
            aligned += 1;
        }
        starts.push(first);
    }
    assert!(starts.iter().any(|a| *a < 0x100) && starts.iter().any(|a| *a >= 0x100), "el barrido debe pasar el limite 0x100: {starts:x?}");
    assert!(aligned >= 1, "al menos un caso tuvo que alinearse");
}

// ---- reglas del lenguaje ----------------------------------------------------------------------------

#[test]
fn a_table_declaration_is_checked() {
    // Rom ya es de solo lectura: no hace falta Const, y la forma antigua Table no existe
    assert!(analyze(&format!("{HEAD}T: Rom[u8] = [1]\ndef main() -> None:\n    pass\n"), &pack_target()).program.is_some());
    assert!(analyze(&format!("{HEAD}T: Const[Rom[u8]] = [1]\ndef main() -> None:\n    pass\n"), &pack_target()).program.is_some());
    let e = errors(&format!("{HEAD}T: Table[u8] = [1]\ndef main() -> None:\n    pass\n"));
    assert!(!e.is_empty(), "la forma antigua Table[u8] ya no existe: {e:?}");
    let e = errors(&format!("{HEAD}T: Rom[u8] = [1, 300]\ndef main() -> None:\n    pass\n"));
    assert!(e[0].starts_with("PYP2160") && e[0].contains("300"), "{e:?}");
    let e = errors(&format!("{HEAD}T: Rom[u8] = []\ndef main() -> None:\n    pass\n"));
    assert!(e[0].starts_with("PYP2160"), "{e:?}");
    let many = (0..257).map(|i| (i % 256).to_string()).collect::<Vec<_>>().join(",");
    let e = errors(&format!("{HEAD}T: Rom[u8] = [{many}]\ndef main() -> None:\n    pass\n"));
    assert!(e[0].starts_with("PYP2160") && e[0].contains("257"), "{e:?}");
    let e = errors(&format!("{HEAD}T: Rom[u16] = [1]\ndef main() -> None:\n    pass\n"));
    assert!(e[0].starts_with("PYP2160") && e[0].contains("uint8"), "{e:?}");
    let e = errors(&format!("{HEAD}x: u8 = 3\nT: Rom[u8] = [x]\ndef main() -> None:\n    pass\n"));
    assert!(e[0].starts_with("PYP2160") && e[0].contains("constantes"), "{e:?}");
    let e = errors(&format!("{HEAD}def main() -> None:\n    T: Rom[u8] = [1]\n"));
    assert!(e[0].starts_with("PYP2160") && e[0].contains("modulo"), "{e:?}");
    let e = errors(&format!("{HEAD}T: Rom[u8] = 5\ndef main() -> None:\n    pass\n"));
    assert!(e.iter().any(|m| m.starts_with("PYP2160") && m.contains("lista")), "{e:?}");
}

#[test]
fn indexing_rules() {
    let t = "T: Rom[u8] = [1, 2, 3]\n";
    let e = errors(&format!("{HEAD}{t}x: u8 = T[3]\ndef main() -> None:\n    pass\n"));
    assert!(e.iter().any(|m| m.starts_with("PYP2161") && m.contains("0..=2")), "{e:?}");
    let e = errors(&format!("{HEAD}{t}k: u16 = 1\ndef main() -> None:\n    x: u8 = T[k]\n"));
    assert!(e[0].starts_with("PYP2163") && e[0].contains("uint8"), "{e:?}");
    let e = errors(&format!("{HEAD}{t}k: i8 = 1\ndef main() -> None:\n    x: u8 = T[k]\n"));
    assert!(e[0].starts_with("PYP2163"), "{e:?}");
    let e = errors(&format!("{HEAD}{t}n: u8 = 1\ndef main() -> None:\n    x: u8 = n[0]\n"));
    assert!(e[0].starts_with("PYP2162"), "{e:?}");
    let e = errors(&format!("{HEAD}{t}def main() -> None:\n    x: u8 = T\n"));
    assert!(e[0].starts_with("PYP2164") && e[0].contains("len(T)"), "{e:?}");
    let e = errors(&format!("{HEAD}{t}def main() -> None:\n    x: u8 = len(5)\n"));
    assert!(e[0].starts_with("PYP2162"), "{e:?}");
    let e = errors(&format!("{HEAD}def main() -> None:\n    x: u8 = [1, 2]\n"));
    assert!(e.iter().any(|m| m.starts_with("PYP1025")), "{e:?}");
}

#[test]
fn a_table_lookup_counts_one_stack_level() {
    // main (1 de arranque) + f1..f6 = 7 niveles; la consulta del ultimo es el octavo
    let chain = |with_table: bool| {
        let mut s = format!("{HEAD}T: Rom[u8] = [1, 2]\nr: u8 = 0\nk: u8 = 0\n");
        s.push_str(&format!("def f6() -> None:\n    global r\n    {}\n\n\n", if with_table { "r = T[k]" } else { "r = 1" }));
        for i in (1..6).rev() {
            s.push_str(&format!("def f{i}() -> None:\n    f{}()\n\n\n", i + 1));
        }
        s.push_str("def main() -> None:\n    f1()\n");
        s
    };
    let src = chain(true);
    let r = analyze(&src, &pack_target());
    assert!(r.program.is_some(), "8 niveles caben: {}", r.diagnostics.render(&src, "t.py"));
    // un nivel mas de llamadas y la consulta rebasa los 8 de la pila
    let deeper = src.replace("def main() -> None:\n    f1()\n", "def g() -> None:\n    f1()\n\n\ndef main() -> None:\n    g()\n");
    let r = analyze(&deeper, &pack_target());
    assert!(r.program.is_none() && r.diagnostics.render(&deeper, "t.py").contains("PYP2061"), "{}", r.diagnostics.render(&deeper, "t.py"));
    // sin la tabla, la misma cadena de 9 llamadas... 8 niveles cabe
    let plain = chain(false).replace("def main() -> None:\n    f1()\n", "def g() -> None:\n    f1()\n\n\ndef main() -> None:\n    g()\n");
    let r = analyze(&plain, &pack_target());
    assert!(r.program.is_some(), "{}", r.diagnostics.render(&plain, "t.py"));
}
