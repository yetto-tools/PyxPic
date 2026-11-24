//! La CLI de punta a punta: el dispositivo lo declara el programa y el pack se resuelve solo.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use pypic_pic14::Image;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().unwrap()
}

fn tmp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("pypic-cli-{}-{name}", std::process::id()));
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
fn build_finds_the_device_pack_from_the_programs_declaration() {
    let out = tmp("build").join("button_led.hex");
    let o = pypic(&root(), &["build", "examples/button_led.py", "-o", out.to_str().unwrap()]);
    assert!(o.status.success(), "{}", text(&o));
    assert!(text(&o).contains("PIC16F887") && text(&o).contains("4000000 Hz"), "{}", text(&o));
    let img = Image::from_intel_hex(&std::fs::read_to_string(&out).unwrap()).unwrap();
    assert_eq!(img.get(0x2007), Some(0x20D4)); // palabra de configuracion en la direccion del pack
    assert!(img.get(0).is_some());
}

#[test]
fn build_works_from_any_directory_with_pypic_devices() {
    let out = tmp("elsewhere").join("c.hex");
    let o = Command::new(env!("CARGO_BIN_EXE_pypic"))
        .current_dir(tmp("elsewhere"))
        .env("PYPIC_DEVICES", root().join("devices"))
        .args(["build", root().join("examples/counter.py").to_str().unwrap(), "-o", out.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(o.status.success(), "{}", text(&o));
    assert!(out.exists());
}

#[test]
fn build_requires_the_declaration() {
    let dir = tmp("nodecl");
    let src = dir.join("p.py");
    std::fs::write(&src, "def main() -> None:\n    pass\n").unwrap();
    let o = pypic(&root(), &["build", src.to_str().unwrap(), "-o", dir.join("p.hex").to_str().unwrap()]);
    assert_eq!(o.status.code(), Some(1));
    assert!(text(&o).contains("from pypic.device import"), "{}", text(&o));
    assert!(!dir.join("p.hex").exists());
    // `check` si funciona sin dispositivo (analisis generico)
    assert!(pypic(&root(), &["check", src.to_str().unwrap()]).status.success());
}

#[test]
fn unknown_device_lists_the_available_ones() {
    let dir = tmp("unknown");
    let src = dir.join("p.py");
    std::fs::write(&src, "from pypic.device import PIC99F999\ndef main() -> None:\n    pass\n").unwrap();
    let o = pypic(&root(), &["build", src.to_str().unwrap()]);
    assert_eq!(o.status.code(), Some(1));
    assert!(text(&o).contains("PIC99F999") && text(&o).contains("PIC16F887"), "{}", text(&o));
}

#[test]
fn a_device_without_a_backend_is_refused_with_a_clear_error() {
    // un pack valido de otro dispositivo (copia con otro nombre): el backend no lo conoce todavia
    let dir = tmp("otherdev");
    let pack = dir.join("devices/pic16f886");
    std::fs::create_dir_all(&pack).unwrap();
    for f in ["device.json", "memory.json", "pins.json", "config.json"] {
        let s = std::fs::read_to_string(root().join("devices/pic16f887").join(f)).unwrap();
        std::fs::write(pack.join(f), s.replace("\"PIC16F887\"", "\"PIC16F886\"")).unwrap();
    }
    let src = dir.join("p.py");
    std::fs::write(&src, "from pypic.device import PIC16F886\ndef main() -> None:\n    pass\n").unwrap();
    let o = pypic(&dir, &["build", src.to_str().unwrap(), "-o", dir.join("p.hex").to_str().unwrap()]);
    assert_eq!(o.status.code(), Some(1), "{}", text(&o));
    assert!(text(&o).contains("no soporta") && text(&o).contains("PIC16F886"), "{}", text(&o));
}

#[test]
fn declaring_one_device_and_forcing_another_pack_is_an_error() {
    let dir = tmp("mismatch");
    let src = dir.join("p.py");
    std::fs::write(&src, "from pypic.device import PIC12F625\ndef main() -> None:\n    pass\n").unwrap();
    let o = pypic(&root(), &["check", src.to_str().unwrap(), "--device", "devices/pic16f887"]);
    assert_eq!(o.status.code(), Some(1));
    assert!(text(&o).contains("PYP2090"), "{}", text(&o));
}

#[test]
fn clock_above_the_device_maximum_is_reported_with_the_source_location() {
    let dir = tmp("clock");
    let src = dir.join("p.py");
    std::fs::write(&src, "from pypic.device import PIC16F887\nCLOCK_HZ: Final[u32] = 25_000_000\ndef main() -> None:\n    pass\n").unwrap();
    let o = pypic(&root(), &["check", src.to_str().unwrap()]);
    assert_eq!(o.status.code(), Some(1));
    assert!(text(&o).contains(":2:1: error[PYP2094]") && text(&o).contains("20000000"), "{}", text(&o));
}

#[test]
fn pins_are_validated_against_the_declared_device() {
    let dir = tmp("pins");
    let src = dir.join("p.py");
    std::fs::write(&src, "from pypic.device import PIC16F887\nfrom pypic.periph.gpio import RD9\ndef main() -> None:\n    pass\n").unwrap();
    let o = pypic(&root(), &["check", src.to_str().unwrap()]);
    assert_eq!(o.status.code(), Some(1));
    assert!(text(&o).contains("PYP2074") && text(&o).contains("RD0"), "{}", text(&o));
}

#[test]
fn the_shipped_examples_all_build() {
    for ex in ["examples/counter.py", "examples/button_led.py"] {
        let out = tmp("examples").join(format!("{}.hex", Path::new(ex).file_stem().unwrap().to_string_lossy()));
        let o = pypic(&root(), &["build", ex, "-o", out.to_str().unwrap()]);
        assert!(o.status.success(), "{ex}: {}", text(&o));
    }
}

// ---- esperas: reloj por defecto, una sola forma, informe de tiempos ----------------------------------

#[test]
fn blink_example_builds_and_the_timing_report_shows_exact_cycles() {
    let out = tmp("blink").join("blink.hex");
    let o = pypic(&root(), &["build", "examples/blink.py", "--timing", "-o", out.to_str().unwrap()]);
    assert!(o.status.success(), "{}", text(&o));
    let t = text(&o);
    assert!(t.contains("500 ms -> 500000 ciclos") && t.contains("+0.0 ns"), "{t}");
    assert!(out.exists());
}

#[test]
fn a_missing_clock_hz_uses_the_device_default_and_says_so() {
    let dir = tmp("noclock");
    let src = dir.join("p.py");
    std::fs::write(&src, "from pypic.device import PIC16F887\nfrom pypic.system.time import pause\ndef main() -> None:\n    pause(1)\n").unwrap();
    let o = pypic(&root(), &["build", src.to_str().unwrap(), "-o", dir.join("p.hex").to_str().unwrap()]);
    assert!(o.status.success(), "{}", text(&o));
    let t = text(&o);
    assert!(t.contains("PYP3004") && t.contains("(por defecto)") && t.contains("4000000"), "{t}");
}

#[test]
fn removed_aliases_point_to_the_single_name() {
    let dir = tmp("alias");
    let src = dir.join("p.py");
    std::fs::write(&src, "from pypic.device import PIC16F887\nfrom pypic.system.time import delay_ms\ndef main() -> None:\n    pass\n").unwrap();
    let o = pypic(&root(), &["check", src.to_str().unwrap()]);
    assert_eq!(o.status.code(), Some(1));
    assert!(text(&o).contains("PYP2104") && text(&o).contains("pause"), "{}", text(&o));
}
