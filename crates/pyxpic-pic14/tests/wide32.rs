//! Enteros de 32 bits (`uint32`, `int32`; #32): el PIC y el interprete de la IR deben dar los mismos globales para suma, resta, bits,
//! desplazamientos, comparaciones, multiplicacion, division, modulo, negacion y conversiones entre anchos. Los costos y la RAM de las
//! rutinas de 4 bytes estan en `docs/sdk-runtime.md`.

use std::path::PathBuf;

use pyxpic_device::DevicePack;
use pyxpic_frontend::analyze;
use pyxpic_ir::{lower, verify, Machine, Module};
use pyxpic_pic14::codegen::Compiled;
use pyxpic_pic14::sim::Sim;
use pyxpic_pic14::{compile, Options};

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

const HEAD: &str = "from pyxpic.device import PIC16F887\nCLOCK: Internal\nFREQUENCY: 4MHz\n";

fn pic_global(c: &Compiled, s: &Sim, name: &str) -> i128 {
    let g = c.globals.iter().find(|g| g.name == name).unwrap();
    let n = g.ty.bits().max(8) as usize / 8;
    g.ty.wrap((0..n).map(|i| (s.read(g.addr as usize + i) as i128) << (8 * i)).sum())
}

/// Corre el programa en el PIC y en el interprete y exige los mismos globales.
fn same(src: &str) -> (Compiled, Sim) {
    let (m, c) = build(&format!("{HEAD}{src}"));
    let m: &'static Module = Box::leak(Box::new(m));
    let mut mach = Machine::new(m);
    mach.run_main().unwrap();
    let mut s = Sim::new(&c.image);
    s.run_until(200_000_000, |s| s.halted);
    assert!(s.halted, "el programa no termino");
    for g in m.globals.iter().filter(|g| !g.is_const) {
        assert_eq!(pic_global(&c, &s, &g.name), mach.global(&g.name).unwrap(), "global {} en:\n{src}", g.name);
    }
    (c, s)
}

const U: [u64; 9] = [0, 1, 2, 255, 256, 65_535, 65_536, 0x1234_5678, 0xFFFF_FFFF];
const S: [i64; 9] = [0, 1, -1, 127, -128, 32_767, -32_768, 0x1234_5678, -0x7FFF_FFFF];

#[test]
fn thirty_two_bit_add_sub_bits_and_compare_match_the_ir() {
    for &a in &U {
        for &b in &[0u64, 1, 255, 256, 65_536, 0xFFFF_FFFF, 0x8000_0000] {
            let (c, s) = same(&format!(
                "a: uint32 = {a}\nb: uint32 = {b}\nsum: uint32 = 0\ndif: uint32 = 0\nand_: uint32 = 0\nor_: uint32 = 0\nxor_: uint32 = 0\nlt: uint8 = 0\nge: uint8 = 0\neq: uint8 = 0\nne: uint8 = 0\n\n\ndef main() -> None:\n    global sum, dif, and_, or_, xor_, lt, ge, eq, ne\n    sum = a + b\n    dif = a - b\n    and_ = a & b\n    or_ = a | b\n    xor_ = a ^ b\n    if a < b:\n        lt = 1\n    if a >= b:\n        ge = 1\n    if a == b:\n        eq = 1\n    if a != b:\n        ne = 1\n"
            ));
            assert_eq!(pic_global(&c, &s, "sum"), ((a + b) & 0xFFFF_FFFF) as i128, "{a} + {b}");
        }
    }
}

#[test]
fn thirty_two_bit_signed_compare_negate_and_shifts_match_the_ir() {
    for &a in &S {
        for &b in &[0i64, 1, -1, 100, -100, 0x7FFF_FFFF, -0x7FFF_FFFF] {
            same(&format!(
                "a: int32 = {a}\nb: int32 = {b}\nlt: uint8 = 0\nle: uint8 = 0\ngt: uint8 = 0\nneg: int32 = 0\ninv: int32 = 0\nsum: int32 = 0\n\n\ndef main() -> None:\n    global lt, le, gt, neg, inv, sum\n    if a < b:\n        lt = 1\n    if a <= b:\n        le = 1\n    if a > b:\n        gt = 1\n    neg = -a\n    inv = ~a\n    sum = a + b\n"
            ));
        }
    }
    for &v in &[0x1234_5678u64, 0xFFFF_FFFF, 0x8000_0001, 1] {
        for n in [0, 1, 7, 8, 9, 15, 16, 17, 24, 31] {
            same(&format!("u: uint32 = {v}\ni: int32 = {}\nl: uint32 = 0\nr: uint32 = 0\nar: int32 = 0\n\n\ndef main() -> None:\n    global l, r, ar\n    l = u << {n}\n    r = u >> {n}\n    ar = i >> {n}\n", v as u32 as i32));
        }
    }
    // cantidad variable
    for n in [0u8, 1, 9, 31, 32, 40] {
        same(&format!("u: uint32 = 0x80000001\nn: uint8 = {n}\nl: uint32 = 0\nr: uint32 = 0\n\n\ndef main() -> None:\n    global l, r\n    l = u << n\n    r = u >> n\n"));
    }
}

