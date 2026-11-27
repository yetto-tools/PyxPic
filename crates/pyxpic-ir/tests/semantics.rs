//! Semantica observable ejecutada sobre la IR con el interprete de referencia.

use pyxpic_frontend::{analyze, TargetProfile};
use pyxpic_ir::{lower, print, verify, Machine, Module, Trap};

fn build(src: &str) -> Module {
    let r = analyze(src, &TargetProfile::generic());
    let p = r.program.unwrap_or_else(|| panic!("frontend:\n{}", r.diagnostics.render(src, "t.py")));
    let m = lower(&p);
    verify(&m).unwrap_or_else(|e| panic!("IR invalida: {e:?}\n{}", print(&m)));
    m
}

fn run(src: &str, f: &str, args: &[i128]) -> Option<i128> {
    let m = build(src);
    Machine::new(&m).call(f, args).unwrap()
}

#[test]
fn modular_arithmetic() {
    let s = "def f(a: u8, b: u8) -> u8:\n    return a + b\n";
    assert_eq!(run(s, "f", &[200, 100]), Some(44));
    let s = "def f(a: u8) -> u8:\n    return a - 1\n";
    assert_eq!(run(s, "f", &[0]), Some(255));
    let s = "def f(a: i8) -> i8:\n    return a + 1\n";
    assert_eq!(run(s, "f", &[127]), Some(-128));
}

#[test]
fn signed_division_truncates_toward_zero() {
    let s = "def f(a: i8, b: i8) -> i8:\n    return a // b\n";
    assert_eq!(run(s, "f", &[-7, 2]), Some(-3));
    assert_eq!(run(s, "f", &[-128, -1]), Some(-128)); // desborda y da la vuelta
    let s = "def f(a: i8, b: i8) -> i8:\n    return a % b\n";
    assert_eq!(run(s, "f", &[-7, 2]), Some(-1));
}

#[test]
fn division_by_zero_has_a_defined_result_and_no_trap() {
    // un PIC no tiene excepciones: a // 0 es todo unos del ancho y a % 0 es a (docs/sdk-runtime.md)
    assert_eq!(run("def f(a: u8, b: u8) -> u8:\n    return a // b\n", "f", &[77, 0]), Some(255));
    assert_eq!(run("def f(a: u8, b: u8) -> u8:\n    return a % b\n", "f", &[77, 0]), Some(77));
    assert_eq!(run("def f(a: i8, b: i8) -> i8:\n    return a // b\n", "f", &[-9, 0]), Some(-1));
    assert_eq!(run("def f(a: i8, b: i8) -> i8:\n    return a % b\n", "f", &[-9, 0]), Some(-9));
    assert_eq!(run("def f(a: u16, b: u16) -> u16:\n    return a // b\n", "f", &[5000, 0]), Some(65535));
}

#[test]
fn casts_truncate() {
    assert_eq!(run("def f(a: u16) -> u8:\n    return u8(a)\n", "f", &[300]), Some(44));
    assert_eq!(run("def f(a: u8) -> i8:\n    return i8(a)\n", "f", &[200]), Some(-56));
}

#[test]
fn shifts_and_bitwise() {
    let s = "def f(a: u8, n: u8) -> u8:\n    return (a << n) | (a >> 1) ^ 3\n";
    assert_eq!(run(s, "f", &[0b1000_0001, 1]), Some(0b0000_0010 | (0b0100_0000 ^ 3)));
    assert_eq!(run("def f(a: u8) -> u8:\n    return ~a\n", "f", &[5]), Some(250));
}

/// El bucle `range(0, 255, 10)` sobre u8 no debe dar la vuelta (250 + 10 = 4).
#[test]
fn range_loop_terminates_without_wrapping() {
    let s = "def f() -> u8:\n    n: u8 = 0\n    for i in range(0, 255, 10):\n        n += 1\n    return n\n";
    assert_eq!(run(s, "f", &[]), Some(26)); // 0,10,...,250
}

#[test]
fn range_loop_variants() {
    let count = |range: &str| {
        let s = format!("def f() -> u16:\n    n: u16 = 0\n    for i in range({range}):\n        n += 1\n    return n\n");
        run(&s, "f", &[]).unwrap()
    };
    assert_eq!(count("10"), 10);
    assert_eq!(count("0"), 0);
    assert_eq!(count("5, 5"), 0);
    assert_eq!(count("7, 3"), 0);
    assert_eq!(count("3, 7"), 4);
    assert_eq!(count("10, 0, -1"), 10);
    assert_eq!(count("100, -100, -7"), 29);
    assert_eq!(count("0, 255"), 255);
    assert_eq!(count("0, 1000, 333"), 4);
    assert_eq!(count("-128, 127"), 255);
}

