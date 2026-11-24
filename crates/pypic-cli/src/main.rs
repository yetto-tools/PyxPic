use std::path::Path;
use std::process::ExitCode;

use pypic_device::DevicePack;
use pypic_frontend::{analyze, TargetProfile};
use pypic_pic14::{disassemble, Image};

const USAGE: &str = "uso:
  pypic check <archivo.py> [opciones]
  pypic ir <archivo.py> [opciones]
  pypic build <archivo.py> [-o salida.hex] [--listing] [opciones]
  pypic disasm <archivo.hex>
  pypic device validate <directorio-del-pack>

opciones:
  --device <directorio>   usa el Device Pack (p. ej. devices/pic16f887)
  --native-bits N         ancho nativo del destino (sobrescribe al pack)
  --max-int-bits N        ancho maximo de entero (sobrescribe al pack)
  --stack-levels N        niveles de la pila de retorno (sobrescribe al pack)";

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
        Some("disasm") => disasm(&args[1..]),
        Some(c @ ("check" | "ir" | "build")) => compile(c, &args[1..]),
        _ => usage(),
    }
}

/// Desensambla un `.hex` (PIC14): util para inspeccionar lo que se carga en Proteus o en un programador.
fn disasm(args: &[String]) -> ExitCode {
    let [path] = args else { return usage() };
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("no se pudo leer '{path}': {e}");
            return ExitCode::from(2);
        }
    };
    let img = match Image::from_intel_hex(&text) {
        Ok(i) => i,
        Err(e) => {
            eprintln!("{path}: {e}");
            return ExitCode::from(1);
        }
    };
    // rachas de direcciones consecutivas
    let mut runs: Vec<(u32, Vec<u16>)> = Vec::new();
    for (addr, word) in img.iter() {
        match runs.last_mut() {
            Some((start, words)) if *start + words.len() as u32 == addr => words.push(word),
            _ => runs.push((addr, vec![word])),
        }
    }
    println!("; {} palabras en {} bloque(s)", img.len(), runs.len());
    for (start, words) in runs {
        let zone = if start >= 0x2000 { "configuracion (no son instrucciones)" } else { "programa" };
        println!("; --- {zone}, desde {start:#06x} ---");
        if start >= 0x2000 {
            for (i, w) in words.iter().enumerate() {
                println!("{:04X}: {:04X}  CONFIG", start + i as u32, w);
            }
        } else {
            print!("{}", disassemble(&words, start));
        }
    }
    ExitCode::SUCCESS
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
    let mut stack = None;
    let mut out_path: Option<String> = None;
    let mut listing = false;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "-o" => out_path = it.next().cloned(),
            "--listing" => listing = true,
            "--device" => device_dir = it.next().cloned(),
            "--native-bits" => native = it.next().and_then(|v| v.parse::<u32>().ok()),
            "--max-int-bits" => max = it.next().and_then(|v| v.parse::<u32>().ok()),
            "--stack-levels" => stack = it.next().and_then(|v| v.parse::<u32>().ok()),
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
    if let Some(s) = stack {
        target.stack_levels = Some(s);
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
    if cmd == "ir" {
        print!("{}", pypic_ir::print(&module));
        return ExitCode::SUCCESS;
    }
    // build: IR -> instrucciones PIC14 -> Intel HEX
    let compiled = match pypic_pic14::compile(&module, &pypic_pic14::Options::default()) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("{file}: error: {e}");
            return ExitCode::from(1);
        }
    };
    if let Err(e) = compiled.image.check_bounds(8192, (0x2000, 0x3FFF)) {
        eprintln!("{file}: error interno: {e}");
        return ExitCode::from(70);
    }
    let out_path = out_path.unwrap_or_else(|| Path::new(&file).with_extension("hex").to_string_lossy().into_owned());
    if let Err(e) = std::fs::write(&out_path, compiled.image.to_intel_hex(16)) {
        eprintln!("no se pudo escribir '{out_path}': {e}");
        return ExitCode::from(2);
    }
    if listing {
        print!("{}", compiled.listing);
    }
    println!(
        "{out_path}: {} palabras de programa, {} bytes de RAM (banco 0). Palabras de configuracion por defecto (INTOSCIO, WDT apagado, MCLR interno).",
        compiled.flash_words, compiled.ram_bytes
    );
    ExitCode::SUCCESS
}
