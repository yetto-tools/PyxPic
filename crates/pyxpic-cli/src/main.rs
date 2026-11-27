use std::path::{Path, PathBuf};
use std::process::ExitCode;

use pyxpic_device::DevicePack;
use pyxpic_frontend::TargetProfile;
use pyxpic_pic14::{disassemble, Image};

mod clean;
mod memory;
mod monitor;
mod project;
mod report;
mod workflow;

const USAGE: &str = "uso:
  pyxpic check [archivo.py] [opciones]
  pyxpic ir [archivo.py] [opciones]
  pyxpic build [archivo.py] [-o salida.hex] [--listing] [--timing] [opciones]
  pyxpic memory [archivo] [--details] [--format text|json] [opciones]   memoria usada por el programa (programa, RAM, pila), sin grabar nada
  pyxpic vars <archivo.bas | -> [--format text|json]     variables de un programa BASIC: predefinidas, DIM y SYMBOL, con tipo, linea y uso (- lee la entrada estandar)
  pyxpic monitor --list | --port COM3 [--baud 9600] | --sim programa     terminal serial (ver pyxpic monitor --help)
  pyxpic clean [archivo.py] [-o salida.hex] [--project pyxpic.toml] [--target perfil] [--dry-run]   borra el .hex que build escribe (solo ese archivo)
  pyxpic asm <archivo.s> [-mcpu=16F887] [-Wl,-pNOMBRE=dir,...] [-o salida.hex] [--listing]
  pyxpic disasm <archivo.hex> [--pic-as] [-mcpu=16F887] [--devices-dir <dir>]
  pyxpic device validate <directorio-del-pack>
  pyxpic init [carpeta] [--name N] [--device PIC16F887] [--clock DECLARACION]   crea pyxpic.toml y un main.py de partida
  pyxpic project check [--project pyxpic.toml] [--target perfil] [--format text|json] [--devices-dir dir]   revisa el proyecto sin modificar nada
  pyxpic project set [--target perfil] [--create] [--dry-run] [--unset CLAVE] [CLAVE=VALOR]...   cambia un perfil (valida antes de escribir)

opciones:
  --project <pyxpic.toml>  archivo de proyecto (por defecto se busca el mas cercano al programa; sin archivo, usa su 'main')
  --target <perfil>       perfil de destino del proyecto ([target.perfil]); por defecto default_target o el unico
  --lang py|bas           el lenguaje cuando el programa llega por la entrada estandar (check -)
  --format text|json      como se imprimen los diagnosticos: texto (por defecto) o JSON versionado, solo con check (docs/diagnosticos-json.md)
  --no-opt                no optimiza la IR (para depurar o comparar tamanos; ver docs/ir-optimizaciones.md)
  --device <directorio>   usa este Device Pack (por defecto se busca el que declara el programa)
  --devices-dir <dir>     donde buscar los Device Packs (tambien $PYXPIC_DEVICES y ./devices)
  --config AJUSTE=VALOR   palabra de configuracion por nombre, p. ej. --config WDTE=ON,FOSC=XT (repetible)
  --native-bits N         ancho nativo del destino (sobrescribe al pack)
  --max-int-bits N        ancho maximo de entero (sobrescribe al pack)
  --stack-levels N        niveles de la pila de retorno (sobrescribe al pack)";

fn usage() -> ExitCode {
    eprintln!("{USAGE}");
    ExitCode::from(2)
}

fn load_pack(dir: &str) -> Result<DevicePack, ExitCode> {
    workflow::load_pack_text(dir).map_err(|(m, c)| {
        eprintln!("{m}");
        ExitCode::from(c)
    })
}

/// Busca el Device Pack de `name` (`PIC16F887` -> `<raiz>/pic16f887`) en `--devices-dir`, `$PYXPIC_DEVICES` y `./devices`.
fn find_pack(name: &str, extra: Option<&str>) -> Result<DevicePack, ExitCode> {
    workflow::locate_pack(name, extra).map_err(|(m, c)| {
        eprintln!("{m}");
        ExitCode::from(c)
    })
}

