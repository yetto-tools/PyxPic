//! Pruebas diferenciales del generador de codigo: el mismo programa se ejecuta en el interprete de
//! referencia de la IR y, ya compilado, en el simulador del nucleo PIC14; los globales finales
//! tienen que coincidir.

use std::collections::BTreeMap;

use pypic_frontend::{analyze, TargetProfile};
use pypic_ir::{lower_unoptimized, lower, verify, Machine, Module};
use pypic_pic14::codegen::{CodegenError, Compiled, GlobalSym, Options};
use pypic_pic14::sim::Sim;
use pypic_pic14::{compile, Inst};

fn build(src: &str) -> (Module, Compiled) {
    let r = analyze(src, &TargetProfile::generic());
    let p = r.program.unwrap_or_else(|| panic!("frontend:\n{}\n{src}", r.diagnostics.render(src, "t.py")));
    let m = lower(&p);
    verify(&m).unwrap_or_else(|e| panic!("IR invalida: {e:?}"));
    let c = compile(&m, &Options::default()).unwrap_or_else(|e| panic!("codegen: {e}\n{src}"));
    (m, c)
}

fn run_pic(c: &Compiled) -> Sim {
    let mut s = Sim::new(&c.image);
    s.run_until(100_000_000, |s| s.halted);
    assert!(s.halted, "el programa no termino");
    s
}

fn read_global(s: &Sim, g: &GlobalSym) -> i128 {
    let n = if g.ty.bits() == 16 { 2 } else { 1 };
    let mut v: i128 = 0;
    for i in 0..n {
        v |= (s.read(g.addr as usize + i) as i128) << (8 * i);
    }
    g.ty.wrap(v)
}

fn pic_globals(c: &Compiled, s: &Sim) -> BTreeMap<String, i128> {
    // las constantes Const no existen en memoria: solo se comparan las variables
    c.globals.iter().filter(|g| !g.is_const).map(|g| (g.name.clone(), read_global(s, g))).collect()
}

fn ir_globals(m: &Module) -> BTreeMap<String, i128> {
    let mut mach = Machine::new(m);
    mach.run_main().unwrap();
    m.globals.iter().filter(|g| !g.is_const).map(|g| (g.name.clone(), mach.global(&g.name).unwrap())).collect()
}

/// Compila y exige que PIC14 e IR den los mismos globales. Devuelve los globales.
fn same(src: &str) -> BTreeMap<String, i128> {
    let (_, c) = build(src);
    // el oraculo es el interprete sobre la IR sin optimizar: asi el PIC (con IR optimizada) y las optimizaciones se comprueban a la vez
    let r = analyze(src, &TargetProfile::generic());
    let raw = lower_unoptimized(&r.program.expect("el frontend lo acepta"));
    let want = ir_globals(&raw);
    let got = pic_globals(&c, &run_pic(&c));
    assert_eq!(got, want, "PIC14 distinto del oraculo de IR en:\n{src}\n--- listado ---\n{}", c.listing);
    got
}

// ---- generador pseudoaleatorio determinista ------------------------------------------------------
struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
}

fn range_of(ty: &str) -> (i128, i128) {
    match ty {
        "u8" => (0, 255),
        "i8" => (-128, 127),
        "u16" => (0, 65535),
        _ => (-32768, 32767),
    }
}

fn operands(ty: &str, rng: &mut Rng) -> Vec<(i128, i128)> {
    let (lo, hi) = range_of(ty);
    let mut edge = vec![lo, lo + 1, hi, hi - 1, 0, 1, 2.min(hi), hi / 2, hi / 2 + 1, lo / 2];
    if ty.ends_with("16") {
        edge.extend([0x00FF, 0x0100, 0x0101, 0x7FFF.min(hi), 0x8000.min(hi), 0xFF00.min(hi), -256.max(lo), -255.max(lo)]);
    }
    edge.retain(|v| (lo..=hi).contains(v));
    edge.sort_unstable();
    edge.dedup();
    let mut out: Vec<(i128, i128)> = edge.iter().flat_map(|&a| edge.iter().map(move |&b| (a, b))).collect();
    for _ in 0..120 {
        let span = (hi - lo + 1) as u64;
        out.push((lo + (rng.next() % span) as i128, lo + (rng.next() % span) as i128));
    }
    out
}

