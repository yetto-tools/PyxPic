//! `pypic init` y `pypic project check` (docs/proyecto.md): crear y revisar un proyecto con `pypic.toml`. Son la base del asistente de la
//! extension: la extension solo presenta lo que estas ordenes dicen (la logica vive aqui y se prueba sin VS Code).
//!
//! Regla: **nunca se sobrescribe nada** y `project check` no modifica ningun archivo.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use pypic_device::DevicePack;
use pypic_frontend::{analyze, TargetProfile};

use crate::project::{self, Project, Target};

pub const REPORT_VERSION: u32 = 1;

// ---- Device Packs ------------------------------------------------------------------------------------------------------------------

/// Carga un Device Pack y lo valida. Error: (mensaje, codigo de salida).
pub fn load_pack_text(dir: &str) -> Result<DevicePack, (String, u8)> {
    let pack = DevicePack::load(Path::new(dir)).map_err(|e| (e.to_string(), 2))?;
    let errs = pack.validate();
    if !errs.is_empty() {
        let list: Vec<String> = errs.iter().map(|e| format!("  - {e}")).collect();
        return Err((format!("{dir}: el Device Pack no es valido:\n{}", list.join("\n")), 1));
    }
    Ok(pack)
}

/// Busca el Device Pack de `name` (`PIC16F887` -> `<raiz>/pic16f887`) en `extra`, `$PYPIC_DEVICES` y `./devices`.
pub fn locate_pack(name: &str, extra: Option<&str>) -> Result<DevicePack, (String, u8)> {
    let mut roots: Vec<String> = Vec::new();
    roots.extend(extra.map(String::from));
    roots.extend(std::env::var("PYPIC_DEVICES").ok());
    roots.push("devices".into());
    for root in &roots {
        let dir = Path::new(root).join(name.to_lowercase());
        if dir.join("device.json").is_file() {
            return load_pack_text(&dir.to_string_lossy());
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
    Err((
        format!("dispositivo '{name}' desconocido: no hay Device Pack en {} (disponibles: {})", roots.join(", "), if avail.is_empty() { "ninguno".to_string() } else { avail.join(", ") }),
        1,
    ))
}

// ---- pypic init -----------------------------------------------------------------------------------------------------------------------

/// `4_000_000` -> `4 * MHz`, `125_000` -> `125 * kHz`.
fn hz_expr(hz: u32) -> String {
    if hz % 1_000_000 == 0 {
        format!("{} * MHz", hz / 1_000_000)
    } else if hz % 1_000 == 0 {
        format!("{} * kHz", hz / 1_000)
    } else {
        format!("{hz} * Hz")
    }
}

fn toml_str(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "/").replace('"', "\\\""))
}

const MAIN_TEMPLATE: &str = "\"\"\"@NAME@: parpadeo de RB0 cada 500 ms. El dispositivo y el reloj estan en pypic.toml (docs/proyecto.md).\"\"\"\nfrom pypic.periph.gpio import RB0\nfrom pypic.system.time import pause\n\n\ndef main() -> None:\n    RB0.output()\n    while True:\n        RB0.toggle()\n        pause(500)\n";

