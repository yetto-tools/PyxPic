//! Punto fijo (`Fixed[T, n]` con escala decimal 10^n y `QFixed[T, n]` con escala binaria 2^n): diferenciales contra el interprete de la IR
//! (el PIC y el interprete deben dar los mismos globales), los valores exactos de cada operacion y los errores del verificador. En el PIC todo
//! son enteros: sumas y comparaciones son las del entero; el producto y las conversiones usan un intermedio de 32 bits.

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

fn diagnostics(src: &str) -> (bool, String) {
    let r = analyze(src, &pack().target_profile());
    (r.program.is_some(), r.diagnostics.render(src, "t.py"))
}

const HEAD: &str = "from pyxpic.device import PIC16F887\nCLOCK: Internal\nFREQUENCY: 4MHz\n";

fn pic_global(c: &Compiled, s: &Sim, name: &str) -> i128 {
    let g = c.globals.iter().find(|g| g.name == name).unwrap();
    let n = g.ty.bits().max(8) as usize / 8;
    g.ty.wrap((0..n).map(|i| (s.read(g.addr as usize + i) as i128) << (8 * i)).sum())
}

/// Compila el programa, lo corre en el PIC y en el interprete de la IR y exige los mismos globales. Devuelve el PIC para leer valores.
fn same(src: &str) -> (Compiled, Sim) {
    let (m, c) = build(&format!("{HEAD}{src}"));
    let m: &'static Module = Box::leak(Box::new(m));
    let mut mach = Machine::new(m);
    mach.run_main().unwrap();
    let mut s = Sim::new(&c.image);
    s.run_until(80_000_000, |s| s.halted);
    assert!(s.halted, "el programa no termino");
    for g in m.globals.iter().filter(|g| !g.is_const) {
        assert_eq!(pic_global(&c, &s, &g.name), mach.global(&g.name).unwrap(), "global {}", g.name);
    }
    (c, s)
}

fn rejected(src: &str, code: &str, frag: &str) {
    let (ok, t) = diagnostics(&format!("{HEAD}{src}"));
    assert!(!ok && t.contains(code) && t.contains(frag), "{src}: {t}");
}

#[test]
fn a_fixed_point_variable_holds_the_scaled_integer_and_adds_like_an_integer() {
    // 25.37 es 2537 con dos decimales; + 1.25 es + 125: 26.62
    let (c, s) = same("t: Fixed[int16, 2] = 25.37\nu: Fixed[int16, 2] = 0\nk: Fixed[int16, 2] = 0.25\n\n\ndef main() -> None:\n    global t, u\n    t = t + 1.25\n    u = t - k\n    t += 1.25\n");
    assert_eq!(pic_global(&c, &s, "t"), 2787); // 25.37 + 1.25 + 1.25 = 27.87
    assert_eq!(pic_global(&c, &s, "u"), 2637); // 26.62 - 0.25
    assert_eq!(pic_global(&c, &s, "k"), 25);
}

#[test]
fn the_product_of_two_fixed_values_rescales_with_a_16_bit_intermediate() {
    // 1.10 V x 0.50 A = 0.55 W con base de 8 bits: 110 * 50 = 5500, entre 100 (cabe en el intermedio de 16 bits)
    let (c, s) = same("voltage: Fixed[uint8, 2] = 1.10\ncurrent: Fixed[uint8, 2] = 0.50\npower: Fixed[uint8, 2] = 0\na: Fixed[uint8, 2] = 1.50\nb: Fixed[uint8, 2] = 1.25\nm: Fixed[uint8, 2] = 0\n\n\ndef main() -> None:\n    global power, m\n    power = voltage * current\n    m = a * b\n");
    assert_eq!(pic_global(&c, &s, "power"), 55);
    assert_eq!(pic_global(&c, &s, "m"), 187); // 1.50 x 1.25 = 1.875: trunca hacia cero
    // con signo: -0.50 x -0.50 = 0.25 y -0.50 x 1.20 = -0.60
    let (c, s) = same("a: Fixed[int8, 2] = -0.50\nb: Fixed[int8, 2] = 1.20\nsq: Fixed[int8, 2] = 0\nm: Fixed[int8, 2] = 0\n\n\ndef main() -> None:\n    global sq, m\n    sq = a * a\n    m = a * b\n");
    assert_eq!(pic_global(&c, &s, "sq"), 25);
    assert_eq!(pic_global(&c, &s, "m"), -60);
}

