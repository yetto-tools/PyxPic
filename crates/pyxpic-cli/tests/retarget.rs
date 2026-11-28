//! `pyxpic devices` y `pyxpic project retarget` (docs/proyecto.md): elegir y cambiar de PIC sin escribir la configuracion a mano.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::Value;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().unwrap()
}

fn pyxpic(cwd: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_pyxpic")).current_dir(cwd).env("PYXPIC_DEVICES", root().join("devices")).args(args).output().unwrap()
}

fn text(o: &Output) -> String {
    format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr))
}

fn project(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("pyxpic-retarget-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    let o = pyxpic(&root(), &["init", d.to_str().unwrap(), "--name", "demo"]);
    assert!(o.status.success(), "{}", text(&o));
    d
}

fn read(d: &Path, file: &str) -> String {
    std::fs::read_to_string(d.join(file)).unwrap()
}

#[test]
fn devices_lists_every_pack_with_clocks_it_accepts() {
    let o = pyxpic(&root(), &["devices", "--format", "json"]);
    assert!(o.status.success(), "{}", text(&o));
    let v: Value = serde_json::from_slice(&o.stdout).unwrap();
    let list = v["devices"].as_array().unwrap();
    let find = |n: &str| list.iter().find(|d| d["name"] == n).unwrap_or_else(|| panic!("falta {n}"));
    let clocks = |n: &str| find(n)["clocks"].as_array().unwrap().iter().map(|c| c["value"].as_str().unwrap().to_string()).collect::<Vec<_>>();
    assert_eq!(find("PIC16F628A")["buildable"], true);
    assert!(find("PIC16F628A")["limits"].is_string());
    assert!(find("PIC16F887")["limits"].is_null());
    assert_eq!(clocks("PIC16F887")[0], "Internal 4MHz");
    assert!(clocks("PIC16F887").contains(&"Internal 8MHz".to_string()));
    assert!(!clocks("PIC16F628A").contains(&"Internal 8MHz".to_string()));
}

#[test]
fn retarget_changes_the_program_and_the_profile_and_it_builds() {
    let d = project("ok");
    let o = pyxpic(&d, &["project", "retarget", "16F628A"]);
    assert!(o.status.success(), "{}", text(&o));
    assert!(read(&d, "main.py").contains("from pyxpic.device import PIC16F628A\n"));
    let toml = read(&d, "pyxpic.toml");
    assert!(toml.contains("[target.pic16f628a]") && toml.contains("device = \"PIC16F628A\""), "{toml}");
    assert!(read(&d, "main.py.bak").contains("PIC16F887"));
    let b = pyxpic(&d, &["build"]);
    assert!(b.status.success() && text(&b).contains("PIC16F628A"), "{}", text(&b));
}

#[test]
fn retarget_writes_nothing_when_the_program_does_not_fit_the_new_chip() {
    let d = project("pin");
    let src = read(&d, "main.py").replace("RB0", "RD0");
    std::fs::write(d.join("main.py"), &src).unwrap();
    let toml = read(&d, "pyxpic.toml");
    let o = pyxpic(&d, &["project", "retarget", "16F628A"]);
    assert_eq!(o.status.code(), Some(1), "{}", text(&o));
    assert!(text(&o).contains("RD0"), "{}", text(&o));
    assert_eq!((read(&d, "main.py"), read(&d, "pyxpic.toml")), (src, toml));
    assert!(!d.join("main.py.bak").exists());
}

#[test]
fn retarget_dry_run_and_unknown_device() {
    let d = project("dry");
    let before = read(&d, "main.py");
    assert!(pyxpic(&d, &["project", "retarget", "16F628A", "--dry-run"]).status.success());
    assert_eq!(read(&d, "main.py"), before);
    let o = pyxpic(&d, &["project", "retarget", "16F999"]);
    assert!(!o.status.success() && text(&o).contains("disponibles"), "{}", text(&o));
}