/// `pypic init [carpeta] [--name N] [--device PIC16F887] [--clock "Internal[4 * MHz]"] [--devices-dir D]`
pub fn init(args: &[String]) -> ExitCode {
    let (mut dir, mut name, mut device, mut clock, mut devices_dir) = (None::<String>, None::<String>, None::<String>, None::<String>, None::<String>);
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--name" => name = it.next().cloned(),
            "--device" => device = it.next().cloned(),
            "--clock" => clock = it.next().cloned(),
            "--devices-dir" => devices_dir = it.next().cloned(),
            s if s.starts_with("--") => return crate::usage(),
            _ if dir.is_none() => dir = Some(a.clone()),
            _ => return crate::usage(),
        }
    }
    let dir = PathBuf::from(dir.unwrap_or_else(|| ".".into()));
    let name = name.unwrap_or_else(|| {
        let base = dir.canonicalize().ok().or_else(|| std::env::current_dir().ok().map(|c| c.join(&dir))).and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()));
        base.filter(|b| !b.is_empty() && b != ".").unwrap_or_else(|| "proyecto".into())
    });
    let device = crate::pic_name(&device.unwrap_or_else(|| "PIC16F887".into()));
    let pack = match locate_pack(&device, devices_dir.as_deref()) {
        Ok(p) => p,
        Err((m, c)) => {
            eprintln!("{m}");
            return ExitCode::from(c);
        }
    };
    let target = pack.target_profile();
    let clock = match clock {
        Some(c) => c,
        None => match target.default_clock_hz {
            Some(hz) => format!("Internal[{}]", hz_expr(hz)),
            None => {
                eprintln!("{device} no declara un reloj por defecto: indica uno con --clock \"Crystal[20 * MHz]\"");
                return ExitCode::from(2);
            }
        },
    };
    if let Err(e) = pypic_frontend::parse_clock(&clock, &target) {
        eprint!("{e}");
        eprintln!("--clock \"{clock}\" no es valido para {device}");
        return ExitCode::from(1);
    }
    let main_src = MAIN_TEMPLATE.replace("@NAME@", &name);
    // el programa de partida debe ser valido para este dispositivo y este reloj antes de escribir nada
    let mut probe: TargetProfile = target.clone();
    probe.project_clock = pypic_frontend::parse_clock(&clock, &target).ok();
    let res = analyze(&main_src, &probe);
    if res.program.is_none() {
        eprint!("{}", res.diagnostics.render(&main_src, "main.py"));
        eprintln!("el programa de partida no es valido para {device}: no se crea nada");
        return ExitCode::from(1);
    }
    let (toml_path, main_path) = (dir.join(project::FILE), dir.join("main.py"));
    let existing: Vec<String> = [&toml_path, &main_path].iter().filter(|p| p.exists()).map(|p| p.display().to_string()).collect();
    if !existing.is_empty() {
        eprintln!("ya existe {}: no se sobrescribe nada (borra el archivo o usa otra carpeta)", existing.join(" y "));
        return ExitCode::from(1);
    }
    let mut toml = format!("# Proyecto PyPIC (docs/proyecto.md): un perfil por destino.\n[project]\nname = {}\nmain = \"main.py\"\n", toml_str(&name));
    if let Some(d) = &devices_dir {
        toml += &format!("devices_dir = {}\n", toml_str(d));
    }
    toml += &format!("\n[target.{}]\ndevice = {}\nclock = {}\noutput = {}\n", device.to_lowercase(), toml_str(&device), toml_str(&clock), toml_str(&format!("build/{name}.hex")));
    if let Err(e) = std::fs::create_dir_all(&dir).and_then(|_| std::fs::write(&toml_path, &toml)).and_then(|_| std::fs::write(&main_path, &main_src)) {
        eprintln!("no se pudo crear el proyecto en '{}': {e}", dir.display());
        return ExitCode::from(2);
    }
    println!("Proyecto creado en {}:", dir.display());
    println!("  {}     el proyecto (dispositivo {device}, reloj {clock})", project::FILE);
    println!("  main.py         un programa de partida (parpadeo de RB0)");
    println!("siguiente: cd {} && pypic build   (o: pypic project check)", dir.display());
    ExitCode::SUCCESS
}

// ---- pypic project check ----------------------------------------------------------------------------------------------------------

/// El resultado de revisar un perfil de destino.
#[derive(Debug, Default)]
pub struct TargetReport {
    pub name: String,
    pub device: Option<String>,
    pub clock: Option<String>,
    pub clock_description: Option<String>,
    pub config: Vec<(String, String)>,
    pub output: Option<String>,
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
}

impl TargetReport {
    pub fn ok(&self) -> bool {
        self.errors.is_empty()
    }
}

/// Revisa un perfil sin escribir nada: dispositivo y pack, reloj, configuracion y el analisis del programa para ese destino.
pub fn check_target(project: &Project, name: &str, t: &Target) -> TargetReport {
    let mut r = TargetReport { name: name.to_string(), device: t.device.clone(), clock: t.clock.clone(), config: t.config.clone(), output: t.output.as_ref().map(|o| o.display().to_string()), ..Default::default() };
    let devices_dir = project.devices_dir.as_ref().map(|d| d.to_string_lossy().into_owned());
    let src = match &project.main {
        Some(m) => match std::fs::read_to_string(m) {
            Ok(s) => Some(s),
            Err(e) => {
                r.errors.push(format!("no se pudo leer el programa '{}': {e}", m.display()));
                None
            }
        },
        None => {
            r.warnings.push("el proyecto no define 'main' ([project] main = \"...\"): se compila pasando el programa a pypic build".into());
            None
        }
    };
    let declared = src.as_deref().and_then(pypic_frontend::declared_device);
    if let (Some(d), Some(p)) = (&declared, &t.device) {
        if !d.eq_ignore_ascii_case(p) {
            r.errors.push(format!("el programa declara 'from pypic.device import {d}' y el perfil '{name}' pide {p}"));
        }
    }
    let Some(device) = declared.clone().or_else(|| t.device.clone()) else {
        r.errors.push(format!("falta el dispositivo: ponlo en el perfil ([target.{name}] device = \"PIC16F887\") o en el programa (from pypic.device import PIC16F887)"));
        return r;
    };
    r.device = Some(device.clone());
    let pack = match locate_pack(&device, devices_dir.as_deref()) {
        Ok(p) => p,
        Err((m, _)) => {
            r.errors.push(m);
            return r;
        }
    };
    let target = pack.target_profile();
    let mut project_clock = None;
    if let Some(text) = &t.clock {
        match pypic_frontend::parse_clock(text, &target) {
            Ok(c) => {
                r.clock_description = Some(c.describe());
                project_clock = Some(c);
            }
            Err(e) => r.errors.push(format!("[target.{name}] clock = \"{text}\": {}", e.trim().replace('\n', " "))),
        }
    }
    let mut words = pack.config.build_words();
    for (n, v) in &t.config {
        if let Err(e) = pack.config.apply(&mut words, n, v) {
            r.errors.push(format!("[target.{name}] config {n} = {v}: {e}"));
        }
    }
    if let Some(src) = &src {
        let mut tp = target;
        if !pypic_frontend::declares_clock(src) {
            tp.project_clock = project_clock.clone();
        } else if let Some(c) = &project_clock {
            if let Some(p) = analyze(src, &tp).program {
                if p.clock_source.as_ref() != Some(c) {
                    r.warnings.push(format!("el perfil '{name}' pide el reloj {} pero el programa declara el suyo: manda el programa", c.describe()));
                }
            }
        }
        let res = analyze(src, &tp);
        let file = project.main.as_ref().map(|m| m.display().to_string()).unwrap_or_default();
        for d in &res.diagnostics.items {
            let (line, col) = pypic_frontend::diag::line_col(src, d.span.start as usize);
            let text = format!("{file}:{line}:{col}: {} {}", d.code, d.message);
            match d.severity {
                pypic_frontend::Severity::Error => r.errors.push(text),
                _ => r.warnings.push(text),
            }
        }
    }
    r
}

