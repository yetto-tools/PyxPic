use pypic_frontend::{analyze, TargetProfile};
use pypic_ir::{lower_unoptimized, print, verify};

fn ir(src: &str) -> String {
    let r = analyze(src, &TargetProfile::generic());
    let p = r.program.unwrap_or_else(|| panic!("frontend:\n{}", r.diagnostics.render(src, "t.py")));
    // estas pruebas comprueban la forma de lo que genera el frontend; las optimizaciones tienen las suyas en opt.rs
    let m = lower_unoptimized(&p);
    verify(&m).unwrap_or_else(|e| panic!("IR invalida: {e:?}\n{}", print(&m)));
    print(&m)
}

#[test]
fn example_program_lowers_and_verifies() {
    ir(include_str!("../../../examples/counter.py"));
}

#[test]
fn constants_become_immediates() {
    let out = ir("K: Const[u8] = 2 * 3\ndef f() -> u8:\n    return K + 1\n");
    assert!(out.contains("return 7:uint8"), "{out}");
}

#[test]
fn short_circuit_has_no_materialized_bool() {
    let out = ir("def f(a: u8, b: u8) -> None:\n    if a != 0 and b != 0:\n        pass\n");
    assert_eq!(out.matches("branch").count(), 2, "{out}");
}

#[test]
fn bool_value_of_and_is_materialized() {
    ir("def f(a: u8, b: u8) -> bool:\n    r: bool = a > 1 or b > 1\n    return r\n");
}

#[test]
fn for_loop_cannot_wrap_counter() {
    // range(0, 255, 10) sobre u8: un `i += 10` ingenuo daria la vuelta en 250 -> 4.
    let out = ir("def f() -> None:\n    for i in range(0, 255, 10):\n        pass\n");
    assert!(out.contains("cast"), "{out}");
    assert!(out.contains("<= 10:uint8"), "{out}");
}

#[test]
fn signed_descending_loop() {
    let out = ir("def f() -> None:\n    for i in range(100, -100, -1):\n        pass\n");
    assert!(out.contains("> -100:int8") || out.contains("> -100:int16"), "{out}");
}

#[test]
fn break_continue_while_nested_calls() {
    ir("def g(x: u8) -> u8:\n    return x\n\
        def f() -> None:\n    n: u8 = 0\n    while True:\n        n = g(n) + 1\n        if n == 5:\n            continue\n        if n > 9:\n            break\n");
}

#[test]
fn verifier_rejects_ill_typed_ir() {
    use pypic_ir::ir::*;
    let f = Function {
        name: "bad".into(),
        is_interrupt: false,
        is_init: false,
        num_params: 0,
        ret: Type::Void,
        locals: vec![Local { name: "x".into(), ty: pypic_frontend::types::U8, is_param: false }],
        temps: vec![],
        blocks: vec![Block {
            insts: vec![Inst::Copy { dst: Place::Local(0), src: Operand::Const { value: 1, ty: pypic_frontend::types::U16 } }],
            term: Terminator::Jump(9),
        }],
        asm: None,
    };
    let m = Module { globals: vec![], adc_acq_ns: 0, roms: vec![], arrays: vec![], array_inits: vec![], functions: vec![f], pins: vec![], device: None, clock_hz: None, clock_defaulted: false, clock_source: None };
    assert_eq!(verify(&m).unwrap_err().len(), 2);
}