/// `ANn` -> (puerto, bit) de los pines del encapsulado del pack.
fn analog_pins(pack: &DevicePack) -> std::collections::BTreeMap<u8, (char, u8)> {
    let mut out = std::collections::BTreeMap::new();
    if let Some(pk) = pack.pins.packages.first() {
        for p in &pk.pins {
            if let (Some(port), Some(bit)) = (&p.port, p.bit) {
                for f in &p.functions {
                    if let Some(n) = f.strip_prefix("AN").and_then(|n| n.parse::<u8>().ok()) {
                        out.insert(n, (port.chars().next().unwrap_or('?'), bit as u8));
                    }
                }
            }
        }
    }
    out
}

/// Parametros del backend tomados del Device Pack: RAM util del banco 0, tamano de programa y direcciones de CONFIG.
fn backend_options(pack: &DevicePack, settings: &[(String, String)]) -> Result<pyxpic_pic14::Options, String> {
    let d = &pack.memory.data;
    let first = d.gpr.first().ok_or("el pack no define RAM de propósito general")?;
    if pack.config.words.is_empty() || d.common.start.0 == 0 {
        return Err("el pack no define palabras de configuracion o la zona comun".into());
    }
    // el valor por defecto del pack y encima, uno a uno, los ajustes pedidos por nombre
    let mut config = pack.config.build_words();
    let mut seen = std::collections::BTreeSet::new();
    for (name, value) in settings {
        if !seen.insert(name.clone()) {
            return Err(format!("--config {name} se indica dos veces"));
        }
        pack.config.apply(&mut config, name, value)?;
    }
    Ok(pyxpic_pic14::Options {
        device: pack.device.name.clone(),
        program_words: pack.memory.program.words,
        clocks_per_cycle: pack.device.cpu.clocks_per_instruction,
        ram_start: first.start.0 as u16,
        ram_end: (d.common.start.0 - 1) as u16,
        config,
        registers: pack.registers.clone(),
        analog_pins: analog_pins(pack),
    })
}

fn fmt_ns(ns: f64) -> String {
    let a = ns.abs();
    if a >= 1e9 {
        format!("{:.6} s", ns / 1e9)
    } else if a >= 1e6 {
        format!("{:.6} ms", ns / 1e6)
    } else if a >= 1e3 {
        format!("{:.3} us", ns / 1e3)
    } else {
        format!("{ns:.1} ns")
    }
}

/// Informe de esperas: lo pedido, los ciclos emitidos y el tiempo real que dan (la precision, a la vista).
fn print_timing(timing: &[pyxpic_pic14::DelayReport]) {
    if timing.is_empty() {
        println!("esperas: ninguna");
        return;
    }
    println!("esperas ({}):", timing.len());
    for r in timing {
        let unit = match r.unit {
            pyxpic_ir::ir::DelayUnit::Cycles => "ciclos",
            pyxpic_ir::ir::DelayUnit::Us => "us",
            pyxpic_ir::ir::DelayUnit::Ms => "ms",
            pyxpic_ir::ir::DelayUnit::S => "s",
        };
        match r.requested {
            Some(n) => {
                let err = r.achieved_ns() - r.requested_ns().unwrap_or(0.0);
                println!("  {}: {n} {unit} -> {} ciclos = {} (error {:+.1} ns)", r.function, r.cycles, fmt_ns(r.achieved_ns()), err);
            }
            None => println!("  {}: cantidad variable de {unit} -> {} ciclos por unidad ({}) + {} ciclos fijos de entrada", r.function, r.cycles, fmt_ns(r.achieved_ns()), r.fixed_cycles),
        }
    }
}