// ---- operaciones binarias y comparaciones: cada tipo x cada operador x muchos operandos ---------

#[test]
fn arithmetic_and_logic_match_the_ir_oracle_for_all_types() {
    let mut rng = Rng(0x9E3779B97F4A7C15);
    let mut cases = 0;
    for ty in ["u8", "i8", "u16", "i16"] {
        for op in ["+", "-", "&", "|", "^"] {
            for (a, b) in operands(ty, &mut rng) {
                let src = format!("a: {ty} = {a}\nb: {ty} = {b}\nr: {ty} = 0\ndef main() -> None:\n    global r\n    r = a {op} b\n");
                same(&src);
                cases += 1;
            }
        }
    }
    assert!(cases > 3000, "{cases}");
}

#[test]
fn comparisons_match_the_ir_oracle_for_all_types() {
    let mut rng = Rng(0xD1B54A32D192ED03);
    let mut cases = 0;
    for ty in ["u8", "i8", "u16", "i16"] {
        for op in ["==", "!=", "<", "<=", ">", ">="] {
            for (a, b) in operands(ty, &mut rng) {
                let src = format!("a: {ty} = {a}\nb: {ty} = {b}\nr: bool = False\ndef main() -> None:\n    global r\n    r = a {op} b\n");
                same(&src);
                cases += 1;
            }
        }
    }
    assert!(cases > 4000, "{cases}");
}

#[test]
fn constants_as_either_operand() {
    let mut rng = Rng(0xA0761D6478BD642F);
    for ty in ["u8", "i8", "u16", "i16"] {
        let (lo, hi) = range_of(ty);
        for _ in 0..40 {
            let span = (hi - lo + 1) as u64;
            let (a, k) = (lo + (rng.next() % span) as i128, lo + (rng.next() % span) as i128);
            for body in ["r = a + K", "r = K - a", "r = a - K", "r = K & a", "r = a | K", "r = K ^ a"] {
                let body = body.replace('K', &k.to_string());
                same(&format!("a: {ty} = {a}\nr: {ty} = 0\ndef main() -> None:\n    global r\n    {body}\n"));
            }
            for body in ["q = a < K", "q = K < a", "q = a >= K", "q = K >= a", "q = a == K", "q = K != a"] {
                let body = body.replace('K', &k.to_string());
                same(&format!("a: {ty} = {a}\nq: bool = False\ndef main() -> None:\n    global q\n    {body}\n"));
            }
        }
    }
}

#[test]
fn unary_operators_and_casts() {
    for (ty, vals) in [("i8", vec![-128, -1, 0, 1, 127, 55]), ("i16", vec![-32768, -256, -1, 0, 255, 256, 32767, 1234])] {
        for v in vals {
            same(&format!("a: {ty} = {v}\nr: {ty} = 0\ndef main() -> None:\n    global r\n    r = -a\n"));
        }
    }
    for (ty, vals) in [("u8", vec![0, 1, 0x55, 255]), ("i8", vec![-128, -1, 0, 127]), ("u16", vec![0, 255, 256, 0xAAAA, 65535]), ("i16", vec![-32768, -1, 0, 32767])] {
        for v in vals {
            same(&format!("a: {ty} = {v}\nr: {ty} = 0\ndef main() -> None:\n    global r\n    r = ~a\n"));
        }
    }
    for flag in ["True", "False"] {
        same(&format!("f: bool = {flag}\nr: bool = False\ndef main() -> None:\n    global r\n    r = not f\n"));
    }
    // conversiones: ensanchar con y sin signo, estrechar, bool -> entero
    for (from, to) in [("u8", "u16"), ("u8", "i16"), ("i8", "i16"), ("i8", "u16"), ("u16", "u8"), ("i16", "i8"), ("u16", "i16"), ("i16", "u16"), ("u8", "i8"), ("i8", "u8")] {
        let (lo, hi) = range_of(from);
        for v in [lo, lo + 1, -1.max(lo), 0, 1, 0x7F.min(hi), 0x80.min(hi), 0xFF.min(hi), hi - 1, hi, 1234.min(hi)] {
            if (lo..=hi).contains(&v) {
                same(&format!("a: {from} = {v}\nr: {to} = 0\ndef main() -> None:\n    global r\n    r = {to}(a)\n"));
            }
        }
    }
    for flag in ["True", "False"] {
        same(&format!("f: bool = {flag}\nr: u8 = 9\nw: u16 = 9\ndef main() -> None:\n    global r, w\n    r = u8(f)\n    w = u16(f)\n"));
    }
}

