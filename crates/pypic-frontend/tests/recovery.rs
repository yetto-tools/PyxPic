//! #4: con errores de sintaxis tambien se reportan los errores de tipos de lo que el parser si leyo, sin diagnosticos falsos que dependan de lo
//! que la recuperacion descarto, y sin romperse con un AST incompleto.

use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::{Path, PathBuf};

use pypic_frontend::{analyze, Severity, TargetProfile};

fn run(src: &str) -> (Vec<&'static str>, String, bool) {
    let r = analyze(src, &TargetProfile::generic());
    (r.diagnostics.codes(), r.diagnostics.render(src, "t.py"), r.program.is_some())
}

#[test]
fn a_syntax_error_and_a_type_error_in_the_same_file_are_both_reported() {
    let (codes, shown, program) = run("x: uint8 = 300\n\n\ndef main() -> None:\n    y = = 5\n");
    assert!(!program, "con un error de sintaxis no hay programa");
    assert!(codes.contains(&"PYP1100"), "el error de sintaxis: {codes:?}");
    assert!(codes.contains(&"PYP2040"), "el error de tipos: {codes:?}");
    assert!(shown.contains("t.py:1:") && shown.contains("t.py:5:"), "cada uno con su linea:\n{shown}");
    // salen ordenados por posicion
    assert!(shown.find("t.py:1:").unwrap() < shown.find("t.py:5:").unwrap());
}

#[test]
fn what_the_parser_dropped_does_not_cause_false_undefined_name_errors() {
    // la definicion rota de `helper` se descarta: llamarla no debe dar "funcion no definida"
    let (codes, _, _) = run("def helper() -> None\n    pass\n\n\ndef main() -> None:\n    helper()\n    z: uint8 = 300\n");
    assert!(codes.contains(&"PYP1100") && codes.contains(&"PYP2040"), "{codes:?}");
    assert!(!codes.contains(&"PYP2043"), "no hay 'no definida' falso: {codes:?}");
    // una declaracion global rota: usarla o declararla `global` no da errores falsos
    let (codes, _, _) = run("a: uint8 = = 1\n\n\ndef main() -> None:\n    global a\n    a = 2\n    b: uint8 = 999\n");
    assert!(codes.contains(&"PYP1100") && codes.contains(&"PYP2040"), "{codes:?}");
    for falso in ["PYP2043", "PYP2022", "PYP2020", "PYP2023"] {
        assert!(!codes.contains(&falso), "{falso} seria falso: {codes:?}");
    }
    // el inicio de la UART en una funcion descartada no hace que parezca que falta
    let (codes, _, _) = run("from pypic.periph.uart import UART\n\n\ndef setup() -> None\n    UART.start(baud=9600)\n\n\ndef main() -> None:\n    UART.write(65)\n    q: uint8 = 300\n");
    assert!(codes.contains(&"PYP2040"), "{codes:?}");
    assert!(!codes.contains(&"PYP2181"), "UART sin start seria falso: {codes:?}");
}

#[test]
fn only_errors_are_added_not_warnings_and_a_real_undefined_name_still_counts_without_syntax_errors() {
    // un aviso de coste (uint16) no se mezcla con un error de sintaxis
    let r = analyze("v: uint16 = 1\n\n\ndef main() -> None:\n    q: uint8 = = 5\n", &TargetProfile::generic());
    assert!(r.diagnostics.items.iter().all(|d| d.severity == Severity::Error), "{:?}", r.diagnostics.codes());
    // sin errores de sintaxis todo sigue igual: el nombre no definido SI se reporta
    let (codes, _, _) = run("def main() -> None:\n    x: uint8 = nope\n");
    assert!(codes.contains(&"PYP2043"), "{codes:?}");
    // y un programa correcto sigue dando programa
    let (codes, _, program) = run("def main() -> None:\n    _x: uint8 = 1\n");
    assert!(codes.is_empty() && program);
}

#[test]
fn only_the_syntax_error_is_reported_when_nothing_else_is_wrong() {
    let (codes, _, program) = run("def main() -> None:\n    x: uint8 = = 5\n");
    assert_eq!(codes, vec!["PYP1100"]);
    assert!(!program);
}

fn python_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for e in std::fs::read_dir(dir).unwrap().flatten() {
        let p = e.path();
        if p.is_dir() {
            python_files(&p, out);
        } else if p.extension().is_some_and(|x| x == "py") {
            out.push(p);
        }
    }
}

#[test]
fn breaking_any_line_of_any_example_never_crashes_the_checker() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples");
    let mut files = Vec::new();
    python_files(&root, &mut files);
    assert!(files.len() > 10, "se encontraron los ejemplos");
    let target = TargetProfile::generic();
    let mut checked = 0;
    for file in files {
        let src = std::fs::read_to_string(&file).unwrap();
        let lines: Vec<&str> = src.lines().collect();
        for i in 0..lines.len() {
            let broken = {
                // tres roturas distintas de la misma linea: basura, borrarla y cortar el archivo ahi
                let garbage = lines.iter().enumerate().map(|(k, l)| if k == i { "@@@ = = (" } else { l }).collect::<Vec<_>>().join("\n");
                let removed = lines.iter().enumerate().filter(|(k, _)| *k != i).map(|(_, l)| *l).collect::<Vec<_>>().join("\n");
                let cut = lines[..i].join("\n");
                [garbage, removed, cut]
            };
            for (k, text) in broken.iter().enumerate() {
                let result = catch_unwind(AssertUnwindSafe(|| analyze(text, &target)));
                assert!(result.is_ok(), "el verificador se cayo en {} linea {} (rotura {k})", file.display(), i + 1);
                checked += 1;
            }
        }
    }
    assert!(checked > 2000, "{checked} programas rotos comprobados");
}
