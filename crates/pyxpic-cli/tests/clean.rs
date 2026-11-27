//! `pyxpic clean`: borra exactamente la salida que `build` escribe (el `.hex`) y nada mas.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().unwrap()
}

fn tmp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("pyxpic-clean-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn pyxpic(cwd: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_pyxpic")).current_dir(cwd).args(args).output().unwrap()
}

fn text(o: &Output) -> String {
    format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr))
}

const PROGRAM: &str =
    "from pyxpic.device import PIC16F887\nfrom pyxpic.periph.gpio import RD0\n\nCLOCK: Internal
FREQUENCY: 4MHz\n\n\ndef main() -> None:\n    RD0.output()\n    while True:\n        RD0.toggle()\n";

fn project(dir: &Path, targets: &str) {
    std::fs::write(dir.join("pyxpic.toml"), format!("[project]\nname = \"p\"\nmain = \"main.py\"\ndevices_dir = {:?}\n{targets}", root().join("devices").to_string_lossy().replace('\\', "/")))
        .unwrap();
    std::fs::write(dir.join("main.py"), PROGRAM).unwrap();
}

#[test]
fn a_loose_program_loses_the_hex_next_to_it_and_nothing_else() {
    let dir = tmp("loose");
    std::fs::write(dir.join("prog.py"), PROGRAM).unwrap();
    let built = pyxpic(&dir, &["build", "prog.py", "--devices-dir", root().join("devices").to_str().unwrap()]);
    assert!(built.status.success(), "{}", text(&built));
    assert!(dir.join("prog.hex").is_file());
    std::fs::write(dir.join("otro.hex"), ":00000001FF\n").unwrap();
    let r = pyxpic(&dir, &["clean", "prog.py"]);
    assert!(r.status.success(), "{}", text(&r));
    assert!(!dir.join("prog.hex").exists(), "se borra la salida de build");
    assert!(dir.join("prog.py").is_file() && dir.join("otro.hex").is_file(), "ni el fuente ni otro .hex");
    assert!(text(&r).contains("1 archivo(s) borrados"), "{}", text(&r));
    // otra vez: nada que borrar, y no es un error
    let again = pyxpic(&dir, &["clean", "prog.py"]);
    assert!(again.status.success() && text(&again).contains("no existe") && text(&again).contains("0 archivo(s) borrados"), "{}", text(&again));
}

#[test]
fn dash_o_names_the_file_exactly_like_build() {
    let dir = tmp("dash-o");
    std::fs::write(dir.join("fw.hex"), ":00000001FF\n").unwrap();
    std::fs::write(dir.join("keep.hex"), ":00000001FF\n").unwrap();
    let r = pyxpic(&dir, &["clean", "-o", "fw.hex"]);
    assert!(r.status.success(), "{}", text(&r));
    assert!(!dir.join("fw.hex").exists() && dir.join("keep.hex").exists());
}

#[test]
fn dry_run_lists_without_deleting() {
    let dir = tmp("dry");
    std::fs::write(dir.join("fw.hex"), ":00000001FF\n").unwrap();
    let r = pyxpic(&dir, &["clean", "-o", "fw.hex", "--dry-run"]);
    assert!(r.status.success(), "{}", text(&r));
    assert!(dir.join("fw.hex").exists());
    assert!(text(&r).contains("se borraria") && text(&r).contains("1 archivo(s) se borrarian"), "{}", text(&r));
}

#[test]
fn a_project_cleans_every_profile_output_or_just_the_asked_one() {
    let dir = tmp("project");
    project(
        &dir,
        "default_target = \"a\"\n\n[target.a]\ndevice = \"PIC16F887\"\noutput = \"build/a.hex\"\n\n[target.b]\ndevice = \"PIC16F887\"\nclock = \"Crystal 20MHz\"\noutput = \"build/b.hex\"\n",
    );
    for t in ["a", "b"] {
        let r = pyxpic(&dir, &["build", "--target", t]);
        assert!(r.status.success(), "{}", text(&r));
        assert!(dir.join(format!("build/{t}.hex")).is_file());
    }
    let r = pyxpic(&dir, &["clean", "--target", "a"]);
    assert!(r.status.success(), "{}", text(&r));
    assert!(!dir.join("build/a.hex").exists() && dir.join("build/b.hex").exists(), "solo el perfil pedido");
    let r = pyxpic(&dir, &["clean"]);
    assert!(r.status.success(), "{}", text(&r));
    assert!(!dir.join("build/b.hex").exists(), "sin --target, todos los perfiles");
    assert!(!dir.join("build").exists(), "la carpeta de salida queda vacia y se quita");
    assert!(dir.join("main.py").is_file() && dir.join("pyxpic.toml").is_file());
}

#[test]
fn a_profile_without_output_cleans_the_hex_next_to_main() {
    let dir = tmp("no-output");
    project(&dir, "\n[target.solo]\ndevice = \"PIC16F887\"\n");
    assert!(pyxpic(&dir, &["build"]).status.success());
    assert!(dir.join("main.hex").is_file());
    let r = pyxpic(&dir, &["clean"]);
    assert!(r.status.success() && !dir.join("main.hex").exists(), "{}", text(&r));
}

#[test]
fn clean_never_touches_sources_or_the_project_file() {
    let dir = tmp("protected");
    project(&dir, "\n[target.mal]\ndevice = \"PIC16F887\"\noutput = \"main.py\"\n");
    let r = pyxpic(&dir, &["clean"]);
    assert_eq!(r.status.code(), Some(2), "{}", text(&r));
    assert!(text(&r).contains("no se borra") && text(&r).contains("fuente"), "{}", text(&r));
    assert!(dir.join("main.py").is_file());
    let r = pyxpic(&dir, &["clean", "-o", "pyxpic.toml"]);
    assert_eq!(r.status.code(), Some(2), "{}", text(&r));
    assert!(dir.join("pyxpic.toml").is_file());
    // un directorio tampoco
    std::fs::create_dir_all(dir.join("carpeta")).unwrap();
    let r = pyxpic(&dir, &["clean", "-o", "carpeta"]);
    assert_eq!(r.status.code(), Some(2), "{}", text(&r));
    assert!(dir.join("carpeta").is_dir());
}

#[test]
fn usage_errors_are_clear() {
    let dir = tmp("errors");
    let r = pyxpic(&dir, &["clean"]);
    assert_eq!(r.status.code(), Some(2));
    assert!(text(&r).contains("necesita un programa"), "{}", text(&r));
    let r = pyxpic(&dir, &["clean", "prog.py", "--target", "x"]);
    assert_eq!(r.status.code(), Some(2));
    assert!(text(&r).contains("--target necesita un pyxpic.toml"), "{}", text(&r));
    project(&dir, "\n[target.a]\ndevice = \"PIC16F887\"\n");
    let r = pyxpic(&dir, &["clean", "--target", "zzz"]);
    assert_eq!(r.status.code(), Some(2));
    assert!(text(&r).contains("no hay un perfil 'zzz'"), "{}", text(&r));
}
