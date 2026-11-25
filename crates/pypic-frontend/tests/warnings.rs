//! #5: advertencias reales (`Severity::Warning`): codigo inalcanzable (`PYP3020`), variables locales que nunca se leen (`PYP3021`) y parametros
//! que nunca se usan (`PYP3022`). No impiden compilar.

use pypic_frontend::{analyze, Severity, TargetProfile};

fn analysis(src: &str) -> (Vec<&'static str>, String, bool) {
    let r = analyze(src, &TargetProfile::generic());
    (r.diagnostics.codes(), r.diagnostics.render(src, "t.py"), r.program.is_some())
}

fn codes(src: &str) -> Vec<&'static str> {
    analysis(src).0
}

fn main_body(body: &str) -> String {
    format!("g: uint8 = 0\n\n\ndef main() -> None:\n    global g\n{body}")
}

// ---- codigo inalcanzable ------------------------------------------------------------------------------------------------------

#[test]
fn code_after_return_break_and_continue_is_unreachable() {
    let src = "def f(a: uint8) -> uint8:\n    return a\n    a = 1\n\n\ndef main() -> None:\n    f(1)\n";
    let (codes, shown, program) = analysis(src);
    assert_eq!(codes, vec!["PYP3020"]);
    assert!(shown.contains("t.py:3:5: warning[PYP3020]: codigo inalcanzable: sigue a un return"), "{shown}");
    assert!(program, "una advertencia no impide compilar");
    let loop_body = |stmt: &str| main_body(&format!("    for i in range(3):\n        {stmt}\n        g = i\n"));
    assert_eq!(codes_of(&loop_body("break")), vec!["PYP3020"]);
    assert!(analysis(&loop_body("break")).1.contains("sigue a un break"));
    assert!(analysis(&loop_body("continue")).1.contains("sigue a un continue"));
}

fn codes_of(src: &str) -> Vec<&'static str> {
    codes(src)
}

#[test]
fn a_branch_that_always_leaves_and_an_endless_loop_make_what_follows_unreachable() {
    let both = "def f(a: uint8) -> uint8:\n    if a > 1:\n        return 1\n    else:\n        return 2\n    return 3\n\n\ndef main() -> None:\n    f(1)\n";
    let (c, shown, _) = analysis(both);
    assert_eq!(c, vec!["PYP3020"]);
    assert!(shown.contains("las dos ramas del if terminan"), "{shown}");
    let forever = main_body("    while True:\n        g += 1\n    g = 9\n");
    let (c, shown, _) = analysis(&forever);
    assert_eq!(c, vec!["PYP3020"]);
    assert!(shown.contains("'while True' sin break"), "{shown}");
}

#[test]
fn reachable_code_does_not_warn() {
    // un if sin else, un if cuyas ramas no terminan las dos, y un while True con break
    assert!(codes(&main_body("    if g > 1:\n        return\n    g = 2\n")).is_empty());
    assert!(codes("def f(a: uint8) -> uint8:\n    if a > 1:\n        return 1\n    else:\n        a = 2\n    return a\n\n\ndef main() -> None:\n    f(1)\n").is_empty());
    assert!(codes(&main_body("    while True:\n        g += 1\n        if g > 5:\n            break\n    g = 0\n")).is_empty());
    // el break de un bucle interior no libra al `while True` exterior de no terminar
    let nested = main_body("    while True:\n        for i in range(3):\n            if i > 1:\n                break\n        g += 1\n    g = 9\n");
    assert_eq!(codes(&nested), vec!["PYP3020"]);
}

#[test]
fn only_the_first_unreachable_statement_of_a_block_warns_and_pass_does_not_count() {
    let src = "def f(a: uint8) -> uint8:\n    return a\n    a = 1\n    a = 2\n    a = 3\n\n\ndef main() -> None:\n    f(1)\n";
    assert_eq!(codes(src), vec!["PYP3020"], "una advertencia por bloque");
    let with_pass = "def f(a: uint8) -> uint8:\n    return a\n    pass\n\n\ndef main() -> None:\n    f(1)\n";
    assert!(codes(with_pass).is_empty(), "un pass suelto tras un return no avisa");
    // y el codigo inalcanzable se sigue comprobando: un error de tipos alli se reporta
    let typed = "def f(a: uint8) -> uint8:\n    return a\n    b: uint8 = 300\n\n\ndef main() -> None:\n    f(1)\n";
    let c = codes(typed);
    assert!(c.contains(&"PYP3020") && c.contains(&"PYP2040"), "{c:?}");
}