#[test]
fn range_with_typed_bound_covers_whole_u8_range() {
    let s = "def f(top: u8) -> u16:\n    n: u16 = 0\n    for i in range(top):\n        n += 1\n    return n\n";
    assert_eq!(run(s, "f", &[255]), Some(255));
    assert_eq!(run(s, "f", &[0]), Some(0));
}

#[test]
fn range_bound_is_evaluated_once() {
    let s = "def f() -> u8:\n    n: u8 = 3\n    c: u8 = 0\n    for i in range(n):\n        n += 1\n        c += 1\n    return c\n";
    assert_eq!(run(s, "f", &[]), Some(3));
}

#[test]
fn break_and_continue() {
    let s =
        "def f() -> u8:\n    n: u8 = 0\n    i: u8 = 0\n    while True:\n        i += 1\n        if i == 3:\n            continue\n        if i > 6:\n            break\n        n += i\n    return n\n";
    assert_eq!(run(s, "f", &[]), Some(1 + 2 + 4 + 5 + 6));
}

#[test]
fn short_circuit_skips_side_effects() {
    let s = "calls: u8 = 0\n\
        def touch() -> bool:\n    global calls\n    calls += 1\n    return True\n\
        def f(a: bool) -> u8:\n    if a and touch():\n        pass\n    if a or touch():\n        pass\n    return calls\n";
    assert_eq!(run(s, "f", &[0]), Some(1)); // a=False: `and` corta, `or` evalua touch()
    assert_eq!(run(s, "f", &[1]), Some(1)); // a=True: `and` evalua touch(), `or` corta
}

#[test]
fn globals_and_isr_as_plain_function() {
    let s = "ticks: u16 = 65535\n@interrupt\ndef isr() -> None:\n    global ticks\n    ticks += 1\n";
    let m = build(s);
    let mut mach = Machine::new(&m);
    mach.call("isr", &[]).unwrap();
    assert_eq!(mach.global("ticks"), Some(0));
}

#[test]
fn nested_calls_and_early_return() {
    let s = "def fib(n: u8) -> u16:\n    a: u16 = 0\n    b: u16 = 1\n    for i in range(n):\n        t: u16 = a + b\n        a = b\n        b = t\n    return a\n\
        def f() -> u16:\n    return fib(10)\n";
    assert_eq!(run(s, "f", &[]), Some(55));
}

#[test]
fn infinite_loop_hits_step_limit() {
    let m = build("def f() -> None:\n    while True:\n        pass\n");
    assert_eq!(Machine::new(&m).with_step_limit(1000).call("f", &[]), Err(Trap::StepLimit));
}

#[test]
fn example_program_runs() {
    let m = build(include_str!("../../../examples/python/gpio/counter.py"));
    assert_eq!(Machine::new(&m).with_step_limit(100_000).call("main", &[]), Ok(None));
}

// ---- constantes locales sin slot (#11) ---------------------------------------------------------------

#[test]
fn a_local_const_has_no_slot_and_no_store() {
    let src = "def f(a: u8) -> u16:\n    K: Const[u8] = 5\n    L: Const[u16] = 1000\n    M: Const[u16] = L + 24\n    return u16(a + K) + M\n";
    let m = build(src);
    let f = &m.functions[0];
    // el unico local es el parametro; ni K ni L ni M existen en la IR
    assert_eq!(f.locals.len(), 1, "{}", print(&m));
    assert_eq!(run(src, "f", &[10]), Some(15 + 1024));
}

#[test]
fn a_local_const_is_usable_in_conditions_loops_and_calls() {
    let src = "def g(x: u8) -> u8:\n    return x + 1\n\n\ndef f(a: u8) -> u8:\n    LIM: Const[u8] = 3\n    n: u8 = 0\n    for i in range(LIM):\n        n = g(n) + i\n    if a > LIM:\n        n += LIM\n    return n\n";
    let m = build(src);
    // solo `a`, `n` y la variable del bucle: LIM no tiene slot
    assert_eq!(m.functions[1].locals.len(), 3, "{}", print(&m));
    // i = 0, 1, 2: n = 1, 3, 6; a = 9 > 3 suma LIM: 9
    assert_eq!(run(src, "f", &[9]), Some(9));
    assert_eq!(run(src, "f", &[2]), Some(6));
}