#[test]
fn thirty_two_bit_multiply_divide_and_modulo_match_the_ir_including_zero() {
    for &a in &U {
        for &b in &[0u64, 1, 3, 255, 65_537, 0x1000_0001, 0xFFFF_FFFF] {
            same(&format!("a: uint32 = {a}\nb: uint32 = {b}\nm: uint32 = 0\nq: uint32 = 0\nr: uint32 = 0\n\n\ndef main() -> None:\n    global m, q, r\n    m = a * b\n    q = a // b\n    r = a % b\n"));
        }
    }
    for &a in &S {
        for &b in &[0i64, 1, -1, 3, -7, 32_767, -0x7FFF_FFFF] {
            same(&format!("a: int32 = {a}\nb: int32 = {b}\nm: int32 = 0\nq: int32 = 0\nr: int32 = 0\n\n\ndef main() -> None:\n    global m, q, r\n    m = a * b\n    q = a // b\n    r = a % b\n"));
        }
    }
}

#[test]
fn conversions_between_widths_extend_with_the_sign_and_truncate() {
    for &v in &[0i64, 1, -1, 127, -128, 255, 32_767, -32_768, 65_535, 0x1234_5678, -0x1234_5678] {
        same(&format!("i: int32 = {v}\nu: uint32 = {}\nb: uint8 = 0\nh: uint16 = 0\ns: int16 = 0\nw: int32 = 0\nz: uint32 = 0\n\n\ndef main() -> None:\n    global b, h, s, w, z\n    b = uint8(i)\n    h = uint16(u)\n    s = int16(i)\n    w = int32(s)\n    z = uint32(h)\n", v as u32));
    }
}

#[test]
fn a_thirty_two_bit_variable_takes_four_bytes_of_ram_and_a_constant_none() {
    let (c, s) = same("big: uint32 = 3_000_000_000\nsmall: uint8 = 7\nK: Const[uint32] = 70_000\n\n\ndef main() -> None:\n    global big\n    big = big + K\n");
    assert_eq!(pic_global(&c, &s, "big"), 3_000_070_000);
    let g = c.globals.iter().find(|g| g.name == "big").unwrap();
    let s8 = c.globals.iter().find(|g| g.name == "small").unwrap();
    assert_eq!(s8.addr as usize - g.addr as usize, 4, "uint32 ocupa 4 bytes");
}

#[test]
fn a_function_takes_and_returns_thirty_two_bit_values() {
    for (a, b) in [(1u64, 2u64), (0xFFFF_FFFF, 1), (0x1234_5678, 0x0F0F_0F0F), (100_000, 3)] {
        let (c, s) = same(&format!("r: uint32 = 0\nq: int32 = 0\n\n\ndef add(x: uint32, y: uint32) -> uint32:\n    return x + y\n\n\ndef neg(x: int32) -> int32:\n    return -x\n\n\ndef main() -> None:\n    global r, q\n    r = add({a}, {b})\n    q = neg(-{b})\n"));
        assert_eq!(pic_global(&c, &s, "r"), ((a + b) & 0xFFFF_FFFF) as i128);
    }
}

/// Ciclos de una operacion: lo que dura el programa con ella menos el programa sin ella (misma lectura y escritura de globales).
fn cycles_of(ty: &str, op: &str, a: i64, b: i64) -> u64 {
    let run = |body: &str| -> u64 {
        let (_, c) = build(&format!("{HEAD}a: {ty} = {a}\nb: {ty} = {b}\nr: {ty} = 0\n\n\ndef main() -> None:\n    global r\n    {body}\n"));
        let mut s = Sim::new(&c.image);
        s.run_until(200_000_000, |s| s.halted);
        s.cycles
    };
    run(&format!("r = a {op} b")) - run("r = a")
}

#[test]
fn the_thirty_two_bit_routines_have_the_documented_cost() {
    // la multiplicacion es de tiempo constante; la division recorre 32 pasos y depende de los datos
    let m1 = cycles_of("uint32", "*", 1, 1);
    let m2 = cycles_of("uint32", "*", 0xFFFF_FFFF, 0xFFFF_FFFF);
    assert_eq!(m1, m2, "mul32 es de tiempo constante");
    let (dmin, dmax) = (cycles_of("uint32", "//", 0, 1), cycles_of("uint32", "//", 0xFFFF_FFFF, 1));
    let (smin, smax) = (cycles_of("int32", "//", 5, 0), cycles_of("int32", "//", -0x7FFF_FFFF, 3));
    eprintln!("mul32 = {m1} ciclos; divmod32 = {dmin}..{dmax}; sdivmod32 = {smin}..{smax}");
    assert_eq!(m1, 1817, "mul32");
    assert!((1100..=2500).contains(&dmin) && (1100..=2500).contains(&dmax), "divmod32: {dmin}..{dmax}");
    assert!(smin < 100 && smax <= 2600, "sdivmod32: {smin}..{smax}");
}