/// `pyxpic vars <archivo.bas | -> [--format text|json]`: las variables de un programa PyxPic BASIC (predefinidas, `DIM` y `SYMBOL`) con su tipo, la
/// linea donde se declaran y si el programa las usa. `-` lee el programa de la entrada estandar (la extension de VS Code lo usa con el texto sin guardar).
fn vars_cmd(args: &[String]) -> ExitCode {
    let (mut file, mut json) = (None::<&String>, false);
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--format" => match it.next().map(String::as_str) {
                Some("json") => json = true,
                Some("text") => json = false,
                _ => {
                    eprintln!("--format admite text o json");
                    return ExitCode::from(2);
                }
            },
            _ if file.is_none() => file = Some(a),
            _ => return usage(),
        }
    }
    let Some(file) = file else { return usage() };
    let source = if file == "-" {
        let mut text = String::new();
        if std::io::Read::read_to_string(&mut std::io::stdin(), &mut text).is_err() {
            eprintln!("no se pudo leer la entrada estandar");
            return ExitCode::from(1);
        }
        text
    } else if pyxpic_frontend::Language::from_path(file) != pyxpic_frontend::Language::Basic {
        eprintln!("{file}: vars analiza programas PyxPic BASIC (.bas)");
        return ExitCode::from(2);
    } else {
        match std::fs::read_to_string(file) {
            Ok(t) => t,
            Err(e) => {
                eprintln!("{file}: {e}");
                return ExitCode::from(1);
            }
        }
    };
    let vars = pyxpic_frontend::basic::variables(&source);
    if json {
        let items: Vec<serde_json::Value> = vars
            .iter()
            .map(|v| serde_json::json!({ "name": v.name, "kind": v.kind, "type": v.ty, "length": v.len, "line": v.line, "used": v.used, "detail": v.detail }))
            .collect();
        let report = serde_json::json!({ "version": 1, "language": "basic", "device": pyxpic_frontend::basic::declared_device(&source), "variables": items });
        println!("{}", serde_json::to_string_pretty(&report).expect("el informe es JSON valido"));
    } else {
        println!("{:<12} {:<11} {:<8} {:>5}  {:<6} {}", "NOMBRE", "ORIGEN", "TIPO", "LINEA", "USADA", "DETALLE");
        for v in &vars {
            let name = match v.len {
                Some(n) => format!("{}({n})", v.name),
                None => v.name.clone(),
            };
            let line = v.line.map(|l| l.to_string()).unwrap_or_else(|| "-".into());
            println!("{:<12} {:<11} {:<8} {:>5}  {:<6} {}", name, v.kind, v.ty, line, if v.used { "si" } else { "no" }, v.detail);
        }
    }
    ExitCode::SUCCESS
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("device") => device(&args[1..]),
        Some("init") => workflow::init(&args[1..]),
        Some("project") => workflow::project_cmd(&args[1..]),
        Some("clean") => clean::clean(&args[1..]),
        Some("monitor") => monitor::monitor(&args[1..]),
        Some("disasm") => disasm(&args[1..]),
        Some("vars") => vars_cmd(&args[1..]),
        Some("asm") => asm_cmd(&args[1..]),
        Some(c @ ("check" | "ir" | "build" | "memory")) => compile(c, &args[1..]),
        _ => usage(),
    }
}

/// `16F887` o `PIC16F887` -> `PIC16F887`.
fn pic_name(s: &str) -> String {
    let u = s.to_ascii_uppercase();
    if u.starts_with("PIC") {
        u
    } else {
        format!("PIC{u}")
    }
}