// ---- variables y parametros sin usar ---------------------------------------------------------------------------------------------

#[test]
fn a_local_that_is_never_read_warns_where_it_is_declared() {
    let (c, shown, program) = analysis("def main() -> None:\n    x: uint8 = 5\n");
    assert_eq!(c, vec!["PYP3021"]);
    assert!(shown.contains("t.py:2:5: warning[PYP3021]: la variable local 'x' se declara pero nunca se lee"), "{shown}");
    assert!(program);
    // solo se asigna, nunca se lee
    assert_eq!(codes("def main() -> None:\n    x: uint8 = 5\n    x = 6\n"), vec!["PYP3021"]);
    // una constante local sin usar tambien
    assert_eq!(codes("def main() -> None:\n    K: Const[uint8] = 5\n"), vec!["PYP3021"]);
}

#[test]
fn a_local_that_is_read_in_any_way_does_not_warn() {
    let read = |body: &str| codes(&main_body(body));
    assert!(read("    x: uint8 = 5\n    g = x\n").is_empty(), "leida en una asignacion");
    assert!(read("    x: uint8 = 5\n    x += 1\n    g = x\n").is_empty());
    assert!(read("    c: uint8 = 0\n    c += 1\n").is_empty(), "un contador que solo se incrementa es un uso");
    assert!(read("    x: uint8 = 5\n    if x > 2:\n        g = 1\n").is_empty(), "leida en una condicion");
    assert!(codes("from pypic.periph.uart import UART\nCLOCK_HZ: Const[uint32] = 4_000_000\n\n\ndef main() -> None:\n    UART.start(baud=9600)\n    v: uint8 = 7\n    UART.write_text(f\"{v}\")\n").is_empty(), "leida en un campo de f-string");
    // la variable bool donde parse_dec anota el error: se escribe y despues se lee
    let parse = "buf: uint8[4]\n\n\ndef main() -> None:\n    bad: bool = False\n    v: uint16 = parse_dec(buf, 2, bad)\n    if bad:\n        v = 0\n    w: uint16 = v\n    w += 1\n";
    assert!(codes(parse).is_empty(), "{:?}", codes(parse));
}

#[test]
fn names_starting_with_an_underscore_and_loop_variables_never_warn() {
    assert!(codes("def main() -> None:\n    _x: uint8 = 5\n").is_empty());
    assert!(codes(&main_body("    for i in range(3):\n        g += 1\n")).is_empty(), "la variable de un for no avisa");
}

#[test]
fn an_unused_parameter_warns_and_an_underscore_name_does_not() {
    let (c, shown, _) = analysis("def f(a: uint8, b: uint8) -> uint8:\n    return a\n\n\ndef main() -> None:\n    f(1, 2)\n");
    assert_eq!(c, vec!["PYP3022"]);
    assert!(shown.contains("t.py:1:17: warning[PYP3022]: el parametro 'b' de 'f' nunca se usa"), "{shown}");
    assert!(codes("def f(a: uint8, _b: uint8) -> uint8:\n    return a\n\n\ndef main() -> None:\n    f(1, 2)\n").is_empty());
}

#[test]
fn an_asm_function_parameters_are_not_checked_because_the_assembly_uses_them() {
    let src = "from pypic.device import PIC16F887\n\n\n@asm_function\ndef sumar(a: uint8) -> uint8:\n    \"\"\"\n    MOVF a,W\n    RETURN\n    \"\"\"\n\n\ndef main() -> None:\n    sumar(1)\n";
    assert!(!codes(src).contains(&"PYP3022"), "{:?}", codes(src));
}

#[test]
fn the_warnings_are_real_warnings_and_do_not_block_a_program() {
    let r = analyze("def f(a: uint8) -> uint8:\n    return 1\n\n\ndef main() -> None:\n    x: uint8 = 1\n    f(2)\n    return\n    f(3)\n", &TargetProfile::generic());
    let mut got: Vec<&str> = r.diagnostics.items.iter().filter(|d| d.severity == Severity::Warning).map(|d| d.code).collect();
    got.sort();
    assert_eq!(got, vec!["PYP3020", "PYP3021", "PYP3022"]);
    assert!(r.diagnostics.items.iter().all(|d| d.severity == Severity::Warning));
    assert!(r.program.is_some());
}
