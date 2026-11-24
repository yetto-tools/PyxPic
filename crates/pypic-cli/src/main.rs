use std::path::Path;
use std::process::ExitCode;

use pypic_device::DevicePack;
use pypic_frontend::{analyze, TargetProfile};
use pypic_pic14::{disassemble, Image};

const USAGE: &str = "uso:
  pypic check <archivo.py> [opciones]
  pypic ir <archivo.py> [opciones]
  pypic build <archivo.py> [-o salida.hex] [--listing] [--timing] [opciones]
  pypic asm <archivo.s> [-mcpu=16F887] [-Wl,-pNOMBRE=dir,...] [-o salida.hex] [--listing]
  pypic disasm <archivo.hex> [--pic-as] [-mcpu=16F887] [--devices-dir <dir>]
  pypic device validate <directorio-del-pack>

opciones:
  --device <directorio>   usa este Device Pack (por defecto se busca el que declara el programa)
  --devices-dir <dir>     donde buscar los Device Packs (tambien $PYPIC_DEVICES y ./devices)
  --config AJUSTE=VALOR   palabra de configuracion por nombre, p. ej. --config WDTE=ON,FOSC=XT (repetible)
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
fn backend_options(pack: &DevicePack, settings: &[(String, String)]) -> Result<pypic_pic14::Options, String> {
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
    Ok(pypic_pic14::Options {
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
fn print_timing(timing: &[pypic_pic14::DelayReport]) {
    if timing.is_empty() {
        println!("esperas: ninguna");
        return;
    }
    println!("esperas ({}):", timing.len());
    for r in timing {
        let unit = match r.unit {
            pypic_ir::ir::DelayUnit::Cycles => "ciclos",
            pypic_ir::ir::DelayUnit::Us => "us",
            pypic_ir::ir::DelayUnit::Ms => "ms",
            pypic_ir::ir::DelayUnit::S => "s",
        };
        match r.requested {
            Some(n) => {
                let err = r.achieved_ns() - r.requested_ns().unwrap_or(0.0);
                println!("  {}: {n} {unit} -> {} ciclos = {} (error {:+.1} ns)", r.function, r.cycles, fmt_ns(r.achieved_ns()), err);
            }
            None => println!(
                "  {}: cantidad variable de {unit} -> {} ciclos por unidad ({}) + {} ciclos fijos de entrada",
                r.function,
                r.cycles,
                fmt_ns(r.achieved_ns()),
                r.fixed_cycles
            ),
        }
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("device") => device(&args[1..]),
        Some("disasm") => disasm(&args[1..]),
        Some("asm") => asm_cmd(&args[1..]),
        Some(c @ ("check" | "ir" | "build")) => compile(c, &args[1..]),
        _ => usage(),
    }
}

/// `16F887` o `PIC16F887` -> `PIC16F887`.
fn pic_name(s: &str) -> String {
    let u = s.to_ascii_uppercase();
    if u.starts_with("PIC") { u } else { format!("PIC{u}") }
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
    let name = cpu.or_else(|| pypic_pic14::declared_processor(&src)).map(|n| pic_name(&n));
    let mut env = match &name {
        Some(n) => match find_pack(n, devices_dir.as_deref()) {
            Ok(pack) => pypic_pic14::AsmEnv::from_pack(&pack),
            Err(code) => return code,
        },
        None => pypic_pic14::AsmEnv::default(),
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
    let asm = match pypic_pic14::assemble_with(&src, &env) {
        Ok(a) => a,
        Err(errors) => {
            eprint!("{}", pypic_pic14::render_errors(&errors, &file));
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
            "--devices-dir" => match it.next() { Some(dir) => devices_dir = Some(dir.as_str()), None => return usage() },
            s if s.starts_with("-mcpu=") && s.len() > 6 => cpu = Some(&s[6..]),
            s if !s.starts_with('-') && path.is_none() => path = Some(s),
            _ => return usage(),
        }
    }
    let Some(path) = path else { return usage() };
    if !pic_as && (cpu.is_some() || devices_dir.is_some()) { return usage(); }
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
                Ok(_) => { eprintln!("disasm --pic-as solo admite palabras PIC14 de 14 bits"); return ExitCode::from(1); }
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
        println!("PSECT pypic_disasm,class=CODE,delta=2,abs");
        for (addr, word) in img.iter() {
            if pack.as_ref().is_some_and(|p| p.config.words.iter().any(|c| c.address.0 == addr)) { continue; }
            if pack.as_ref().is_some_and(|p| p.config.id_locations.iter().any(|id| id.address.0 == addr && u32::from(word) & !id.mask.0 == 0)) { continue; }
            println!("    ORG {addr:#06x}");
            if let Some(pack) = &pack {
                if addr == pack.memory.program.reset_vector.0 { println!("pypic_reset:"); }
                if addr == pack.memory.program.interrupt_vector.0 { println!("pypic_interrupt:"); }
            }
            let program_end = pack.as_ref().map_or(0x2000, |p| p.memory.program.words);
            if addr >= program_end {
                println!("    DW {word:#06x} ; zona especial: no se interpreta como instrucción");
            } else {
                println!("    {}", pypic_pic14::isa::pic_as_word(word));
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
    let mut timing = false;
    let mut settings: Vec<(String, String)> = Vec::new();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "-o" => out_path = it.next().cloned(),
            "--listing" => listing = true,
            "--timing" => timing = true,
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
    let opts = match backend_options(&pack, &settings) {
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
        let how = if settings.is_empty() { "por defecto del pack: INTOSCIO, WDT apagado, MCLR interno".to_string() } else { format!("{} ajuste(s) pedidos", settings.len()) };
        format!("{} ({how})", names.join(", "))
    };
    let clock = module.clock_hz.map(|hz| if module.clock_defaulted { format!(", reloj {hz} Hz (por defecto)") } else { format!(", reloj declarado {hz} Hz") }).unwrap_or_default();
    println!(
        "{out_path}: para {}: {} palabras de programa, {} bytes de RAM (banco 0){clock}. Configuracion: {}.",
        pack.device.name,
        compiled.flash_words,
        compiled.ram_bytes,
        cfg_text
    );
    ExitCode::SUCCESS
}