/// Ensambla un `.s` PIC14 con la sintaxis de pic-as a Intel HEX (ver docs/asm-pic14.md).
fn asm_cmd(args: &[String]) -> ExitCode {
    let mut file = None;
    let mut out_path: Option<String> = None;
    let mut listing = false;
    let mut cpu: Option<String> = None;
    let mut devices_dir: Option<String> = None;
    let mut link_opts: Vec<String> = Vec::new();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "-o" => out_path = it.next().cloned(),
            "--listing" => listing = true,
            "--devices-dir" => devices_dir = it.next().cloned(),
            "--device" => cpu = it.next().cloned(),
            s if s.starts_with("-mcpu=") => cpu = Some(s["-mcpu=".len()..].to_string()),
            // -Wl,-presetVec=0h,-pintentry=4h: las opciones van separadas por comas (como en pic-as)
            s if s.starts_with("-Wl,") => link_opts.extend(s["-Wl,".len()..].split(',').map(str::to_string)),
            s if s.starts_with("--fixupoverflow=") => link_opts.push(s.to_string()),
            s if s.starts_with('-') => return usage(),
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
    // el dispositivo: -mcpu/--device, o la directiva PROCESSOR del programa
    let name = cpu.or_else(|| pyxpic_pic14::declared_processor(&src)).map(|n| pic_name(&n));
    let mut env = match &name {
        Some(n) => match find_pack(n, devices_dir.as_deref()) {
            Ok(pack) => pyxpic_pic14::AsmEnv::from_pack(&pack),
            Err(code) => return code,
        },
        None => pyxpic_pic14::AsmEnv::default(),
    };
    // las opciones -p con lista (-pa=0h,b,c) llegan partidas por la coma: se vuelven a unir
    let mut joined: Vec<String> = Vec::new();
    for o in link_opts {
        match joined.last_mut() {
            Some(last) if !o.starts_with('-') && last.starts_with("-p") => {
                last.push(',');
                last.push_str(&o);
            }
            _ => joined.push(o),
        }
    }
    if let Err(e) = env.add_link_options(&joined) {
        eprintln!("{e}");
        return ExitCode::from(2);
    }
    let asm = match pyxpic_pic14::assemble_with(&src, &env) {
        Ok(a) => a,
        Err(errors) => {
            eprint!("{}", pyxpic_pic14::render_errors(&errors, &file));
            return ExitCode::from(1);
        }
    };
    for warning in &asm.warnings {
        eprintln!("{file}:{}:{}: warning[{}]: {}", warning.line, warning.col, warning.code, warning.message);
    }
    let out_path = out_path.unwrap_or_else(|| Path::new(&file).with_extension("hex").to_string_lossy().into_owned());
    if let Err(e) = std::fs::write(&out_path, asm.image.to_intel_hex(16)) {
        eprintln!("no se pudo escribir '{out_path}': {e}");
        return ExitCode::from(2);
    }
    if listing {
        print!("{}", asm.listing);
    }
    println!("{out_path}: {} palabras", asm.image.len());
    ExitCode::SUCCESS
}

