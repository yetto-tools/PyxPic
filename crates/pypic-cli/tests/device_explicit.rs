//! El dispositivo se declara en el codigo (`from pypic.device import PIC16F887`): es lo que dice para que PIC esta escrito el programa y, por tanto,
//! que pines, perifericos y registros se esperan. Aqui se comprueba que falte se vea (`PYP3017`, `PYP3018`) y que lo que generamos (ejemplos,
//! documentacion y plantillas) siempre lo lleve.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().unwrap()
}

fn tmp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("pypic-dev-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn pypic(cwd: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_pypic")).current_dir(cwd).args(args).output().unwrap()
}

fn text(o: &Output) -> String {
    format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr))
}

const BODY: &str = "from pypic.periph.gpio import RD0\nCLOCK: Internal[4 * MHz]\n\n\ndef main() -> None:\n    RD0.output()\n    while True:\n        RD0.toggle()\n";

#[test]
fn check_without_a_device_says_it_is_not_validating_anything() {
    let dir = tmp("sin");
    std::fs::write(dir.join("p.py"), BODY).unwrap();
    let o = pypic(&dir, &["check", "p.py"]);
    let t = text(&o);
    assert!(o.status.success(), "es un aviso, no un error: {t}");
    assert!(t.contains("warning[PYP3017]") && t.contains("no se declaro el dispositivo") && t.contains("NO comprueba") && t.contains("from pypic.device import PIC16F887"), "{t}");
    assert!(t.contains("p.py: ok"), "{t}");
}

#[test]
fn the_json_report_carries_the_same_warning() {
    let dir = tmp("json");
    std::fs::write(dir.join("p.py"), BODY).unwrap();
    let o = pypic(&dir, &["check", "p.py", "--format", "json"]);
    let v: serde_json::Value = serde_json::from_slice(&o.stdout).expect("un solo documento JSON");
    assert_eq!(v["ok"], true);
    assert_eq!(v["summary"]["warnings"], 1, "{v}");
    assert_eq!(v["diagnostics"][0]["code"], "PYP3017", "{v}");
    assert_eq!(v["diagnostics"][0]["severity"], "warning");
}

#[test]
fn a_program_that_declares_its_device_gets_no_such_warning() {
    let dir = tmp("con");
    std::fs::write(dir.join("p.py"), format!("from pypic.device import PIC16F887\n{BODY}")).unwrap();
    let o = pypic(&root(), &["check", dir.join("p.py").to_str().unwrap()]);
    let t = text(&o);
    assert!(o.status.success() && !t.contains("PYP3017") && !t.contains("PYP3018"), "{t}");
}

fn project(name: &str, main: &str) -> PathBuf {
    let dir = tmp(name);
    let devices = root().join("devices").to_string_lossy().replace('\\', "/");
    std::fs::write(dir.join("pypic.toml"), format!("[project]\nname = \"t\"\nmain = \"main.py\"\ndevices_dir = \"{devices}\"\ndefault_target = \"a\"\n\n[target.a]\ndevice = \"PIC16F887\"\n")).unwrap();
    std::fs::write(dir.join("main.py"), main).unwrap();
    dir
}

#[test]
fn a_device_that_only_comes_from_the_profile_is_flagged_because_the_code_does_not_say_it() {
    let dir = project("perfil", BODY);
    let o = pypic(&dir, &["check"]);
    let t = text(&o);
    assert!(o.status.success(), "{t}");
    assert!(t.contains("warning[PYP3018]") && t.contains("PIC16F887") && t.contains("el codigo diga que PIC espera"), "{t}");
    assert!(!t.contains("PYP3017"), "el dispositivo si existe (del perfil): {t}");
    // declarado en el codigo y de acuerdo con el perfil: sin aviso
    let dir = project("perfil-ok", &format!("from pypic.device import PIC16F887\n{BODY}"));
    let t = text(&pypic(&dir, &["check"]));
    assert!(!t.contains("PYP3017") && !t.contains("PYP3018"), "{t}");
}

#[test]
fn init_writes_a_program_that_declares_the_device_and_is_clean() {
    let dir = tmp("init");
    let devices = root().join("devices");
    let o = pypic(&root(), &["init", dir.to_str().unwrap(), "--devices-dir", devices.to_str().unwrap()]);
    assert!(o.status.success(), "{}", text(&o));
    let main = std::fs::read_to_string(dir.join("main.py")).unwrap();
    assert!(main.contains("from pypic.device import PIC16F887"), "{main}");
    let t = text(&pypic(&dir, &["check"]));
    assert!(!t.contains("PYP3017") && !t.contains("PYP3018"), "{t}");
    // con otro dispositivo el codigo dice cual es
    let dir6 = tmp("init6");
    let o = pypic(&root(), &["init", dir6.to_str().unwrap(), "--device", "PIC16F886", "--devices-dir", devices.to_str().unwrap()]);
    if o.status.success() {
        assert!(std::fs::read_to_string(dir6.join("main.py")).unwrap().contains("from pypic.device import PIC16F886"));
    }
}

// ---- guardas: lo que generamos siempre declara el dispositivo ----------------------------------------------------------------

fn python_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for e in std::fs::read_dir(dir).unwrap().flatten() {
        let p = e.path();
        if p.is_dir() {
            if p.file_name().is_some_and(|n| n != "build" && n != "hex") {
                python_files(&p, out);
            }
        } else if p.extension().is_some_and(|x| x == "py") {
            out.push(p);
        }
    }
}

#[test]
fn every_shipped_example_declares_its_device_in_the_code() {
    let mut files = Vec::new();
    python_files(&root().join("examples"), &mut files);
    assert!(files.len() >= 20, "se esperaban los ejemplos: {}", files.len());
    let missing: Vec<String> = files
        .iter()
        .filter(|p| !std::fs::read_to_string(p).unwrap().lines().any(|l| l.starts_with("from pypic.device import ")))
        .map(|p| p.strip_prefix(root()).unwrap().display().to_string())
        .collect();
    assert!(missing.is_empty(), "estos ejemplos no declaran el dispositivo: {missing:?}");
}

#[test]
fn every_documentation_snippet_that_imports_hardware_modules_declares_the_device() {
    let mut bad = Vec::new();
    for e in std::fs::read_dir(root().join("docs")).unwrap().flatten() {
        let p = e.path();
        if p.extension().is_none_or(|x| x != "md") {
            continue;
        }
        let s = String::from_utf8_lossy(&std::fs::read(&p).unwrap()).replace("\r\n", "\n");
        let mut rest = s.as_str();
        while let Some(i) = rest.find("```python\n") {
            let body = &rest[i + 10..];
            let end = body.find("```").unwrap_or(body.len());
            let block = &body[..end];
            let imports_hw = block.lines().any(|l| l.starts_with("from pypic.periph") || l.starts_with("from pypic.regs") || l.starts_with("from pypic.system"));
            if imports_hw && !block.contains("from pypic.device import ") {
                bad.push(format!("{}: {}", p.file_name().unwrap().to_string_lossy(), block.lines().next().unwrap_or("")));
            }
            rest = &body[end..];
        }
    }
    assert!(bad.is_empty(), "fragmentos sin 'from pypic.device import ...': {bad:?}");
}