#[test]
fn constant_shifts() {
    let mut rng = Rng(0xE7037ED1A0B428DB);
    for ty in ["u8", "i8", "u16", "i16"] {
        let (lo, hi) = range_of(ty);
        let bits = if ty.ends_with("16") { 16 } else { 8 };
        for k in 0..bits {
            for _ in 0..6 {
                let a = lo + (rng.next() % (hi - lo + 1) as u64) as i128;
                for op in ["<<", ">>"] {
                    same(&format!("a: {ty} = {a}\nr: {ty} = 0\ndef main() -> None:\n    global r\n    r = a {op} {k}\n"));
                }
            }
        }
    }
}

// ---- programas con control de flujo, llamadas y globales --------------------------------------

#[test]
fn loops_and_calls() {
    let g = same(
        "total: u16 = 0\ndef main() -> None:\n    global total\n    for i in range(100):\n        total += u16(i)\n",
    );
    assert_eq!(g["total"], 4950);

    let g = same(
        "r: u16 = 0\ndef main() -> None:\n    global r\n    a: u16 = 0\n    b: u16 = 1\n    for i in range(24):\n        t: u16 = a + b\n        a = b\n        b = t\n    r = a\n",
    );
    assert_eq!(g["r"], 46368);

    // maximo comun divisor por restas
    let g = same(
        "r: u8 = 0\ndef main() -> None:\n    global r\n    a: u8 = 252\n    b: u8 = 105\n    while a != b:\n        if a > b:\n            a -= b\n        else:\n            b -= a\n    r = a\n",
    );
    assert_eq!(g["r"], 21);
}

#[test]
fn functions_with_arguments_and_returns() {
    let g = same(
        "r1: u8 = 0\nr2: u16 = 0\nr3: i16 = 0\nr4: bool = False\n\
         def add(a: u8, b: u8) -> u8:\n    return a + b\n\
         def widen(a: u8, b: u8) -> u16:\n    return u16(a) + u16(b) + 300\n\
         def neg(a: i16) -> i16:\n    return -a\n\
         def lt(a: i16, b: i16) -> bool:\n    return a < b\n\
         def main() -> None:\n    global r1, r2, r3, r4\n    r1 = add(200, 100)\n    r2 = widen(r1, 7)\n    r3 = neg(i16(r2))\n    r4 = lt(r3, -500)\n",
    );
    // 200+100 envuelve a 44; 44+7+300 = 351; -351; y -351 < -500 es falso
    assert_eq!((g["r1"], g["r2"], g["r3"], g["r4"]), (44, 351, -351, 0));
}

#[test]
fn nested_calls_and_early_returns() {
    same(
        "r: u16 = 0\n\
         def sq3(n: u8) -> u16:\n    return u16(n) + u16(n) + u16(n)\n\
         def pick(n: u8) -> u16:\n    if n > 10:\n        return sq3(n)\n    if n == 0:\n        return 1000\n    return sq3(n) + 1\n\
         def main() -> None:\n    global r\n    for i in range(15):\n        r += pick(i)\n",
    );
}