#[test]
fn qfixed_rescales_with_shifts_and_floors_negatives() {
    // 1.5 en Q4 es 24; 1.5 x 1.5 = 2.25 = 36 y -1.5 x 1.5 = -2.25 = -36
    let (c, s) = same("q: QFixed[int8, 4] = 1.5\nr: QFixed[int8, 4] = 0\nn: QFixed[int8, 4] = -1.5\nm: QFixed[int8, 4] = 0\n\n\ndef main() -> None:\n    global r, m\n    r = q * q\n    m = n * q\n");
    assert_eq!(pic_global(&c, &s, "q"), 24);
    assert_eq!(pic_global(&c, &s, "r"), 36);
    assert_eq!(pic_global(&c, &s, "m"), -36);
}

#[test]
fn multiplying_or_dividing_by_a_constant_is_an_exact_fraction() {
    let (c, s) = same("t: Fixed[int16, 2] = 25.37\na: Fixed[int16, 2] = 0\nb: Fixed[int16, 2] = 0\nd: Fixed[int16, 2] = 0\nu: Fixed[uint16, 2] = 100.00\nh: Fixed[uint16, 2] = 0\nz: Fixed[uint8, 2] = 2.00\nw: Fixed[uint8, 2] = 0\n\n\ndef main() -> None:\n    global a, b, d, h, w\n    a = t * 2\n    b = t * 0.5\n    d = t / 2\n    h = u / 4\n    w = z * 1.25\n");
    assert_eq!(pic_global(&c, &s, "a"), 5074);
    assert_eq!(pic_global(&c, &s, "b"), 1268); // 2537 * 1 / 2 = 1268 (trunca hacia cero)
    assert_eq!(pic_global(&c, &s, "d"), 1268);
    assert_eq!(pic_global(&c, &s, "h"), 2500); // sin signo y potencia de dos: un desplazamiento
    assert_eq!(pic_global(&c, &s, "w"), 250); // 2.00 * 5/4 = 2.50
}

#[test]
fn conversions_between_integers_and_fixed_point_are_explicit_and_exact() {
    let (c, s) = same("n: uint8 = 7\nf: Fixed[int16, 2] = 0\nback: uint8 = 0\nt: Fixed[int16, 2] = 25.37\ncoarse: Fixed[int16, 1] = 0\nq: QFixed[int8, 4] = 1.5\nfrom_q: Fixed[int16, 2] = 0\nneg: Fixed[int16, 2] = -2.5\nneg_int: int8 = 0\n\n\ndef main() -> None:\n    global f, back, coarse, from_q, neg_int\n    f = Fixed[int16, 2](n)\n    back = uint8(t)\n    coarse = Fixed[int16, 1](t)\n    from_q = Fixed[int16, 2](q)\n    neg_int = int8(neg)\n");
    assert_eq!(pic_global(&c, &s, "f"), 700);
    assert_eq!(pic_global(&c, &s, "back"), 25);
    assert_eq!(pic_global(&c, &s, "coarse"), 253); // 25.3: trunca
    assert_eq!(pic_global(&c, &s, "from_q"), 150); // 1.5
    assert_eq!(pic_global(&c, &s, "neg_int"), -2); // hacia cero
}

#[test]
fn comparisons_and_unary_minus_work_on_the_raw_integer() {
    let (c, s) = same("t: Fixed[int16, 2] = 25.37\nbig: uint8 = 0\nsmall: uint8 = 0\neq: uint8 = 0\nneg: Fixed[int16, 2] = 0\n\n\ndef main() -> None:\n    global big, small, eq, neg\n    if t > 25.0:\n        big = 1\n    if t < 2.5:\n        small = 1\n    if t == 25.37:\n        eq = 1\n    neg = -t\n");
    assert_eq!((pic_global(&c, &s, "big"), pic_global(&c, &s, "small"), pic_global(&c, &s, "eq")), (1, 0, 1));
    assert_eq!(pic_global(&c, &s, "neg"), -2537);
}

