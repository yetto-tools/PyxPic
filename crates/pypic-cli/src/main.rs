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
  --device <directorio>   usa este Device Pack (por defecto se busca el que declara el programa)
  --devices-dir <dir>     donde buscar los Device Packs (tambien $PYPIC_DEVICES y ./devices)
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

/// Busca el Device Pack de `name` (`PIC16F887` -> `<raiz>/pic16f887`) en `--devices-dir`, `$PYPIC_DEVICES` y `./devices`.
fn find_pack(name: &str, extra: Option<&str>) -> Result<DevicePack, ExitCode> {
    let mut roots: Vec<String> = Vec::new();
    roots.extend(extra.map(String::from));
    roots.extend(std::env::var("PYPIC_DEVICES").ok());
    roots.push("devices".into());
    for root in &roots {
        let dir = Path::new(root).join(name.to_lowercase());
        if dir.join("device.json").is_file() {
            return load_pack(&dir.to_string_lossy());
        }
    }
    let mut avail: Vec<String> = Vec::new();
    for root in &roots {
        if let Ok(rd) = std::fs::read_dir(root) {
            for e in rd.flatten() {
                if e.path().join("device.json").is_file() {
                    avail.push(e.file_name().to_string_lossy().to_uppercase());
                }
            }
        }
    }
    avail.sort();
    avail.dedup();
    eprintln!(
        "dispositivo '{name}' desconocido: no hay Device Pack en {} (disponibles: {})",
        roots.join(", "),
        if avail.is_empty() { "ninguno".to_string() } else { avail.join(", ") }
    );
    Err(ExitCode::from(1))
}

/// Parametros del backend tomados del Device Pack: RAM util del banco 0, tamano de programa y direcciones de CONFIG.
fn backend_options(pack: &DevicePack) -> Result<pypic_pic14::Options, String> {
    let d = &pack.memory.data;
    let first = d.gpr.first().ok_or("el pack no define RAM de propósito general")?;
    let cfg = &pack.config.words;
    if cfg.len() < 2 || d.common.start.0 == 0 {
        return Err("el pack no define dos palabras de configuracion o la zona comun".into());
    }
    Ok(pypic_pic14::Options {
        device: pack.device.name.clone(),
        program_words: pack.memory.program.words,
        ram_start: first.start.0 as u16,
        ram_end: (d.common.start.0 - 1) as u16,
        config_addr: (cfg[0].address.0, cfg[1].address.0),
        // TODO: por nombre de campo desde el pack (#33)
        config: (pypic_pic14::samples::BLINK_CONFIG1, pypic_pic14::samples::BLINK_CONFIG2),
    })
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
    let mut devices_dir: Option<String> = None;
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
            "--devices-dir" => devices_dir = it.next().cloned(),
            "--native-bits" => native = it.next().and_then(|v| v.parse::<u32>().ok()),
            "--max-int-bits" => max = it.next().and_then(|v| v.parse::<u32>().ok()),
            "--stack-levels" => stack = it.next().and_then(|v| v.parse::<u32>().ok()),
            s if s.starts_with("--") => return usage(),
            _ if file.is_none() => file = Some(a.clone()),
            _ => return usage(),
        }
    }
    let Some(file) = file else { return usage() };
    let src = match std::fs::read_to_string(&file) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("no se pudo leer '{file}': {e}");
            return ExitCode::from(2);
        }
    };

    // El dispositivo lo declara el propio programa (`from pypic.device import PIC16F887`);
    // `--device` solo fuerza un pack concreto y el analisis comprueba que coincida con la declaracion.
    let declared = pypic_frontend::declared_device(&src);
    let pack = match (&device_dir, &declared) {
        (Some(dir), _) => match load_pack(dir) {
            Ok(p) => Some(p),
            Err(code) => return code,
        },
        (None, Some(name)) => match find_pack(name, devices_dir.as_deref()) {
            Ok(p) => Some(p),
            Err(code) => return code,
        },
        (None, None) => None,
    };
    let mut target = pack.as_ref().map(DevicePack::target_profile).unwrap_or_else(TargetProfile::generic);
    if let Some(n) = native {
        target.native_bits = n;
    }
    if let Some(m) = max {
        target.max_int_bits = m;
    }
    if let Some(s) = stack {
        target.stack_levels = Some(s);
    }

    let res = analyze(&src, &target);
    print!("{}", res.diagnostics.render(&src, &file));
    let Some(program) = res.program else { return ExitCode::from(1) };
    if cmd == "check" {
        println!("{file}: ok");
        return ExitCode::SUCCESS;
    }
    if cmd == "build" && (declared.is_none() || pack.is_none()) {
        eprintln!("{file}: error: falta la declaracion del dispositivo: la primera linea del programa debe ser, por ejemplo, 'from pypic.device import PIC16F887'");
        return ExitCode::from(1);
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
    let pack = pack.expect("comprobado arriba");
    let opts = match backend_options(&pack) {
        Ok(o) => o,
        Err(e) => {
            eprintln!("{file}: error: {e}");
            return ExitCode::from(1);
        }
    };
    let compiled = match pypic_pic14::compile(&module, &opts) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("{file}: error: {e}");
            return ExitCode::from(1);
        }
    };
    let zone = (pack.config.space.start.0, pack.config.space.end.0);
    if let Err(e) = compiled.image.check_bounds(pack.memory.program.words, zone) {
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
    let clock = module.clock_hz.map(|hz| format!(", reloj declarado {hz} Hz")).unwrap_or_default();
    println!(
        "{out_path}: para {}: {} palabras de programa, {} bytes de RAM (banco 0){clock}. Palabras de configuracion por defecto (INTOSCIO, WDT apagado, MCLR interno).",
        pack.device.name, compiled.flash_words, compiled.ram_bytes
    );
    ExitCode::SUCCESS
}