fn json_report(project: &Project, reports: &[TargetReport]) -> serde_json::Value {
    use serde_json::json;
    json!({
        "version": REPORT_VERSION,
        "file": project.file.display().to_string(),
        "name": project.name,
        "main": project.main.as_ref().map(|m| m.display().to_string()),
        "devices_dir": project.devices_dir.as_ref().map(|m| m.display().to_string()),
        "default_target": project.default_target,
        "ok": reports.iter().all(TargetReport::ok),
        "targets": reports.iter().map(|r| json!({
            "name": r.name,
            "status": if r.ok() { "ok" } else { "error" },
            "device": r.device,
            "clock": r.clock.as_ref().map(|c| json!({ "declaration": c, "description": r.clock_description })),
            "config": r.config.iter().map(|(n, v)| (n.clone(), json!(v))).collect::<serde_json::Map<_, _>>(),
            "output": r.output,
            "errors": r.errors,
            "warnings": r.warnings,
        })).collect::<Vec<_>>(),
    })
}

/// `pypic project check [--project pypic.toml] [--target perfil] [--format text|json]`
pub fn project_cmd(args: &[String]) -> ExitCode {
    let Some(sub) = args.first() else { return crate::usage() };
    if sub != "check" {
        eprintln!("'pypic project {sub}' no existe todavia (hay: check)");
        return ExitCode::from(2);
    }
    let (mut project_arg, mut target_arg, mut json) = (None::<String>, None::<String>, false);
    let mut it = args[1..].iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--project" => project_arg = it.next().cloned(),
            "--target" => target_arg = it.next().cloned(),
            "--format" => match it.next().map(String::as_str) {
                Some("json") => json = true,
                Some("text") => json = false,
                _ => {
                    eprintln!("--format es 'text' o 'json'");
                    return ExitCode::from(2);
                }
            },
            _ => return crate::usage(),
        }
    }
    let file = match project_arg.map(PathBuf::from).or_else(|| project::find(Path::new("."))) {
        Some(f) => f,
        None => {
            eprintln!("no hay un {} en esta carpeta ni en las de arriba (crea uno con: pypic init)", project::FILE);
            return ExitCode::from(2);
        }
    };
    let project = match project::load(&file) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::from(2);
        }
    };
    let mut targets: Vec<(String, Target)> = Vec::new();
    match &target_arg {
        Some(_) => match project.target(target_arg.as_deref()) {
            Ok(Some(t)) => targets.push(t),
            Ok(None) => {}
            Err(e) => {
                eprintln!("{e}");
                return ExitCode::from(2);
            }
        },
        None => targets.extend(project.targets.iter().map(|(n, t)| (n.clone(), t.clone()))),
    }
    let reports: Vec<TargetReport> = targets.iter().map(|(n, t)| check_target(&project, n, t)).collect();
    let ok = reports.iter().all(TargetReport::ok);
    if json {
        println!("{}", serde_json::to_string_pretty(&json_report(&project, &reports)).expect("el informe es JSON valido"));
    } else {
        println!("Proyecto: {}{}", project.file.display(), project.name.as_ref().map(|n| format!(" ({n})")).unwrap_or_default());
        if reports.is_empty() {
            println!("  sin perfiles de destino ([target.NOMBRE]): nada que revisar");
        }
        for r in &reports {
            println!("  perfil {}: {}", r.name, if r.ok() { "ok" } else { "CON ERRORES" });
            println!("    dispositivo: {}", r.device.as_deref().unwrap_or("(sin definir)"));
            if let Some(c) = &r.clock_description {
                println!("    reloj:       {c}");
            }
            for e in &r.errors {
                println!("    error:   {e}");
            }
            for w in &r.warnings {
                println!("    aviso:   {w}");
            }
        }
    }
    if ok { ExitCode::SUCCESS } else { ExitCode::from(1) }
}
