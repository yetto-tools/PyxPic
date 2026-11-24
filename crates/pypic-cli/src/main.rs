use std::path::Path;
use std::process::ExitCode;

use pypic_device::DevicePack;
use pypic_frontend::{analyze, TargetProfile};

const USAGE: &str = "uso:
  pypic check <archivo.py> [opciones]
  pypic ir <archivo.py> [opciones]
  pypic device validate <directorio-del-pack>

opciones:
  --device <directorio>   usa el Device Pack (p. ej. devices/pic16f887)
  --native-bits N         ancho nativo del destino (sobrescribe al pack)
  --max-int-bits N        ancho maximo de entero (sobrescribe al pack)";

fn usage() -> ExitCode {
    eprintln!("{USAGE}");
    ExitCode::from(2)
}

fn load_pack(dir: &str) -> Result<DevicePack, ExitCode> {
    let pack = DevicePack::load(Path::new(dir)).map_err(|e| {
        eprintln!("{e}");
        ExitCode::from(2)
    })?;
    let errs = pack.validate();
    if !errs.is_empty() {
        eprintln!("{dir}: el Device Pack no es valido:");
        for e in &errs {
            eprintln!("  - {e}");
        }
        return Err(ExitCode::from(1));
    }
    Ok(pack)
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("device") => device(&args[1..]),
        Some(c @ ("check" | "ir")) => compile(c, &args[1..]),
        _ => usage(),
    }
}

fn device(args: &[String]) -> ExitCode {
    let (Some("validate"), Some(dir), None) = (args.first().map(String::as_str), args.get(1), args.get(2)) else {
        return usage();
    };
    match load_pack(dir) {
        Ok(p) => {
            println!(
                "{dir}: ok ({}, {}-bit, {} palabras de programa, {} B de RAM, {} pines de E/S)",
                p.device.name, p.device.cpu.data_bits, p.memory.program.words, p.memory.data.ram_bytes, p.device.io_pins
            );
            ExitCode::SUCCESS
        }
        Err(code) => code,
    }
}

fn compile(cmd: &str, args: &[String]) -> ExitCode {
    let mut file = None;
    let mut device_dir = None;
    let mut native = None;
    let mut max = None;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--device" => device_dir = it.next().cloned(),
            "--native-bits" => native = it.next().and_then(|v| v.parse::<u32>().ok()),
            "--max-int-bits" => max = it.next().and_then(|v| v.parse::<u32>().ok()),
            s if s.starts_with("--") => return usage(),
            _ if file.is_none() => file = Some(a.clone()),
            _ => return usage(),
        }
    }
    let Some(file) = file else { return usage() };

    let mut target = match &device_dir {
        Some(dir) => match load_pack(dir) {
            Ok(p) => p.target_profile(),
            Err(code) => return code,
        },
        None => TargetProfile::generic(),
    };
    if let Some(n) = native {
        target.native_bits = n;
    }
    if let Some(m) = max {
        target.max_int_bits = m;
    }

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
        eprintln!("error interno: la IR generada no es valida:\n  {}", errs.join("\n  "));
        return ExitCode::from(70);
    }
    print!("{}", pypic_ir::print(&module));
    ExitCode::SUCCESS
}