#[test]
fn break_continue_and_signed_ranges() {
    same(
        "n: u8 = 0\ndef main() -> None:\n    global n\n    i: u8 = 0\n    while True:\n        i += 1\n        if i == 3:\n            continue\n        if i > 20:\n            break\n        n += i\n",
    );
    same("s: i16 = 0\ndef main() -> None:\n    global s\n    for i in range(10, -10, -3):\n        s += i16(i)\n");
    same("s: u16 = 0\ndef main() -> None:\n    global s\n    for i in range(0, 255, 10):\n        s += u16(i)\n");
}

#[test]
fn boolean_logic_with_short_circuit_side_effects() {
    let g = same(
        "calls: u8 = 0\nr: bool = False\nq: bool = False\n\
         def touch() -> bool:\n    global calls\n    calls += 1\n    return True\n\
         def main() -> None:\n    global r, q, calls\n    a: bool = False\n    b: bool = True\n    r = a and touch()\n    q = b or touch()\n    if a or touch():\n        calls += 10\n    if not b and touch():\n        calls += 100\n",
    );
    assert_eq!(g["calls"], 11); // solo se evalua touch() en `a or touch()`
}

#[test]
fn startup_initializes_every_global() {
    let g = same("a: u8 = 7\nb: u16 = 0x1234\nc: i8 = -3\nd: bool = True\nK: Const[u8] = 9\ndef main() -> None:\n    pass\n");
    assert_eq!((g["a"], g["b"], g["c"], g["d"]), (7, 0x1234, -3, 1));
    assert!(!g.contains_key("K"), "las constantes no se almacenan");
}

// ---- interrupciones: contexto guardado y restaurado ----------------------------------------------

const ISR_PROGRAM: &str = "\
counter: u8 = 0\nacc: u16 = 0\ntotal: u16 = 0\nflag: bool = False\n\
@interrupt\ndef isr() -> None:\n    global counter, flag\n    counter += 1\n    t: u8 = counter ^ 0x55\n    if t > 100:\n        flag = not flag\n    counter += 0\n\
def main() -> None:\n    global acc, total\n    for i in range(40):\n        acc = acc + u16(i) + 3\n        if acc > 500:\n            acc = acc - 400\n    total = acc ^ 0x1234\n";

#[test]
fn an_interrupt_at_any_instruction_leaves_main_untouched() {
    let (m, c) = build(ISR_PROGRAM);
    let want = ir_globals(&m);
    let main_addr = c.functions.iter().find(|(n, _)| n == "main").unwrap().1;

    // referencia sin interrupcion
    let mut s = Sim::new(&c.image);
    let mut start = None;
    let mut steps = 0u64;
    while !s.halted {
        if start.is_none() && s.pc as u32 == main_addr {
            start = Some(steps);
        }
        s.step();
        steps += 1;
    }
    let start = start.expect("main se alcanza");
    let reference = pic_globals(&c, &s);
    assert_eq!((reference["acc"], reference["total"]), (want["acc"], want["total"]));

    let mut injected = 0;
    for n in start..steps {
        let mut s = Sim::new(&c.image);
        for _ in 0..n {
            s.step();
        }
        s.interrupt(); // entre dos instrucciones cualesquiera de main
        s.run_until(1_000_000, |s| s.halted);
        let got = pic_globals(&c, &s);
        assert_eq!((got["acc"], got["total"]), (reference["acc"], reference["total"]), "interrupcion tras {n} pasos");
        assert_eq!(got["counter"], 1, "interrupcion tras {n} pasos");
        injected += 1;
    }
    assert!(injected > 500, "{injected}");
}