/// Desensambla un `.hex` (PIC14): util para inspeccionar lo que se carga en Proteus o en un programador.
fn disasm(args: &[String]) -> ExitCode {
    let mut path = None;
    let mut pic_as = false;
    let mut cpu = None;
    let mut devices_dir = None;
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--pic-as" => pic_as = true,
            "--devices-dir" => match it.next() {
                Some(dir) => devices_dir = Some(dir.as_str()),
                None => return usage(),
            },
            s if s.starts_with("-mcpu=") && s.len() > 6 => cpu = Some(&s[6..]),
            s if !s.starts_with('-') && path.is_none() => path = Some(s),
            _ => return usage(),
        }
    }
    let Some(path) = path else { return usage() };
    if !pic_as && (cpu.is_some() || devices_dir.is_some()) {
        return usage();
    }
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
    if pic_as {
        let pack = match cpu {
            Some(name) => match find_pack(&pic_name(name), devices_dir) {
                Ok(pack) if pack.memory.program.word_bits == 14 => Some(pack),
                Ok(_) => {
                    eprintln!("disasm --pic-as solo admite palabras PIC14 de 14 bits");
                    return ExitCode::from(1);
                }
                Err(code) => return code,
            },
            None => None,
        };
        println!("; PIC14: direcciones en palabras; el HEX no conserva nombres ni distingue código de datos");
        if let Some(pack) = &pack {
            println!("PROCESSOR {}", pack.device.name.strip_prefix("PIC").unwrap_or(&pack.device.name));
            for config in &pack.config.words {
                if let Some(value) = img.get(config.address.0) {
                    println!("CONFIG {} = {value:#06x}", config.name);
                }
            }
            for id in &pack.config.id_locations {
                if let Some(value) = img.get(id.address.0) {
                    if u32::from(value) & !id.mask.0 == 0 {
                        println!("CONFIG {} = {value:#06x}", id.name);
                    }
                }
            }
        }
        println!("PSECT pyxpic_disasm,class=CODE,delta=2,abs");
        for (addr, word) in img.iter() {
            if pack.as_ref().is_some_and(|p| p.config.words.iter().any(|c| c.address.0 == addr)) {
                continue;
            }
            if pack.as_ref().is_some_and(|p| p.config.id_locations.iter().any(|id| id.address.0 == addr && u32::from(word) & !id.mask.0 == 0)) {
                continue;
            }
            println!("    ORG {addr:#06x}");
            if let Some(pack) = &pack {
                if addr == pack.memory.program.reset_vector.0 {
                    println!("pyxpic_reset:");
                }
                if addr == pack.memory.program.interrupt_vector.0 {
                    println!("pyxpic_interrupt:");
                }
            }
            let program_end = pack.as_ref().map_or(0x2000, |p| p.memory.program.words);
            if addr >= program_end {
                println!("    DW {word:#06x} ; zona especial: no se interpreta como instrucción");
            } else {
                println!("    {}", pyxpic_pic14::isa::pic_as_word(word));
            }
        }
        println!("END");
        return ExitCode::SUCCESS;
    }
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
    let mut details = false;
    let mut timing = false;
    let mut optimize = true;
    let mut json = false;
    let mut settings: Vec<(String, String)> = Vec::new();
    let mut project_arg: Option<String> = None;
    let mut target_arg: Option<String> = None;
    let mut lang_arg: Option<String> = None;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "-o" => out_path = it.next().cloned(),
            "--project" => project_arg = it.next().cloned(),
            "--lang" => lang_arg = it.next().cloned(),
            "--target" => target_arg = it.next().cloned(),
            "--listing" => listing = true,
            "--details" => details = true,
            "--timing" => timing = true,
            "--no-opt" => optimize = false,
            "--format" => match it.next().map(String::as_str) {
                Some("json") => json = true,
                Some("text") => json = false,
                _ => {
                    eprintln!("--format es 'text' o 'json'");
                    return ExitCode::from(2);
                }
            },
            "--device" => device_dir = it.next().cloned(),
            "--devices-dir" => devices_dir = it.next().cloned(),
            "--native-bits" => native = it.next().and_then(|v| v.parse::<u32>().ok()),
            "--max-int-bits" => max = it.next().and_then(|v| v.parse::<u32>().ok()),
            "--stack-levels" => stack = it.next().and_then(|v| v.parse::<u32>().ok()),
            // --config WDTE=ON,FOSC=XT (se puede repetir)
            "--config" => {
                let Some(v) = it.next() else { return usage() };
                for part in v.split(',') {
                    match part.split_once('=') {
                        Some((n, val)) => settings.push((n.trim().to_string(), val.trim().to_string())),
                        None => {
                            eprintln!("--config {part}: se escribe AJUSTE=VALOR (p. ej. WDTE=ON)");
                            return ExitCode::from(2);
                        }
                    }
                }
            }
            s if s.starts_with("--") => return usage(),
            _ if file.is_none() => file = Some(a.clone()),
            _ => return usage(),
        }
    }
    // pyxpic.toml: el proyecto (el mas cercano al programa o a la carpeta actual) y su perfil de destino (docs/proyecto.md)
    let loaded = match &project_arg {
        Some(f) => Some(PathBuf::from(f)),
        None => project::find(&file.as_deref().map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."))),
    };
    let project = match loaded.map(|f| project::load(&f)) {
        Some(Ok(p)) => Some(p),
        Some(Err(e)) => {
            eprintln!("{e}");
            return ExitCode::from(2);
        }
        None => None,
    };
    if target_arg.is_some() && project.is_none() {
        eprintln!("--target necesita un pyxpic.toml (se busca desde la carpeta del programa hacia arriba; o indica --project <archivo>)");
        return ExitCode::from(2);
    }
    let profile = match project.as_ref().map(|p| p.target(target_arg.as_deref())) {
        Some(Ok(t)) => t,
        Some(Err(e)) => {
            eprintln!("{e}");
            return ExitCode::from(2);
        }
        None => None,
    };
    let Some(file) = file.or_else(|| project.as_ref().and_then(|p| p.main.as_ref()).map(|m| m.to_string_lossy().into_owned())) else { return usage() };
    if devices_dir.is_none() {
        devices_dir = project.as_ref().and_then(|p| p.devices_dir.as_ref()).map(|d| d.to_string_lossy().into_owned());
    }
    if let Some((_, t)) = &profile {
        // la linea de comandos manda sobre el perfil
        let asked: std::collections::BTreeSet<String> = settings.iter().map(|(n, _)| n.clone()).collect();
        let mut merged: Vec<(String, String)> = t.config.iter().filter(|(n, _)| !asked.contains(n)).cloned().collect();
        merged.extend(std::mem::take(&mut settings));
        settings = merged;
        if out_path.is_none() {
            out_path = t.output.as_ref().map(|o| o.to_string_lossy().into_owned());
        }
    }
    // `-` lee el programa de la entrada estandar (la extension de VS Code revisa lo que se escribe, sin guardar)
    let src = if file == "-" {
        let mut text = String::new();
        if std::io::Read::read_to_string(&mut std::io::stdin(), &mut text).is_err() {
            eprintln!("no se pudo leer la entrada estandar");
            return ExitCode::from(2);
        }
        text
    } else {
        match std::fs::read_to_string(&file) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("no se pudo leer '{file}': {e}");
                return ExitCode::from(2);
            }
        }
    };

    // El dispositivo lo declara el propio programa (`from pyxpic.device import PIC16F887`);
    // `--device` solo fuerza un pack concreto y el analisis comprueba que coincida con la declaracion.
    let lang = match lang_arg.as_deref() {
        Some("bas" | "basic") => pyxpic_frontend::Language::Basic,
        Some("py" | "python") => pyxpic_frontend::Language::Python,
        Some(other) => {
            eprintln!("--lang es 'py' o 'bas', no '{other}'");
            return ExitCode::from(2);
        }
        None => pyxpic_frontend::Language::from_path(&file),
    };
    let declared = pyxpic_frontend::declared_device_in(lang, &src);
    // el fuente manda: si declara un dispositivo y el perfil pide otro, es un error; si no declara ninguno, vale el del perfil
    let profile_device = profile.as_ref().and_then(|(_, t)| t.device.clone());
    if let (Some(d), Some(pd), Some((tn, _)), Some(pj)) = (&declared, &profile_device, &profile, &project) {
        if !d.eq_ignore_ascii_case(pd) {
            eprintln!(
                "{file}: error: el programa declara 'from pyxpic.device import {d}' y el perfil '{tn}' de {} pide {pd}: quita una de las dos declaraciones o elige otro perfil con --target",
                pj.file.display()
            );
            return ExitCode::from(1);
        }
    }
    let device_name = declared.clone().or(profile_device);
    let pack = match (&device_dir, &device_name) {
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
    // el reloj del perfil: se valida con las mismas reglas que en el fuente y solo vale si el programa no declara el suyo
    let mut profile_clock = None;
    if let (Some((tn, t)), Some(pj)) = (&profile, &project) {
        if let Some(text) = &t.clock {
            match pyxpic_frontend::parse_clock(text, &target) {
                Ok(c) => profile_clock = Some(c),
                Err(e) => {
                    eprint!("{e}");
                    eprintln!("{}: error: [target.{tn}] clock = \"{text}\" no es valido", pj.file.display());
                    return ExitCode::from(1);
                }
            }
        }
    }
    let source_clock = pyxpic_frontend::declares_clock_in(lang, &src);
    if let (Some(c), false) = (&profile_clock, source_clock) {
        target.project_clock = Some(c.clone());
    }

    let mut res = pyxpic_frontend::analyze_in(lang, &src, &target);
    // sin dispositivo el analisis es generico: no se sabe que pines, perifericos, registros ni limites tiene el PIC, y el mismo codigo puede
    // compilar para otro PIC que no tenga lo que el programa espera. Que falte tiene que verse en la salida, no pasar como un `ok` mudo.
    if device_name.is_none() {
        res.diagnostics.warning(
            "PYP3017",
            pyxpic_frontend::Span::new(0, 0),
            "no se declaro el dispositivo: el analisis es generico y NO comprueba que el PIC tenga los pines, perifericos, registros y limites que usa el programa; declaralo en la primera linea ('from pyxpic.device import PIC16F887') o en un perfil de pyxpic.toml",
        );
    } else if declared.is_none() {
        // el perfil lo da, pero el codigo no dice para que PIC esta escrito: el mismo fuente puede compilar para otro PIC que no tenga lo que espera
        res.diagnostics.warning(
            "PYP3018",
            pyxpic_frontend::Span::new(0, 0),
            format!("el programa no declara su dispositivo: se usa {} del perfil de pyxpic.toml; ponlo en la primera linea ('from pyxpic.device import {}') para que el codigo diga que PIC espera y se compruebe contra el perfil", device_name.as_deref().unwrap_or("?"), device_name.as_deref().unwrap_or("PIC16F887")),
        );
    }
    if json && cmd != "memory" {
        if cmd != "check" {
            eprintln!("--format json solo vale con check y memory: build e ir imprimen texto");
            return ExitCode::from(2);
        }
        // un solo documento JSON en la salida estandar, sin ninguna otra linea
        println!("{}", serde_json::to_string_pretty(&report::diagnostics_json(&file, &src, &res.diagnostics)).expect("el informe es JSON valido"));
        return if res.diagnostics.has_errors() { ExitCode::from(1) } else { ExitCode::SUCCESS };
    }
    // con `memory --format json` la salida estandar es un solo documento JSON: los avisos van a la salida de errores
    if cmd == "memory" && json {
        eprint!("{}", res.diagnostics.render(&src, &file));
    } else {
        print!("{}", res.diagnostics.render(&src, &file));
    }
    let Some(program) = res.program else { return ExitCode::from(1) };
    if let (Some(c), true, Some((tn, _))) = (&profile_clock, source_clock, &profile) {
        if program.clock_source.as_ref() != Some(c) {
            eprintln!("{file}: aviso: el perfil '{tn}' pide el reloj {} pero el programa declara el suyo: manda el programa", c.describe());
        }
    }
    if cmd == "check" {
        println!("{file}: ok");
        return ExitCode::SUCCESS;
    }
    if (cmd == "build" || cmd == "memory") && (device_name.is_none() || pack.is_none()) {
        eprintln!("{file}: error: falta el dispositivo: declaralo en la primera linea del programa ('from pyxpic.device import PIC16F887') o en el perfil de un pyxpic.toml ([target.NOMBRE] device = \"PIC16F887\")");
        return ExitCode::from(1);
    }
    let module = if optimize { pyxpic_ir::lower(&program) } else { pyxpic_ir::lower_unoptimized(&program) };
    if let Err(errs) = pyxpic_ir::verify(&module) {
        eprintln!("error interno: la IR generada no es valida:\n  {}", errs.join("\n  "));
        return ExitCode::from(70);
    }
    if cmd == "ir" {
        print!("{}", pyxpic_ir::print(&module));
        return ExitCode::SUCCESS;
    }
    // build: IR -> instrucciones PIC14 -> Intel HEX
    let pack = pack.expect("comprobado arriba");
    // `CLOCK: Crystal` fija el oscilador (FOSC): pedir otro con --config seria una contradiccion
    if let Some(cs) = &module.clock_source {
        if settings.iter().any(|(n, _)| n == "FOSC") {
            eprintln!(
                "{file}: error: el programa declara 'CLOCK: {}', que fija FOSC = {}: quita --config FOSC=... (o declara el reloj con CLOCK_HZ para elegir el oscilador a mano)",
                cs.kind.name(),
                cs.fosc
            );
            return ExitCode::from(1);
        }
        settings.push(("FOSC".to_string(), cs.fosc.to_string()));
        for (n, v) in &cs.extra_config {
            if settings.iter().any(|(s, _)| s == n) {
                eprintln!("{file}: error: el programa declara 'CLOCK: {}' con ajustes que fijan {n} = {v}: quita --config {n}=...", cs.kind.name());
                return ExitCode::from(1);
            }
            settings.push((n.clone(), v.clone()));
        }
    }
    let opts = match backend_options(&pack, &settings) {
        Ok(o) => o,
        Err(e) => {
            eprintln!("{file}: error: {e}");
            return ExitCode::from(1);
        }
    };
    let compiled = match pyxpic_pic14::compile(&module, &opts) {
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
    if cmd == "memory" {
        return memory::run(&memory::Inputs { file: &file, compiled: &compiled, program: &program, pack: &pack, opts: &opts, details }, json);
    }
    let out_path = out_path.unwrap_or_else(|| Path::new(&file).with_extension("hex").to_string_lossy().into_owned());
    // la carpeta de salida de un perfil (output = "build/x.hex") se crea si falta
    if let Some(dir) = Path::new(&out_path).parent().filter(|d| !d.as_os_str().is_empty()) {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Err(e) = std::fs::write(&out_path, compiled.image.to_intel_hex(16)) {
        eprintln!("no se pudo escribir '{out_path}': {e}");
        return ExitCode::from(2);
    }
    if listing {
        print!("{}", compiled.listing);
    }
    if timing {
        print_timing(&compiled.timing);
        if let Some(u) = &compiled.uart {
            let modo = match (u.brg16, u.brgh) {
                (false, false) => "8 bits, BRGH = 0",
                (false, true) => "8 bits, BRGH = 1",
                (true, false) => "16 bits, BRGH = 0",
                (true, true) => "16 bits, BRGH = 1",
            };
            println!("uart: {} baudios pedidos -> {:.1} ({:+.2} %), generador de {modo}, SPBRG = {} a {} Hz", u.baud, u.achieved, u.error_pct, u.n, u.fosc_hz);
        }
    }
    let cfg_text = {
        let names: Vec<String> = pack.config.words.iter().zip(&opts.config).map(|(w, (_, v))| format!("{}={v:#06X}", w.name)).collect();
        let asked = settings.len() - module.clock_source.is_some() as usize;
        let how = match (&module.clock_source, asked) {
            (None, 0) => "por defecto del pack: INTOSCIO, WDT apagado, MCLR interno".to_string(),
            (None, n) => format!("{n} ajuste(s) pedidos"),
            (Some(cs), 0) => format!("FOSC = {} por Clock", cs.fosc),
            (Some(cs), n) => format!("FOSC = {} por Clock y {n} ajuste(s) pedidos", cs.fosc),
        };
        format!("{} ({how})", names.join(", "))
    };
    println!("Compilacion completada:");
    println!("  Archivo:       {out_path}");
    println!("  Dispositivo:   {}", pack.device.name);
    if let (Some((tn, _)), Some(pj)) = (&profile, &project) {
        println!("  Perfil:        {tn} ({})", pj.file.display());
    }
    println!("  Programa:      {} palabras", compiled.flash_words);
    println!("  RAM:           {} bytes (banco 0)", compiled.ram_bytes);
    if let Some(hz) = module.clock_hz {
        let source = if module.clock_defaulted { "por defecto" } else { "declarado" };
        println!("  Reloj:         {hz} Hz ({source})");
    }
    if let Some(cs) = &module.clock_source {
        println!("  Oscilador:     {}", cs.describe());
    }
    println!("  Configuracion: {cfg_text}");
    ExitCode::SUCCESS
}