#[test]
fn fixed_point_constants_fold_at_compile_time_and_cost_nothing() {
    let (c, _) = build(&format!("{HEAD}K: Const[Fixed[int16, 2]] = 1.5 * 3\nR: Const[uint8] = 100 * 0.5\n\n\ndef main() -> None:\n    pass\n"));
    let g = c.globals.iter().find(|g| g.name == "K").unwrap();
    assert!(g.is_const, "una constante no ocupa RAM");
}

#[test]
fn qfixed_constants_round_to_the_nearest_step_and_say_so() {
    let (ok, t) = diagnostics(&format!("{HEAD}q: QFixed[int16, 8] = 0.1\n\n\ndef main() -> None:\n    pass\n"));
    assert!(ok && t.contains("PYP3024") && t.contains("0.101562"), "{t}");
    // 0.5 es exacto: sin aviso
    let (ok, t) = diagnostics(&format!("{HEAD}q: QFixed[int16, 8] = 0.5\n\n\ndef main() -> None:\n    pass\n"));
    assert!(ok && !t.contains("PYP3024"), "{t}");
}

#[test]
fn fixed_point_errors_name_the_rule() {
    // un Fixed guarda multiplos de 10^-n: 0.005 con dos decimales no es exacto
    rejected("t: Fixed[int16, 2] = 0.005\n\n\ndef main() -> None:\n    pass\n", "PYP2252", "no es exacto con 2 decimales");
    rejected("t: Fixed[int16, 2] = 400.00\n\n\ndef main() -> None:\n    pass\n", "PYP2253", "no cabe en Fixed[int16, 2]");
    // tipos o escalas distintos: convertir explicito
    rejected("a: Fixed[int16, 2] = 1.0\nb: Fixed[int16, 3] = 1.0\n\n\ndef main() -> None:\n    x: Fixed[int16, 2] = a + b\n", "PYP2254", "convierte uno");
    // un entero no se mezcla en silencio
    rejected("a: Fixed[int16, 2] = 1.0\nn: uint8 = 3\n\n\ndef main() -> None:\n    x: Fixed[int16, 2] = a + n\n", "PYP2254", "convierte el entero con Fixed[int16, 2](x)");
    rejected("a: Fixed[int16, 2] = 1.0\nn: uint8 = 3\n\n\ndef main() -> None:\n    x: Fixed[int16, 2] = a * n\n", "PYP2254", "se multiplica por otro");
    // asignar un entero a un Fixed
    rejected("n: uint8 = 3\n\n\ndef main() -> None:\n    x: Fixed[int16, 2] = n\n", "PYP2254", "convierte el entero");
    // operadores que no existen en punto fijo
    rejected("a: Fixed[int16, 2] = 1.0\n\n\ndef main() -> None:\n    x: Fixed[int16, 2] = a % 2\n", "PYP2253", "no se aplica al punto fijo");
    rejected("a: Fixed[int16, 2] = 1.0\nb: Fixed[int16, 2] = 2.0\n\n\ndef main() -> None:\n    x: Fixed[int16, 2] = a / b\n", "PYP2253", "divide por una constante");
    // 32 bits de base: sin intermedio de 64
    rejected("a: Fixed[int32, 3] = 1.0\nb: Fixed[int32, 3] = 2.0\n\n\ndef main() -> None:\n    x: Fixed[int32, 3] = a * b\n", "PYP2255", "intermedio");
    // tipo mal formado
    rejected("a: Fixed[bool, 2] = 1.0\n\n\ndef main() -> None:\n    pass\n", "PYP2252", "es un entero");
    rejected("a: Fixed[uint8, 5] = 1.0\n\n\ndef main() -> None:\n    pass\n", "PYP2252", "la escala de uint8 va de 0 a 2");
    rejected("a: QFixed[int16, 16] = 0.5\n\n\ndef main() -> None:\n    pass\n", "PYP2252", "va de 0 a 15");
    // dividir una constante entre un punto fijo
    rejected("a: Fixed[int16, 2] = 1.0\n\n\ndef main() -> None:\n    x: Fixed[int16, 2] = 2 / a\n", "PYP2253", "no se divide una constante");
}