#[test]
fn interrupt_vector_and_default_retfie() {
    let (_, c) = build(ISR_PROGRAM);
    let at = |a| c.image.get(a).and_then(Inst::decode);
    assert!(matches!(at(0), Some(Inst::Goto(5))));
    assert!(matches!(at(4), Some(Inst::Goto(_)))); // salta al prologo de la ISR
    let (_, c) = build("def main() -> None:\n    pass\n");
    assert_eq!(c.image.get(4).and_then(Inst::decode), Some(Inst::Retfie));
}

// ---- errores y limites ----------------------------------------------------------------------------

fn compile_err(src: &str) -> CodegenError {
    let r = analyze(src, &TargetProfile::generic());
    let m = lower(&r.program.expect("frontend ok"));
    compile(&m, &Options::default()).unwrap_err()
}

#[test]
fn unsupported_features_fail_with_a_clear_error() {
    let e = compile_err("a: u32 = 3\ndef main() -> None:\n    pass\n");
    assert!(matches!(&e, CodegenError::Unsupported(s) if s.contains("uint32")), "{e}");
    assert_eq!(compile_err("def f() -> None:\n    pass\n"), CodegenError::NoMain);
}

#[test]
fn bank0_ram_exhaustion_is_reported() {
    let mut src = String::new();
    for i in 0..100 {
        src.push_str(&format!("g{i}: u8 = {i}\n"));
    }
    src.push_str("def main() -> None:\n    pass\n");
    assert!(matches!(compile_err(&src), CodegenError::OutOfRam { needed: 100, available: 80 }));
}

#[test]
fn unreachable_functions_are_not_compiled() {
    let (_, c) = build("def dead() -> u8:\n    return 1\ndef main() -> None:\n    pass\n");
    assert!(c.functions.iter().all(|(n, _)| n != "dead"));
}

#[test]
fn listing_and_config_words() {
    let (_, c) = build("x: u8 = 1\ndef main() -> None:\n    global x\n    x += 1\n");
    assert!(c.listing.contains("main:") && c.listing.contains("startup:") && c.listing.contains("GOTO"), "{}", c.listing);
    assert_eq!(c.image.get(0x2007), Some(0x20D4));
    assert_eq!(c.image.get(0x2008), Some(0x3FFF));
    assert!(c.image.check_bounds(8192, (0x2000, 0x3FFF)).is_ok());
}

// ---- declaracion de dispositivo, constantes y @init_device -------------------------------------------

#[test]
fn init_device_runs_before_main_on_both_the_oracle_and_the_pic() {
    let g = same(
        "x: u8 = 0\ny: u8 = 0\nz: u8 = 0\n\
         @init_device\ndef configurar() -> None:\n    global x, z\n    x = 5\n    z = x + 1\n\
         def main() -> None:\n    global y, z\n    y = x + 1\n    z = z + 10\n",
    );
    assert_eq!((g["x"], g["y"], g["z"]), (5, 6, 16)); // main ve lo que dejo init y ya no puede deshacerlo
}

#[test]
fn init_device_can_call_helpers_and_use_pins() {
    let g = same(
        "from pypic.periph.gpio import RD0\nready: u8 = 0\n\
         def prepara() -> u8:\n    return 3\n\
         @init_device\ndef configurar() -> None:\n    global ready\n    RD0.output()\n    ready = prepara()\n\
         def main() -> None:\n    RD0.value(ready)\n",
    );
    assert_eq!(g["ready"], 3);
}

#[test]
fn final_constants_take_no_ram_even_when_wide() {
    let (_, c) = build("K: Const[u32] = 4_000_000\nL: Const[u16] = 1234\ndef main() -> None:\n    pass\n");
    assert_eq!(c.ram_bytes, 0, "las constantes no ocupan RAM");
    // y se usan por su valor
    let g = same("K: Const[u32] = 4_000_000\nx: u8 = 0\ndef main() -> None:\n    global x\n    x = u8(K >> 18)\n");
    assert_eq!(g["x"], 15);
}

