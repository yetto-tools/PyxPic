//! Profundidad de la pila de retorno de hardware (PYP2061).

use pyxpic_frontend::{analyze, TargetProfile};

fn pic16() -> TargetProfile {
    TargetProfile::new("pic16-like", 8, 32).with_stack_levels(8)
}

/// main -> f0 -> f1 -> ... -> f{n-1}: `n` llamadas anidadas bajo main.
fn chain(n: usize) -> String {
    let mut s = String::new();
    for i in 0..n {
        s.push_str(&format!("def f{i}() -> None:\n    {}\n", if i + 1 < n { format!("f{}()", i + 1) } else { "pass".into() }));
    }
    s.push_str(&format!("def main() -> None:\n    {}\n", if n > 0 { "f0()" } else { "pass" }));
    s
}

fn isr_chain(depth: usize) -> String {
    // la ISR llama a g0 -> g1 -> ... (depth llamadas anidadas)
    let mut s = String::new();
    for i in 0..depth {
        s.push_str(&format!("def g{i}() -> None:\n    {}\n", if i + 1 < depth { format!("g{}()", i + 1) } else { "pass".into() }));
    }
    s.push_str(&format!("@interrupt\ndef isr() -> None:\n    {}\n", if depth > 0 { "g0()" } else { "pass" }));
    s
}

fn codes(src: &str, t: &TargetProfile) -> Vec<&'static str> {
    analyze(src, t).diagnostics.codes()
}

#[test]
fn main_chain_at_the_limit_passes_and_one_more_fails() {
    // el arranque cuenta 1: main + 7 llamadas anidadas = 1 + 7 = 8 niveles
    assert!(analyze(&chain(7), &pic16()).program.is_some());
    assert_eq!(codes(&chain(8), &pic16()), vec!["PYP2061"]);
}

#[test]
fn isr_adds_its_entry_and_its_own_calls() {
    // main: 1 + 5 = 6; ISR: 1 + 1 = 2; total 8 -> cabe
    let src = format!("{}{}", chain(5), isr_chain(1).replace("def g0", "def h0").replace("g0()", "h0()"));
    assert!(analyze(&src, &pic16()).program.is_some(), "{:?}", codes(&src, &pic16()));
    // main: 1 + 6 = 7; ISR: 1 + 1 = 2; total 9 -> no cabe
    let src = format!("{}{}", chain(6), isr_chain(1).replace("def g0", "def h0").replace("g0()", "h0()"));
    assert_eq!(codes(&src, &pic16()), vec!["PYP2061"]);
}

#[test]
fn isr_without_calls_costs_one_level() {
    // main: 1 + 6 = 7; ISR sin llamadas: 1; total 8 -> cabe. Con 7 llamadas: 9 -> error
    let src = format!("{}@interrupt\ndef isr() -> None:\n    pass\n", chain(6));
    assert!(analyze(&src, &pic16()).program.is_some());
    let src = format!("{}@interrupt\ndef isr() -> None:\n    pass\n", chain(7));
    assert_eq!(codes(&src, &pic16()), vec!["PYP2061"]);
}

#[test]
fn widest_branch_counts_not_the_sum() {
    let src = "def a() -> None:\n    pass\ndef b() -> None:\n    a()\ndef c() -> None:\n    a()\n\
               def main() -> None:\n    b()\n    c()\n    a()\n";
    // profundidad maxima de main = 2 (main -> b -> a); uso = 3
    assert!(analyze(src, &TargetProfile::new("t", 8, 32).with_stack_levels(3)).program.is_some());
    assert_eq!(codes(src, &TargetProfile::new("t", 8, 32).with_stack_levels(2)), vec!["PYP2061"]);
}

#[test]
fn no_limit_means_no_check() {
    assert!(analyze(&chain(30), &TargetProfile::generic()).program.is_some());
}

#[test]
fn message_reports_numbers() {
    let r = analyze(&chain(8), &pic16());
    let msg = &r.diagnostics.items[0].message;
    assert!(msg.contains("8 niveles") && msg.contains("necesita 9"), "{msg}");
}

#[test]
fn recursion_does_not_hang_the_depth_check() {
    let src = "def f() -> None:\n    f()\ndef main() -> None:\n    f()\n";
    let c = codes(src, &pic16());
    assert!(c.contains(&"PYP2013"), "{c:?}");
}