#[test]
fn a_fixed_point_function_takes_and_returns_fixed_point_values() {
    let (c, s) = same("out: Fixed[uint8, 2] = 0\n\n\ndef power(v: Fixed[uint8, 2], i: Fixed[uint8, 2]) -> Fixed[uint8, 2]:\n    return v * i\n\n\ndef main() -> None:\n    global out\n    out = power(1.10, 0.50)\n");
    assert_eq!(pic_global(&c, &s, "out"), 55);
}

#[test]
fn sixteen_bit_bases_use_a_32_bit_intermediate_and_say_so() {
    // el ejemplo del diseno: 3.300 V x 0.250 A = 0.825 W con base de 16 bits (3300 * 250 = 825000 cabe en 32 bits)
    let (c, s) = same("voltage: Fixed[uint16, 3] = 3.300\ncurrent: Fixed[uint16, 3] = 0.250\npower: Fixed[uint16, 3] = 0\n\n\ndef main() -> None:\n    global power\n    power = voltage * current\n");
    assert_eq!(pic_global(&c, &s, "power"), 825);
    let (ok, t) = diagnostics(&format!("{HEAD}voltage: Fixed[uint16, 3] = 3.300\npower: Fixed[uint16, 3] = 0\n\n\ndef main() -> None:\n    global power\n    power = voltage * voltage\n"));
    assert!(ok && t.contains("PYP3025") && t.contains("32 bits"), "{t}");
    // con signo, escala por una fraccion y subida de escala (todo necesitaba 32 bits)
    let (c, s) = same("a: Fixed[int16, 2] = -2.5\nb: Fixed[int16, 2] = 1.5\nm: Fixed[int16, 2] = 0\nt: Fixed[int16, 2] = 25.37\nx: Fixed[int16, 2] = 0\nfine: Fixed[int16, 3] = 0\nback: Fixed[int16, 2] = 0\n\n\ndef main() -> None:\n    global m, x, fine, back\n    m = a * b\n    x = t * 1.5\n    fine = Fixed[int16, 3](t)\n    back = Fixed[int16, 2](fine)\n");
    assert_eq!(pic_global(&c, &s, "m"), -375);
    assert_eq!(pic_global(&c, &s, "x"), 3805); // 2537 * 3 / 2 = 3805,5: trunca
    assert_eq!(pic_global(&c, &s, "fine"), 25370);
    assert_eq!(pic_global(&c, &s, "back"), 2537);
    // QFixed con 8 bits fraccionarios sobre 16 bits: 1.5 x 1.5 = 2.25 y -1.5 x 1.5
    let (c, s) = same("q: QFixed[int16, 8] = 1.5\nn: QFixed[int16, 8] = -1.5\nr: QFixed[int16, 8] = 0\nm: QFixed[int16, 8] = 0\n\n\ndef main() -> None:\n    global r, m\n    r = q * q\n    m = n * q\n");
    assert_eq!(pic_global(&c, &s, "r"), 576);
    assert_eq!(pic_global(&c, &s, "m"), -576);
}

#[test]
fn the_intermediate_width_follows_what_the_device_supports() {
    // un dispositivo que solo llegue a 16 bits: lo que pide 32 es PYP2255 con el ancho del dispositivo en el mensaje
    let mut t = pack().target_profile();
    t.max_int_bits = 16;
    let src = format!("{HEAD}a: Fixed[uint16, 3] = 3.300\nb: Fixed[uint16, 3] = 0.250\n\n\ndef main() -> None:\n    x: Fixed[uint16, 3] = a * b\n");
    let r = analyze(&src, &t);
    let msg = r.diagnostics.render(&src, "t.py");
    assert!(r.program.is_none() && msg.contains("PYP2255") && msg.contains("16 bits"), "{msg}");
    // la base de 8 bits cabe en 16 bits y sigue valiendo en ese dispositivo
    let src = format!("{HEAD}a: Fixed[uint8, 2] = 1.10\nb: Fixed[uint8, 2] = 0.50\n\n\ndef main() -> None:\n    x: Fixed[uint8, 2] = a * b\n");
    assert!(analyze(&src, &t).program.is_some());
}
