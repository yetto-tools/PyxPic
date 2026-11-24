//! Arreglos estaticos de RAM (`buf: uint8[16]`, `buf[i]`, `buf[i] = v`; #8). Diferenciales contra el interprete de la IR,
//! indice variable saturado, FSR preservado ante interrupciones, arranque en cero y las reglas del lenguaje.

use std::path::PathBuf;

use pypic_device::DevicePack;
use pypic_frontend::analyze;
use pypic_ir::{lower, verify, Machine, Module};
use pypic_pic14::codegen::Compiled;
use pypic_pic14::sim::Sim;
use pypic_pic14::{compile, Options};

fn pack() -> DevicePack {
    DevicePack::load(&PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../devices/pic16f887")).unwrap()
}

fn build(src: &str) -> (Module, Compiled) {
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

fn head() -> String {
    "from pypic.device import PIC16F887\nCLOCK_HZ: Const[uint32] = 4_000_000\n".to_string()
}

fn pic_array(c: &Compiled, s: &Sim, name: &str) -> Vec<u8> {
    let a = c.arrays.iter().find(|a| a.name == name).unwrap();
    (0..a.len).map(|k| s.read(a.addr as usize + k)).collect()
}

fn pic_global(c: &Compiled, s: &Sim, name: &str) -> i128 {
    let g = c.globals.iter().find(|g| g.name == name).unwrap();
    let n = g.ty.bits().max(8) as usize / 8;
    g.ty.wrap((0..n).map(|i| (s.read(g.addr as usize + i) as i128) << (8 * i)).sum())
}

/// Compila `body` (con sus declaraciones), corre el PIC y el interprete y exige los mismos globales y arreglos.
fn same(src: &str) -> (Machine<'static>, Compiled, Sim) {
    let (m, c) = build(&format!("{}{src}", head()));
    let m: &'static Module = Box::leak(Box::new(m));
    let mut mach = Machine::new(m);
    mach.run_main().unwrap();
    let mut s = Sim::new(&c.image);
    s.run_until(50_000_000, |s| s.halted);
    assert!(s.halted, "el programa no termino");
    for g in m.globals.iter().filter(|g| !g.is_const) {
        assert_eq!(pic_global(&c, &s, &g.name), mach.global(&g.name).unwrap(), "global {}", g.name);
    }
    for (i, (n, _)) in m.arrays.iter().enumerate() {
        assert_eq!(pic_array(&c, &s, n), mach.arrays[i], "arreglo {n}");
    }
    (mach, c, s)
}

// ---- lectura y escritura ----------------------------------------------------------------------------------------

#[test]
fn an_array_starts_at_zero_and_holds_what_is_written_with_variable_and_constant_indices() {
    let (mach, c, s) = same("buf: uint8[16]\ntotal: uint16 = 0\nfirst: uint8 = 99\nlast: uint8 = 99\n\n\ndef main() -> None:\n    global total, first, last\n    first = buf[3]\n    for i in range(16):\n        buf[i] = i * 3\n    for i in range(16):\n        total += uint16(buf[i])\n    last = buf[15]\n    buf[0] = 200\n");
    assert_eq!(pic_global(&c, &s, "first"), 0, "los elementos empiezan en cero");
    assert_eq!(pic_global(&c, &s, "total"), 360);
    assert_eq!(pic_global(&c, &s, "last"), 45);
    assert_eq!(pic_array(&c, &s, "buf")[..4], [200, 3, 6, 9]);
    assert!(mach.array_oob.is_empty());
}

#[test]
fn compound_assignment_reads_modifies_and_writes_with_one_index() {
    let (_, c, s) = same("buf: uint8[8]\nk: uint8 = 5\n\n\ndef main() -> None:\n    global k\n    buf[0] = 200\n    buf[0] += 100\n    buf[1] = 1\n    buf[1] <<= 3\n    buf[2] = 0xF0\n    buf[2] &= 0x3C\n    buf[2] |= 1\n    buf[3] = buf[2] ^ 0xFF\n    buf[k] = 7\n    buf[k] -= 9\n    buf[4] = 3\n    buf[4] *= 5\n");
    assert_eq!(pic_array(&c, &s, "buf"), vec![44, 8, 0x31, 0xCE, 15, 254, 0, 0]);
}

#[test]
fn len_is_the_capacity_and_a_constant() {
    let (_, c, s) = same("buf: uint8[24]\nn: uint8 = 0\n\n\ndef main() -> None:\n    global n\n    n = len(buf)\n");
    assert_eq!(pic_global(&c, &s, "n"), 24);
}

#[test]
fn two_arrays_do_not_overlap() {
    let (_, c, s) = same("a: uint8[5]\nb: uint8[5]\n\n\ndef main() -> None:\n    for i in range(5):\n        a[i] = 0xAA\n    for i in range(5):\n        b[i] = i + 1\n");
    assert_eq!(pic_array(&c, &s, "a"), vec![0xAA; 5]);
    assert_eq!(pic_array(&c, &s, "b"), vec![1, 2, 3, 4, 5]);
}

// ---- indice fuera de rango ----------------------------------------------------------------------------------------

#[test]
fn a_variable_index_past_the_end_is_clamped_and_never_touches_other_memory() {
    let prog = |idx: u8| format!("buf: uint8[4]\nafter: uint8 = 0x77\nidx: uint8 = {idx}\nr: uint8 = 0\n\n\ndef main() -> None:\n    global r\n    buf[3] = 5\n    buf[idx] = 42\n    r = buf[idx]\n");
    let (mach, c, s) = same(&prog(9));
    assert_eq!(pic_array(&c, &s, "buf"), vec![0, 0, 0, 42], "el indice 9 se satura a la ultima posicion");
    assert_eq!(pic_global(&c, &s, "r"), 42);
    assert_eq!(pic_global(&c, &s, "after"), 0x77);
    assert_eq!(mach.array_oob.len(), 2, "el interprete anota los dos accesos fuera de rango: {:?}", mach.array_oob);
    // el estado completo de la RAM es el de un acceso con el indice 3: no hay nada mas escrito
    let (_, c3, s3) = same(&prog(3));
    assert_eq!(c.arrays, c3.arrays);
    let idx_addr = c.globals.iter().find(|g| g.name == "idx").unwrap().addr as usize; // lo unico que difiere: el valor de idx
    for a in (0x20..=0x7F).filter(|&a| a != idx_addr) {
        assert_eq!(s.read(a), s3.read(a), "direccion {a:#04x}");
    }
    // un indice de 255 tambien
    let (_, c2, s2) = same(&prog(255));
    assert_eq!(pic_array(&c2, &s2, "buf"), vec![0, 0, 0, 42]);
}

#[test]
fn an_array_of_one_element_clamps_every_index_to_it() {
    let (_, c, s) = same("one: uint8[1]\ni: uint8 = 200\n\n\ndef main() -> None:\n    one[i] = 9\n");
    assert_eq!(pic_array(&c, &s, "one"), vec![9]);
}

// ---- arranque --------------------------------------------------------------------------------------------------------

#[test]
fn the_startup_zeroes_short_and_long_arrays_even_if_ram_is_garbage() {
    let (_, c) = build(&format!("{}small: uint8[3]\nbig: uint8[40]\ns: uint8 = 1\n\n\ndef main() -> None:\n    global s\n    s = small[0] | small[2] | big[0] | big[39]\n", head()));
    let mut sim = Sim::new(&c.image);
    for a in 0x20..=0x6Fusize {
        sim.write(a, 0xFF); // la RAM no esta definida tras el reset
    }
    sim.run_until(1_000_000, |s| s.halted);
    assert_eq!(pic_array(&c, &sim, "small"), vec![0; 3]);
    assert_eq!(pic_array(&c, &sim, "big"), vec![0; 40]);
    assert_eq!(pic_global(&c, &sim, "s"), 0);
}

// ---- interrupciones: FSR se guarda y se restaura ------------------------------------------------------------------

#[test]
fn an_interrupt_using_arrays_at_any_instruction_leaves_main_untouched() {
    let src = format!("{}from pypic import interrupt\na: uint8[16]\nb: uint8[8]\nk: uint8 = 0\nacc: uint8 = 0\n\n\n@interrupt\ndef isr() -> None:\n    global k\n    b[k] += 1\n    k = (k + 1) & 7\n\n\ndef main() -> None:\n    global acc\n    for i in range(16):\n        a[i] = i + 1\n    for i in range(16):\n        acc += a[i]\n    a[15] = acc\n", head());
    let (_, c) = build(&src);
    let main_addr = c.functions.iter().find(|(n, _)| n == "main").unwrap().1;
    let mut s = Sim::new(&c.image);
    let (mut start, mut steps) = (None, 0u64);
    while !s.halted {
        if start.is_none() && s.pc as u32 == main_addr {
            start = Some(steps);
        }
        s.step();
        steps += 1;
    }
    let start = start.expect("main se alcanza");
    let (want_a, want_acc) = (pic_array(&c, &s, "a"), pic_global(&c, &s, "acc"));
    assert_eq!(want_acc, 136);
    let mut hits = 0;
    for n in start..steps {
        let mut s = Sim::new(&c.image);
        for _ in 0..n {
            s.step();
        }
        s.interrupt();
        s.run_until(1_000_000, |s| s.halted);
        assert!(s.halted);
        assert_eq!(pic_array(&c, &s, "a"), want_a, "interrupcion tras {n} instrucciones");
        assert_eq!(pic_global(&c, &s, "acc"), want_acc, "interrupcion tras {n} instrucciones");
        hits += pic_array(&c, &s, "b").iter().map(|&x| x as u32).sum::<u32>();
    }
    assert!(hits > 0, "la interrupcion si escribio en su arreglo");
    // una interrupcion que no usa arreglos no gasta el byte de FSR en su prologo
    let (_, plain) = build(&format!("{}from pypic import interrupt\nn: uint8 = 0\n\n\n@interrupt\ndef isr() -> None:\n    global n\n    n += 1\n\n\ndef main() -> None:\n    pass\n", head()));
    let (_, with_fsr) = build(&format!("{}from pypic import interrupt\nb: uint8[8]\nn: uint8 = 0\n\n\n@interrupt\ndef isr() -> None:\n    global n\n    n += 1\n    b[0] = n\n\n\ndef main() -> None:\n    pass\n", head()));
    assert!(with_fsr.listing.contains("FSR") || with_fsr.flash_words > plain.flash_words);
}

// ---- reglas del lenguaje -----------------------------------------------------------------------------------------------

#[test]
fn bad_arrays_are_located_errors() {
    let h = head();
    let run = |decl: &str, body: &str| diagnostics(&format!("{h}{decl}\n\n\ndef main() -> None:\n{body}"));
    for (decl, body, code, frag) in [
        ("", "    b: uint8[4]\n", "PYP2170", "nivel de modulo"),
        ("x: uint16[4]", "    pass\n", "PYP2170", "uint8"),
        ("x: int8[4]", "    pass\n", "PYP2170", "uint8"),
        ("x: uint8[0]", "    pass\n", "PYP2171", "1 a 255"),
        ("x: uint8[256]", "    pass\n", "PYP2171", "1 a 255"),
        ("x: uint8[4] = 0", "    pass\n", "PYP2170", "se inicializa"),
        ("buf: uint8[16]", "    buf[16] = 1\n", "PYP2161", "fuera del arreglo"),
        ("buf: uint8[16]", "    x: uint8 = buf[16]\n", "PYP2161", "fuera del arreglo"),
        ("buf: uint8[16]", "    x: uint8 = buf\n", "PYP2172", "es un arreglo"),
        ("buf: uint8[16]", "    buf = 3\n", "PYP2172", "es un arreglo"),
        ("buf: uint8[16]\nn: uint16 = 0", "    buf[n] = 1\n", "PYP2163", "uint8"),
        ("buf: uint8[16]\nn: uint16 = 0", "    buf[0] = n\n", "PYP2", ""),
        ("T: Rom[uint8] = [1, 2, 3]", "    T[0] = 1\n", "PYP2162", "solo lectura"),
        ("n: uint8 = 0", "    n[0] = 1\n", "PYP2162", "no es un arreglo"),
        ("buf: uint8[4]", "    buf[1]\n", "PYP1011", "no tiene efecto"),
        ("buf: uint8[4]\nbuf: uint8[4]", "    pass\n", "PYP2009", "ya esta declarado"),
        ("buf: uint8[4]\nbuf2: uint8 = 1", "    buf2[0] = 1\n", "PYP2162", "no es un arreglo"),
    ] {
        let (ok, t) = run(decl, body);
        assert!(!ok && t.contains(code) && t.contains(frag), "{decl} / {body}: {t}");
    }
    let (ok, t) = diagnostics(&format!("{h}def f(b: uint8[4]) -> None:\n    pass\n\n\ndef main() -> None:\n    pass\n"));
    assert!(!ok && t.contains("PYP2001"), "{t}");
    let (ok, t) = diagnostics(&format!("{h}buf: uint8[4]\nbuf: uint16 = 1\n\n\ndef main() -> None:\n    pass\n"));
    assert!(!ok && t.contains("PYP2009"), "{t}");
}

#[test]
fn an_array_that_does_not_fit_in_ram_is_a_codegen_error_not_silent() {
    let r = analyze(&format!("{}big: uint8[255]\n\n\ndef main() -> None:\n    big[0] = 1\n", head()), &pack().target_profile());
    let m = lower(&r.program.expect("el frontend lo acepta: la capacidad cabe en un indice uint8"));
    let err = compile(&m, &Options::default()).err().expect("no cabe en el banco 0").to_string();
    assert!(err.contains("RAM"), "{err}");
}

// ---- costo ---------------------------------------------------------------------------------------------------------

fn cycles_of(stmt: &str) -> u64 {
    let src = format!("{}from pypic.periph.gpio import RD0\nbuf: uint8[16]\nidx: uint8 = 5\nv: uint8 = 0\n\n\ndef main() -> None:\n    global v\n    RD0.output()\n    RD0.on()\n    {stmt}\n    RD0.off()\n", head());
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
    down.unwrap() - up.unwrap() - 1
}

#[test]
fn the_costs_are_fixed_and_documented() {
    // un indice constante es un acceso directo; uno variable lleva el recorte y FSR, siempre los mismos ciclos
    assert_eq!(cycles_of("v = buf[3]"), COST_GET_CONST);
    assert_eq!(cycles_of("buf[3] = 7"), COST_SET_CONST);
    assert_eq!(cycles_of("v = buf[idx]"), COST_GET_VAR);
    assert_eq!(cycles_of("buf[idx] = 7"), COST_SET_VAR);
    assert_eq!(cycles_of("buf[idx] += 1"), COST_ADD_VAR);
    // el costo no depende del valor del indice (en rango o no)
    let var = |i: u8| {
        let src = format!("{}from pypic.periph.gpio import RD0\nbuf: uint8[16]\nidx: uint8 = {i}\nv: uint8 = 0\n\n\ndef main() -> None:\n    global v\n    RD0.output()\n    RD0.on()\n    v = buf[idx]\n    RD0.off()\n", head());
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
        down.unwrap() - up.unwrap() - 1
    };
    assert!([0u8, 7, 15, 16, 100, 255].iter().all(|&i| var(i) == COST_GET_VAR));
}

const COST_GET_CONST: u64 = 2;
const COST_SET_CONST: u64 = 2;
const COST_GET_VAR: u64 = 9;
const COST_SET_VAR: u64 = 9;
const COST_ADD_VAR: u64 = 21;

#[test]
fn the_moving_average_example_converges_to_the_input_and_drives_rd0() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let src = std::fs::read_to_string(root.join("examples/media_movil.py")).unwrap();
    let r = analyze(&src, &pack().target_profile());
    let p = r.program.unwrap_or_else(|| panic!("{}", r.diagnostics.render(&src, "media_movil.py")));
    let c = compile(&lower(&p), &Options::default()).unwrap();
    for (input, avg, rd0) in [(800u16, 200u8, true), (100, 25, false), (1023, 255, true)] {
        let mut s = Sim::new(&c.image);
        s.adc_inputs[0] = input;
        s.run_until(60_000, |_| false);
        assert_eq!(pic_global(&c, &s, "media"), avg as i128, "con {input} en AN0");
        assert_eq!(s.pin('D', 0), Some(rd0), "RD0 con {input}");
        assert!(s.adc_violations.is_empty(), "{:?}", s.adc_violations);
    }
}
