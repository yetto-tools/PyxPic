//! `.bas` en la CLI: la extension elige el frontend; check, build y los proyectos funcionan igual que con Python.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().unwrap()
}

fn tmp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("pypic-bas-{}-{name}", std::process::id()));
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

#[test]
fn the_bas_examples_check_and_build() {
    let dir = tmp("examples");
    for name in ["gpio/parpadeo", "adc/sensor"] {
        let src = root().join(format!("examples/basic/{name}.bas"));
        let r = pypic(&root(), &["check", src.to_str().unwrap()]);
        assert!(r.status.success(), "{name}: {}", text(&r));
        let out = dir.join(format!("{name}.hex"));
        let r = pypic(&root(), &["build", src.to_str().unwrap(), "-o", out.to_str().unwrap()]);
        assert!(r.status.success(), "{name}: {}", text(&r));
        assert!(std::fs::read_to_string(&out).unwrap().starts_with(':'));
    }
}

#[test]
fn a_basic_error_names_the_bas_file_and_line() {
    let dir = tmp("error");
    std::fs::write(dir.join("mal.bas"), "DEVICE PIC16F887\nCLOCK INTERNAL 4 MHZ\nb0 = 300\n").unwrap();
    let r = pypic(&dir, &["check", "mal.bas", "--devices-dir", root().join("devices").to_str().unwrap()]);
    assert_eq!(r.status.code(), Some(1), "{}", text(&r));
    assert!(text(&r).contains("mal.bas:3:6: error[PYB0040]"), "{}", text(&r));
}

#[test]
fn a_project_can_use_a_bas_main_and_clean_removes_its_hex() {
    let dir = tmp("project");
    std::fs::write(dir.join("main.bas"), std::fs::read_to_string(root().join("examples/basic/gpio/parpadeo.bas")).unwrap()).unwrap();
    std::fs::write(
        dir.join("pypic.toml"),
        format!(
            "[project]\nname = \"b\"\nmain = \"main.bas\"\ndevices_dir = {:?}\n\n[target.x]\ndevice = \"PIC16F887\"\noutput = \"build/b.hex\"\n",
            root().join("devices").to_string_lossy().replace(std::path::MAIN_SEPARATOR, "/")
        ),
    )
    .unwrap();
    let r = pypic(&dir, &["build"]);
    assert!(r.status.success(), "{}", text(&r));
    assert!(dir.join("build/b.hex").is_file());
    let r = pypic(&dir, &["project", "check"]);
    assert!(r.status.success(), "{}", text(&r));
    let r = pypic(&dir, &["clean"]);
    assert!(r.status.success() && !dir.join("build/b.hex").exists(), "{}", text(&r));
    // sin .py a la vista: el programa .bas es lo que `clean programa.bas` limpia
    std::fs::write(dir.join("solo.hex"), ":00000001FF\n").unwrap();
    let r = pypic(&dir, &["clean", "solo.bas", "--project", dir.join("pypic.toml").to_str().unwrap(), "-o", "solo.hex"]);
    assert!(r.status.success() && !dir.join("solo.hex").exists(), "{}", text(&r));
}
