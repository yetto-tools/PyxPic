//! `pyxpic memory`: el informe de memoria coincide con la imagen que `pyxpic build` escribe y dice cuando es parcial.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use pyxpic_pic14::Image;
use serde_json::Value;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().unwrap()
}

fn tmp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("pyxpic-mem-{}-{name}", std::process::id()));
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

fn json(cwd: &Path, args: &[&str]) -> Value {
    let mut a = vec!["memory"];
    a.extend_from_slice(args);
    a.extend_from_slice(&["--format", "json"]);
    let o = pyxpic(cwd, &a);
    assert!(o.status.success(), "{}", text(&o));
    serde_json::from_slice(&o.stdout).unwrap_or_else(|e| panic!("la salida no es un solo JSON: {e}\n{}", text(&o)))
}

const HEAD: &str = "from pyxpic.device import PIC16F887\nCLOCK: Internal[4 * MHz]\n";

#[test]
fn the_report_matches_the_image_that_build_writes() {
    let dir = tmp("image");
    let src = root().join("examples/python/gpio/blink.py");
    let hex = dir.join("blink.hex");
    assert!(pyxpic(&root(), &["build", src.to_str().unwrap(), "-o", hex.to_str().unwrap()]).status.success());
    let image = Image::from_intel_hex(&std::fs::read_to_string(&hex).unwrap()).unwrap();
    let flash = image.iter().filter(|(a, _)| *a < 0x2000).count() as u64;
    let config = image.iter().filter(|(a, _)| *a >= 0x2000).count() as u64;
    let v = json(&root(), &[src.to_str().unwrap()]);
    assert_eq!(v["version"], 1);
    assert_eq!((v["device"].as_str(), v["complete"].as_bool()), (Some("PIC16F887"), Some(true)));
    assert_eq!(v["program"]["used"], flash, "las palabras de programa son las de la imagen");
    assert_eq!(v["program"]["config_words"], config, "la configuracion no cuenta como programa");
    assert_eq!((v["program"]["capacity"].as_u64(), v["program"]["word_bits"].as_u64()), (Some(8192), Some(14)));
    assert_eq!(v["program"]["free"].as_u64().unwrap() + flash, 8192);
    let highest = image.iter().filter(|(a, _)| *a < 0x2000).map(|(a, _)| a).max().unwrap() as u64 + 1;
    assert_eq!(v["program"]["highest_address_used"], highest);
    assert_eq!(v["ram"]["chip_total"], 368);
    assert_eq!(v["ram"]["banks"].as_array().unwrap().len(), 4);
    assert_eq!(v["pins"], serde_json::json!(["RD0"]));
}

#[test]
fn ram_arrays_and_the_hardware_stack_are_counted() {
    let dir = tmp("ram");
    let src =
        format!("{HEAD}buf: uint8[16]\nn: uint16 = 0\n\n\ndef g() -> None:\n    n_unused: uint8 = 0\n\n\ndef f() -> None:\n    g()\n\n\ndef main() -> None:\n    f()\n    while True:\n        pass\n");
    std::fs::write(dir.join("p.py"), src).unwrap();
    let v = json(&dir, &["p.py", "--details", "--devices-dir", root().join("devices").to_str().unwrap()]);
    // el arranque llama a main (1) y main llama a f y f a g (2 mas): 3 niveles; sin interrupcion
    assert_eq!((v["stack"]["worst_case"].as_u64(), v["stack"]["levels"].as_u64(), v["stack"]["interrupt"].as_u64()), (Some(3), Some(8), Some(0)));
    assert!(v["ram"]["globals"].as_u64().unwrap() >= 18, "16 del arreglo y 2 del uint16: {}", v["ram"]);
    let vars = v["details"]["variables"].as_array().unwrap();
    assert!(vars.iter().any(|x| x["name"] == "buf" && x["bytes"] == 16) && vars.iter().any(|x| x["name"] == "n" && x["bits"] == 16));
    let funcs: Vec<&str> = v["details"]["functions"].as_array().unwrap().iter().map(|f| f["name"].as_str().unwrap()).collect();
    assert!(["main", "f", "g"].iter().all(|n| funcs.contains(n)), "{funcs:?}");
    // lo que se ve en el informe es la suma exacta de las palabras por funcion y el arranque
    let sum: u64 = v["details"]["functions"].as_array().unwrap().iter().map(|f| f["words"].as_u64().unwrap()).sum::<u64>() + v["program"]["startup_and_vectors"].as_u64().unwrap();
    assert_eq!(sum, v["program"]["used"].as_u64().unwrap());
}

#[test]
fn an_asm_function_makes_the_report_partial() {
    let dir = tmp("partial");
    let v = json(&root(), &[root().join("examples/python/asm-en-linea/inline_asm.py").to_str().unwrap()]);
    assert_eq!(v["complete"], false);
    assert!(v["notes"][0].as_str().unwrap().contains("ensamblador"), "{}", v["notes"]);
    let o = pyxpic(&root(), &["memory", root().join("examples/basic/asm-en-linea/intercambiar.bas").to_str().unwrap()]);
    assert!(text(&o).contains("RESULTADO PARCIAL"), "{}", text(&o));
    let _ = dir;
}

#[test]
fn it_works_with_basic_and_in_a_project_and_reports_errors_like_build() {
    let dir = tmp("proj");
    std::fs::write(dir.join("main.bas"), std::fs::read_to_string(root().join("examples/basic/gpio/parpadeo.bas")).unwrap()).unwrap();
    std::fs::write(
        dir.join("pyxpic.toml"),
        format!(
            "[project]\nname = \"m\"\nmain = \"main.bas\"\ndevices_dir = {:?}\n\n[target.x]\ndevice = \"PIC16F887\"\n",
            root().join("devices").to_string_lossy().replace(std::path::MAIN_SEPARATOR, "/")
        ),
    )
    .unwrap();
    let v = json(&dir, &[]);
    assert_eq!((v["device"].as_str(), v["pins"].clone()), (Some("PIC16F887"), serde_json::json!(["RD0"])));
    let o = pyxpic(&dir, &["memory"]);
    assert!(o.status.success() && text(&o).contains("Programa (flash):"), "{}", text(&o));
    // un programa con errores sale como build: sin informe y con codigo 1
    std::fs::write(dir.join("mal.bas"), "DEVICE PIC16F887\nCLOCK INTERNAL 4 MHZ\nb0 = 300\n").unwrap();
    let o = pyxpic(&dir, &["memory", "mal.bas"]);
    assert_eq!(o.status.code(), Some(1), "{}", text(&o));
    assert!(text(&o).contains("PYB0040") && !text(&o).contains("Programa (flash)"));
    // sin dispositivo (ni en el programa ni en un proyecto) no hay capacidades que informar
    let alone = tmp("nodevice");
    std::fs::write(
        alone.join("sin.py"),
        "def main() -> None:
    while True:
        pass
",
    )
    .unwrap();
    let o = pyxpic(&alone, &["memory", "sin.py"]);
    assert_eq!(o.status.code(), Some(1), "{}", text(&o));
    assert!(text(&o).contains("falta el dispositivo"), "{}", text(&o));
}
