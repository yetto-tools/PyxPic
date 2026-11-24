//! Decisiones de diseno #1 (tipos anchos), #2 (semantica) y #3 (ISR vs main).

use pypic_frontend::{analyze, Severity, TargetProfile};

fn pic16() -> TargetProfile {
    TargetProfile::new("pic16-like", 8, 32)
}

fn count(src: &str, t: &TargetProfile, code: &str) -> usize {
    analyze(src, t).diagnostics.items.iter().filter(|d| d.code == code).count()
}

// ---- #1: tipos anchos se permiten con aviso de coste ------------------------

#[test]
fn wide_type_on_narrow_target_compiles_with_one_warning() {
    let src = "a: u16 = 0\nb: u16 = 1\ndef f(x: u16) -> u16:\n    return x + a + b\n";
    let r = analyze(src, &pic16());
    assert!(r.program.is_some(), "{}", r.diagnostics.render(src, "t.py"));
    let w: Vec<_> = r.diagnostics.items.iter().filter(|d| d.severity == Severity::Warning).collect();
    assert_eq!(w.len(), 1, "un aviso por tipo, no por uso");
    assert_eq!(w[0].code, "PYP3001");
}

#[test]
fn each_emulated_type_warns_once() {
    let src = "a: u16 = 0\nb: u32 = 0\nc: u32 = 0\n";
    assert_eq!(count(src, &pic16(), "PYP3001"), 2);
}

#[test]
fn native_types_do_not_warn() {
    assert_eq!(count("a: u8 = 0\nb: bool = True\n", &pic16(), "PYP3001"), 0);
    assert_eq!(count("a: u32 = 0\n", &TargetProfile::generic(), "PYP3001"), 0);
    // en un destino de 16 bits nativos, u16 es nativo y u32 emulado
    let t = TargetProfile::new("pic24-like", 16, 32);
    assert_eq!(count("a: u16 = 0\n", &t, "PYP3001"), 0);
    assert_eq!(count("a: u32 = 0\n", &t, "PYP3001"), 1);
}

#[test]
fn beyond_max_is_still_an_error() {
    let t = TargetProfile::new("tiny", 8, 16);
    assert_eq!(count("a: u32 = 0\n", &t, "PYP2030"), 1);
    assert!(analyze("a: u32 = 0\n", &t).program.is_none());
}

#[test]
fn warning_for_casts_and_inferred_range_types() {
    let src = "def f(a: u8) -> None:\n    x: u8 = u8(u16(a))\n";
    assert_eq!(count(src, &pic16(), "PYP3001"), 1);
    let src = "def f() -> None:\n    for i in range(300):\n        pass\n";
    assert_eq!(count(src, &pic16(), "PYP3001"), 1);
}

#[test]
fn warnings_do_not_fail_the_check_and_are_rendered() {
    let src = "a: u16 = 0\n";
    let r = analyze(src, &pic16());
    assert!(r.program.is_some());
    assert!(r.diagnostics.render(src, "t.py").contains("warning[PYP3001]"));
}

// ---- #3: funciones compartidas entre main e interrupcion -----------------------

const ISR: &str = "@interrupt\ndef isr() -> None:\n    helper()\n";

#[test]
fn shared_function_is_rejected() {
    let src = format!("{ISR}def helper() -> None:\n    pass\ndef main() -> None:\n    helper()\n");
    assert_eq!(count(&src, &TargetProfile::generic(), "PYP2060"), 1);
}

#[test]
fn transitively_shared_function_is_rejected() {
    let src = "@interrupt\ndef isr() -> None:\n    a()\ndef a() -> None:\n    leaf()\n\
               def main() -> None:\n    b()\ndef b() -> None:\n    leaf()\ndef leaf() -> None:\n    pass\n";
    assert_eq!(count(src, &TargetProfile::generic(), "PYP2060"), 1); // solo `leaf`
}

#[test]
fn separate_versions_per_context_are_fine() {
    let src = format!("{ISR}def helper() -> None:\n    pass\ndef helper_main() -> None:\n    pass\ndef main() -> None:\n    helper_main()\n");
    assert!(analyze(&src, &TargetProfile::generic()).program.is_some());
}

#[test]
fn isr_only_helpers_and_global_sharing_are_fine() {
    let src = "n: u8 = 0\n@interrupt\ndef isr() -> None:\n    global n\n    n += 1\ndef main() -> None:\n    x: u8 = n\n";
    assert!(analyze(src, &TargetProfile::generic()).program.is_some());
}

#[test]
fn without_main_uncalled_functions_are_main_context() {
    let src = format!("{ISR}def helper() -> None:\n    pass\ndef start() -> None:\n    helper()\n");
    assert_eq!(count(&src, &TargetProfile::generic(), "PYP2060"), 1);
}

#[test]
fn no_isr_means_no_sharing_check() {
    let src = "def helper() -> None:\n    pass\ndef main() -> None:\n    helper()\n";
    assert!(analyze(src, &TargetProfile::generic()).program.is_some());
}

// ---- #2: semantica confirmada (regresiones) ----------------------------------------

#[test]
fn confirmed_semantics_still_hold() {
    let g = TargetProfile::generic();
    assert_eq!(count("def f(a: u8, b: u16) -> u16:\n    return a + b\n", &g, "PYP2047"), 1); // sin ensanchamiento implicito
    assert_eq!(count("def f(a: u8) -> None:\n    if a:\n        pass\n", &g, "PYP2041"), 1); // sin truthiness
    assert_eq!(count("x: u8 = 200 + 100\n", &g, "PYP2040"), 1); // overflow constante = error
}
