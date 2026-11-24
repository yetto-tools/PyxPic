use std::process::ExitCode;

use pypic_frontend::{analyze, TargetProfile};

const USAGE: &str = "uso: pypic <check|ir> <archivo.py> [--native-bits N] [--max-int-bits N]";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let cmd = args.first().map(String::as_str).unwrap_or("");
    if cmd != "check" && cmd != "ir" {
        eprintln!("{USAGE}");
        return ExitCode::from(2);
    }
    let mut file = None;
    let mut target = TargetProfile::generic();
    let mut it = args.iter().skip(1);
    while let Some(a) = it.next() {
        if a == "--native-bits" {
            match it.next().and_then(|v| v.parse().ok()) {
                Some(n) => target.native_bits = n,
                None => {
                    eprintln!("{USAGE}");
                    return ExitCode::from(2);
                }
            }
        } else if a == "--max-int-bits" {
            match it.next().and_then(|v| v.parse().ok()) {
                Some(n) => {
                    target.max_int_bits = n;
                    target.name = format!("custom-{}bit", target.native_bits);
                }
                None => {
                    eprintln!("{USAGE}");
                    return ExitCode::from(2);
                }
            }
        } else if file.is_none() {
            file = Some(a.clone());
        } else {
            eprintln!("{USAGE}");
            return ExitCode::from(2);
        }
    }
    let Some(file) = file else {
        eprintln!("{USAGE}");
        return ExitCode::from(2);
    };
    let src = match std::fs::read_to_string(&file) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("no se pudo leer '{file}': {e}");
            return ExitCode::from(2);
        }
    };
    let res = analyze(&src, &target);
    print!("{}", res.diagnostics.render(&src, &file));
    let Some(program) = res.program else { return ExitCode::from(1) };
    if cmd == "check" {
        println!("{file}: ok");
        return ExitCode::SUCCESS;
    }
    let module = pypic_ir::lower(&program);
    if let Err(errs) = pypic_ir::verify(&module) {
        eprintln!("error interno: la IR generada no es valida:
  {}", errs.join("
  "));
        return ExitCode::from(70);
    }
    print!("{}", pypic_ir::print(&module));
    ExitCode::SUCCESS
}