#[test]
fn declared_clock_reaches_the_ir() {
    let r = analyze("from pypic.device import PIC16F887\nCLOCK_HZ: Const[u32] = 8_000_000\ndef main() -> None:\n    pass\n", &TargetProfile::generic());
    let m = lower(&r.program.unwrap());
    assert_eq!((m.device.as_deref(), m.clock_hz), (Some("PIC16F887"), Some(8_000_000)));
}

#[test]
fn the_backend_refuses_devices_it_does_not_know_or_that_do_not_match() {
    let r = analyze("from pypic.device import PIC12F625\ndef main() -> None:\n    pass\n", &TargetProfile::generic());
    let m = lower(&r.program.unwrap());
    // el programa declara un dispositivo y se compila para otro
    let e = compile(&m, &Options::default()).unwrap_err();
    assert!(matches!(&e, CodegenError::Device(s) if s.contains("PIC12F625") && s.contains("PIC16F887")), "{e}");
    // un dispositivo para el que no hay mapa de registros: error, no codigo del PIC16F88x a ciegas
    let opts = Options { device: "PIC12F625".into(), ..Options::default() };
    let e = compile(&m, &opts).unwrap_err();
    assert!(matches!(&e, CodegenError::Device(s) if s.contains("no soporta") && s.contains("PIC16F887")), "{e}");
}

#[test]
fn ram_range_and_program_size_come_from_the_options() {
    let src = "a: u8 = 1\nb: u8 = 2\ndef main() -> None:\n    global a, b\n    a = a + b\n";
    let r = analyze(src, &TargetProfile::generic());
    let m = lower(&r.program.unwrap());
    // con 2 bytes de RAM de datos caben justo los dos globales; con 1 no
    assert!(compile(&m, &Options { ram_end: 0x21, ..Options::default() }).is_ok());
    let tight = Options { ram_end: 0x20, ..Options::default() };
    assert!(matches!(compile(&m, &tight), Err(CodegenError::OutOfRam { available: 1, .. })));
    // un programa mas grande que la memoria del dispositivo
    let tiny = Options { program_words: 10, ..Options::default() };
    assert!(matches!(compile(&m, &tiny), Err(CodegenError::TooLarge { .. })));
    // una zona de datos que pisa la zona comun de la ABI (70h-7Fh)
    let overlap = Options { ram_end: 0x7F, ..Options::default() };
    assert!(matches!(compile(&m, &overlap), Err(CodegenError::Device(_))));
}

#[test]
fn config_words_go_where_the_options_say() {
    let r = analyze("def main() -> None:\n    pass\n", &TargetProfile::generic());
    let m = lower(&r.program.unwrap());
    let opts = Options { config: vec![(0x2007, 0x1234), (0x2008, 0x0ABC)], ..Options::default() };
    let c = compile(&m, &opts).unwrap();
    assert_eq!((c.image.get(0x2007), c.image.get(0x2008)), (Some(0x1234), Some(0x0ABC)));
}

// ---- constantes locales: sin RAM ni instrucciones (#11) -------------------------------------------------

#[test]
fn local_constants_cost_no_ram_and_no_code() {
    let with = "r: u16 = 0\ndef main() -> None:\n    global r\n    K: Const[u8] = 5\n    L: Const[u16] = 1000\n    r = u16(K) + L\n";
    let without = "r: u16 = 0\ndef main() -> None:\n    global r\n    r = u16(5) + 1000\n";
    let (_, a) = build(with);
    let (_, b) = build(without);
    assert_eq!((a.ram_bytes, a.flash_words), (b.ram_bytes, b.flash_words), "{}", a.listing);
    assert_eq!(a.ram_bytes, 2); // solo la global r
    let (_, s) = (0, {
        let mut s = Sim::new(&a.image);
        s.run_until(100_000, |s| s.halted);
        s
    });
    let r = a.globals.iter().find(|g| g.name == "r").unwrap();
    assert_eq!(s.read(r.addr as usize) as u16 | (s.read(r.addr as usize + 1) as u16) << 8, 1005);
}
