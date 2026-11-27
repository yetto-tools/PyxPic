//! `pyxpic monitor`: la terminal serial sin hardware (la lista de puertos y la salida por UART de un programa en el simulador).

use std::process::Command;

fn pyxpic(args: &[&str]) -> (bool, String, String) {
    let o = Command::new(env!("CARGO_BIN_EXE_pyxpic")).current_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/../..")).args(args).output().unwrap();
    (o.status.success(), String::from_utf8_lossy(&o.stdout).into_owned(), String::from_utf8_lossy(&o.stderr).into_owned())
}

fn example(rel: &str) -> String {
    format!("{}/../../examples/{rel}", env!("CARGO_MANIFEST_DIR"))
}

#[test]
fn the_port_list_is_versioned_json() {
    let (ok, out, _) = pyxpic(&["monitor", "--list", "--format", "json"]);
    assert!(ok);
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["version"], 1);
    assert!(v["ports"].is_array());
}

#[test]
fn a_missing_port_is_a_clear_error() {
    let (ok, _, err) = pyxpic(&["monitor", "--port", "NO_EXISTE_9", "--baud", "9600"]);
    assert!(!ok);
    assert!(err.contains("NO_EXISTE_9"), "{err}");
}

#[test]
fn bad_options_explain_themselves() {
    let (ok, _, err) = pyxpic(&["monitor", "--port", "COM1", "--parity", "raro"]);
    assert!(!ok && err.contains("none, even u odd"), "{err}");
    let (ok, _, err) = pyxpic(&["monitor", "--baud", "9600"]);
    assert!(!ok && err.contains("hace falta --port"), "{err}");
}

#[test]
fn sim_shows_the_program_uart_output_with_timestamps_and_jsonl() {
    let file = example("basic/uart/saludo.bas");
    if !std::path::Path::new(&file).exists() {
        return;
    }
    let (ok, out, err) = pyxpic(&["monitor", "--sim", &file, "--seconds", "2", "--timestamps"]);
    assert!(ok, "{err}{out}");
    assert!(out.lines().any(|l| l.starts_with("[00:00.") && l.len() > 12), "{out}");
    let (ok, out, err) = pyxpic(&["monitor", "--sim", &file, "--seconds", "2", "--format", "jsonl"]);
    assert!(ok, "{err}");
    let events: Vec<serde_json::Value> = out.lines().map(|l| serde_json::from_str(l).unwrap()).collect();
    assert!(events.iter().any(|e| e["type"] == "data"), "{out}");
    let (_, hexed, _) = pyxpic(&["monitor", "--sim", &file, "--seconds", "2", "--encoding", "hex"]);
    assert!(hexed.lines().next().is_some_and(|l| l.split(' ').all(|b| b.len() == 2)), "{hexed}");
}
