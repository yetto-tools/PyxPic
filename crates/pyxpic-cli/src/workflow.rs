//! `pyxpic init` y `pyxpic project check` (docs/proyecto.md): crear y revisar un proyecto con `pyxpic.toml`. Son la base del asistente de la
//! extension: la extension solo presenta lo que estas ordenes dicen (la logica vive aqui y se prueba sin VS Code).
//!
//! Regla: **nunca se sobrescribe nada** y `project check` no modifica ningun archivo.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use pyxpic_device::DevicePack;
use pyxpic_frontend::{analyze, analyze_in, declared_device_in, declares_clock_in, Language, TargetProfile};

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

/// Busca el Device Pack de `name` (`PIC16F887` -> `<raiz>/pic16f887`) en `extra`, `$PYXPIC_DEVICES` y `./devices`.
pub fn locate_pack(name: &str, extra: Option<&str>) -> Result<DevicePack, (String, u8)> {
    let mut roots: Vec<String> = Vec::new();
    roots.extend(extra.map(String::from));
    roots.extend(std::env::var("PYXPIC_DEVICES").ok());
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
    Err((format!("dispositivo '{name}' desconocido: no hay Device Pack en {} (disponibles: {})", roots.join(", "), if avail.is_empty() { "ninguno".to_string() } else { avail.join(", ") }), 1))
}

// ---- pyxpic init -----------------------------------------------------------------------------------------------------------------------

/// `4_000_000` -> `4MHz`, `125_000` -> `125kHz`.
fn hz_expr(hz: u32) -> String {
    if hz.is_multiple_of(1_000_000) {
        format!("{}MHz", hz / 1_000_000)
    } else if hz.is_multiple_of(1_000) {
        format!("{}kHz", hz / 1_000)
    } else {
        format!("{hz}Hz")
    }
}

fn toml_str(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "/").replace('"', "\\\""))
}

const MAIN_TEMPLATE: &str = "\"\"\"@NAME@: parpadeo de RB0 cada 500 ms. El dispositivo se declara aqui, en el codigo; el reloj y la configuracion estan en pyxpic.toml (docs/proyecto.md).\"\"\"\nfrom pyxpic.device import @DEVICE@\nfrom pyxpic.periph.gpio import RB0\nfrom pyxpic.system.time import pause\n\n\ndef main() -> None:\n    RB0.output()\n    while True:\n        RB0.toggle()\n        pause(500)\n";

/// `pyxpic init [carpeta] [--name N] [--device PIC16F887] [--clock "Internal 4MHz"] [--devices-dir D]`
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
            Some(hz) => format!("Internal {}", hz_expr(hz)),
            None => {
                eprintln!("{device} no declara un reloj por defecto: indica uno con --clock \"Crystal 20MHz\"");
                return ExitCode::from(2);
            }
        },
    };
    if let Err(e) = pyxpic_frontend::parse_clock(&clock, &target) {
        eprint!("{e}");
        eprintln!("--clock \"{clock}\" no es valido para {device}");
        return ExitCode::from(1);
    }
    let main_src = MAIN_TEMPLATE.replace("@NAME@", &name).replace("@DEVICE@", &device);
    // el programa de partida debe ser valido para este dispositivo y este reloj antes de escribir nada
    let mut probe: TargetProfile = target.clone();
    probe.project_clock = pyxpic_frontend::parse_clock(&clock, &target).ok();
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
    let mut toml = format!("# Proyecto PyxPic (docs/proyecto.md): un perfil por destino.\n[project]\nname = {}\nmain = \"main.py\"\n", toml_str(&name));
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
    println!("siguiente: cd {} && pyxpic build   (o: pyxpic project check)", dir.display());
    ExitCode::SUCCESS
}

// ---- pyxpic project check ----------------------------------------------------------------------------------------------------------

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
    check_target_with(project, name, t, None)
}

/// `check_target` con el programa en memoria (`source`) en lugar del que hay en disco.
fn check_target_with(project: &Project, name: &str, t: &Target, source: Option<&str>) -> TargetReport {
    let mut r = TargetReport {
        name: name.to_string(),
        device: t.device.clone(),
        clock: t.clock.clone(),
        config: t.config.clone(),
        output: t.output.as_ref().map(|o| o.display().to_string()),
        ..Default::default()
    };
    let devices_dir = project.devices_dir.as_ref().map(|d| d.to_string_lossy().into_owned());
    let src = match &project.main {
        Some(m) => match source.map(|s| Ok(s.to_string())).unwrap_or_else(|| std::fs::read_to_string(m)) {
            Ok(s) => Some(s),
            Err(e) => {
                r.errors.push(format!("no se pudo leer el programa '{}': {e}", m.display()));
                None
            }
        },
        None => {
            r.warnings.push("el proyecto no define 'main' ([project] main = \"...\"): se compila pasando el programa a pyxpic build".into());
            None
        }
    };
    // el lenguaje del programa principal: .bas es PyxPic BASIC, lo demas Python
    let lang = project.main.as_ref().map_or(Language::Python, |m| Language::from_path(&m.to_string_lossy()));
    let declared = src.as_deref().and_then(|s| declared_device_in(lang, s));
    if let (Some(d), Some(p)) = (&declared, &t.device) {
        if !d.eq_ignore_ascii_case(p) {
            r.errors.push(format!("el programa declara 'from pyxpic.device import {d}' y el perfil '{name}' pide {p}"));
        }
    }
    let Some(device) = declared.clone().or_else(|| t.device.clone()) else {
        r.errors.push(format!("falta el dispositivo: ponlo en el perfil ([target.{name}] device = \"PIC16F887\") o en el programa (from pyxpic.device import PIC16F887)"));
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
        match pyxpic_frontend::parse_clock(text, &target) {
            Ok(c) => {
                r.clock_description = Some(c.describe_for(pack.registers.get("OSCCON").is_some()));
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
        if !declares_clock_in(lang, src) {
            tp.project_clock = project_clock.clone();
        } else if let Some(c) = &project_clock {
            if let Some(p) = analyze_in(lang, src, &tp).program {
                if p.clock_source.as_ref() != Some(c) {
                    r.warnings.push(format!("el perfil '{name}' pide el reloj {} pero el programa declara el suyo: manda el programa", c.describe()));
                }
            }
        }
        let res = analyze_in(lang, src, &tp);
        let file = project.main.as_ref().map(|m| m.display().to_string()).unwrap_or_default();
        for d in &res.diagnostics.items {
            let (line, col) = pyxpic_frontend::diag::line_col(src, d.span.start as usize);
            let text = format!("{file}:{line}:{col}: {} {}", d.code, d.message);
            match d.severity {
                pyxpic_frontend::Severity::Error => r.errors.push(text),
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

/// `pyxpic project check [--project pyxpic.toml] [--target perfil] [--format text|json]`
pub fn project_cmd(args: &[String]) -> ExitCode {
    let Some(sub) = args.first() else { return crate::usage() };
    if sub == "set" {
        return set_cmd(&args[1..]);
    }
    if sub == "retarget" {
        return retarget_cmd(&args[1..]);
    }
    if sub != "check" {
        eprintln!("'pyxpic project {sub}' no existe (hay: check, set, retarget)");
        return ExitCode::from(2);
    }
    let (mut project_arg, mut target_arg, mut json, mut devices_arg) = (None::<String>, None::<String>, false, None::<String>);
    let mut it = args[1..].iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--project" => project_arg = it.next().cloned(),
            "--target" => target_arg = it.next().cloned(),
            "--devices-dir" => devices_arg = it.next().cloned(),
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
            eprintln!("no hay un {} en esta carpeta ni en las de arriba (crea uno con: pyxpic init)", project::FILE);
            return ExitCode::from(2);
        }
    };
    let mut project = match project::load(&file) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::from(2);
        }
    };
    // como en build: --devices-dir manda sobre el devices_dir del proyecto
    if let Some(d) = devices_arg {
        project.devices_dir = Some(PathBuf::from(d));
    }
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
    if ok {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    }
}

// ---- pyxpic project set ------------------------------------------------------------------------------------------------------------

use toml_edit::{DocumentMut, InlineTable, Item, Table, Value};

const SET_KEYS: &str = "device, clock, output, config.AJUSTE, project.name, project.main, project.devices_dir, project.default_target";

/// Pone `val` en `key` conservando el comentario y el espacio que ya tenia el valor; devuelve el valor anterior como texto.
fn put(tl: &mut dyn toml_edit::TableLike, key: &str, val: Value) -> Option<String> {
    let old = tl.get(key).and_then(Item::as_value).map(|v| v.as_str().map(String::from).unwrap_or_else(|| v.to_string().trim().to_string()));
    match tl.get_mut(key).and_then(Item::as_value_mut) {
        Some(v) => {
            let mut nv = val;
            *nv.decor_mut() = v.decor().clone();
            *v = nv;
        }
        None => {
            tl.insert(key, Item::Value(val));
        }
    }
    old
}

/// Un valor de configuracion: un entero si lo es (`BOREN = 1`), si no un texto (`WDTE = "ON"`).
fn config_value(v: &str) -> Value {
    v.parse::<i64>().map(Value::from).unwrap_or_else(|_| Value::from(v))
}

/// `pyxpic project set [--project f] [--target perfil] [--create] [--dry-run] [--unset CLAVE]... [CLAVE=VALOR]...`
///
/// Cambia el perfil sin tocar el resto del archivo (comentarios y formato se conservan). **Valida el resultado entero antes de escribir**:
/// si el perfil queda con errores no se escribe nada. Antes de modificar guarda `pyxpic.toml.bak`.
pub fn set_cmd(args: &[String]) -> ExitCode {
    let (mut project_arg, mut target_arg, mut create, mut dry) = (None::<String>, None::<String>, false, false);
    let mut sets: Vec<(String, String)> = Vec::new();
    let mut unsets: Vec<String> = Vec::new();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--project" => project_arg = it.next().cloned(),
            "--target" => target_arg = it.next().cloned(),
            "--create" => create = true,
            "--dry-run" => dry = true,
            "--unset" => match it.next() {
                Some(k) => unsets.push(k.clone()),
                None => return crate::usage(),
            },
            s if s.starts_with("--") => return crate::usage(),
            _ => match a.split_once('=') {
                Some((k, v)) => sets.push((k.trim().to_string(), v.trim().to_string())),
                None => {
                    eprintln!("'{a}': se escribe CLAVE=VALOR, por ejemplo clock=\"Crystal 20MHz\" o config.WDTE=ON (claves: {SET_KEYS})");
                    return ExitCode::from(2);
                }
            },
        }
    }
    if sets.is_empty() && unsets.is_empty() {
        eprintln!("no hay nada que cambiar: pasa CLAVE=VALOR o --unset CLAVE (claves: {SET_KEYS})");
        return ExitCode::from(2);
    }
    let file = match project_arg.map(PathBuf::from).or_else(|| project::find(Path::new("."))) {
        Some(f) => f,
        None => {
            eprintln!("no hay un {} en esta carpeta ni en las de arriba (crea uno con: pyxpic init)", project::FILE);
            return ExitCode::from(2);
        }
    };
    let text = match std::fs::read_to_string(&file) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("no se pudo leer '{}': {e}", file.display());
            return ExitCode::from(2);
        }
    };
    let project = match project::parse(&file, &text) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::from(2);
        }
    };
    let mut doc: DocumentMut = match text.parse() {
        Ok(d) => d,
        Err(e) => {
            eprintln!("{}: {e}", file.display());
            return ExitCode::from(2);
        }
    };
    // las claves de proyecto no necesitan perfil; las de perfil, si
    let all: Vec<&str> = sets.iter().map(|(k, _)| k.as_str()).chain(unsets.iter().map(String::as_str)).collect();
    let needs_target = all.iter().any(|k| !k.starts_with("project."));
    let target_name = if !needs_target {
        None
    } else if let Some(t) = &target_arg {
        if !project.targets.contains_key(t) && !create {
            eprintln!("{}: no hay un perfil '{t}' ([target.{t}]); hay: {} (usa --create para crearlo)", file.display(), project.targets.keys().cloned().collect::<Vec<_>>().join(", "));
            return ExitCode::from(2);
        }
        Some(t.clone())
    } else {
        match project.target(None) {
            Ok(Some((n, _))) => Some(n),
            Ok(None) => {
                eprintln!("{}: no hay perfiles: crea uno con --target NOMBRE --create device=PIC16F887", file.display());
                return ExitCode::from(2);
            }
            Err(e) => {
                eprintln!("{e}");
                return ExitCode::from(2);
            }
        }
    };
    let mut changes: Vec<String> = Vec::new();
    let show = |old: Option<String>, new: Option<&str>| match (old, new) {
        (Some(o), Some(n)) => format!("{o} -> {n}"),
        (None, Some(n)) => format!("(nuevo) {n}"),
        (Some(o), None) => format!("{o} -> (quitado)"),
        (None, None) => "(sin cambio)".to_string(),
    };
    let bad_key = |k: &str| {
        eprintln!("la clave '{k}' no existe (claves: {SET_KEYS})");
        ExitCode::from(2)
    };

    for (is_unset, key, value) in sets.iter().map(|(k, v)| (false, k.as_str(), Some(v.as_str()))).chain(unsets.iter().map(|k| (true, k.as_str(), None))) {
        let _ = is_unset;
        if let Some(pk) = key.strip_prefix("project.") {
            if !["name", "main", "devices_dir", "default_target"].contains(&pk) {
                return bad_key(key);
            }
            let root = doc.as_table_mut();
            let proj = root.entry("project").or_insert_with(|| Item::Table(Table::new()));
            let Some(tl) = proj.as_table_like_mut() else {
                eprintln!("[project] no es una tabla");
                return ExitCode::from(2);
            };
            let old = match value {
                Some(v) => put(tl, pk, Value::from(v)),
                None => tl.remove(pk).and_then(|i| i.as_str().map(String::from)),
            };
            changes.push(format!("project.{pk}: {}", show(old, value)));
            continue;
        }
        let name = target_name.clone().expect("hay perfil para las claves de perfil");
        let root = doc.as_table_mut();
        let targets = root.entry("target").or_insert_with(|| {
            let mut t = Table::new();
            t.set_implicit(true);
            Item::Table(t)
        });
        let Some(ttl) = targets.as_table_like_mut() else {
            eprintln!("'target' no es una tabla");
            return ExitCode::from(2);
        };
        if !ttl.contains_key(&name) {
            ttl.insert(&name, Item::Table(Table::new()));
            changes.push(format!("perfil {name}: (nuevo)"));
        }
        let Some(tl) = ttl.get_mut(&name).and_then(Item::as_table_like_mut) else {
            eprintln!("[target.{name}] no es una tabla");
            return ExitCode::from(2);
        };
        match key {
            "device" | "clock" | "output" => {
                let old = match value {
                    Some(v) => put(tl, key, Value::from(v)),
                    None => tl.remove(key).and_then(|i| i.as_str().map(String::from)),
                };
                changes.push(format!("{name}.{key}: {}", show(old, value)));
            }
            _ if key.starts_with("config.") && key.len() > 7 => {
                let setting = &key[7..];
                if !tl.contains_key("config") {
                    if value.is_none() {
                        changes.push(format!("{name}.{key}: (sin cambio)"));
                        continue;
                    }
                    tl.insert("config", Item::Value(Value::InlineTable(InlineTable::new())));
                }
                let Some(cfg) = tl.get_mut("config").and_then(Item::as_table_like_mut) else {
                    eprintln!("[target.{name}] config no es una tabla");
                    return ExitCode::from(2);
                };
                let old = match value {
                    Some(v) => put(cfg, setting, config_value(v)),
                    None => cfg.remove(setting).and_then(|i| i.as_value().map(|v| v.as_str().map(String::from).unwrap_or_else(|| v.to_string().trim().to_string()))),
                };
                let empty = cfg.is_empty();
                changes.push(format!("{name}.{key}: {}", show(old, value)));
                if empty {
                    tl.remove("config");
                }
            }
            _ => return bad_key(key),
        }
    }

    // se valida el resultado entero antes de escribir: estructura y el perfil cambiado
    let new_text = doc.to_string();
    let new_project = match project::parse(&file, &new_text) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("el cambio dejaria el archivo invalido, no se escribe nada:\n{e}");
            return ExitCode::from(1);
        }
    };
    let mut warnings: Vec<String> = Vec::new();
    if let Some(name) = &target_name {
        match new_project.targets.get(name) {
            Some(t) => {
                let r = check_target(&new_project, name, t);
                if !r.ok() {
                    eprintln!("el perfil '{name}' quedaria con errores, no se escribe nada:");
                    for e in &r.errors {
                        eprintln!("  error: {e}");
                    }
                    return ExitCode::from(1);
                }
                warnings = r.warnings;
            }
            None => {
                eprintln!("el perfil '{name}' ya no existe tras el cambio, no se escribe nada");
                return ExitCode::from(1);
            }
        }
    }
    for c in &changes {
        println!("  {c}");
    }
    for w in &warnings {
        println!("  aviso: {w}");
    }
    if dry {
        println!("--dry-run: no se escribio nada ({})", file.display());
        return ExitCode::SUCCESS;
    }
    if new_text == text {
        println!("sin cambios en {}", file.display());
        return ExitCode::SUCCESS;
    }
    let bak = file.with_file_name(format!("{}.bak", file.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| project::FILE.to_string())));
    if let Err(e) = std::fs::write(&bak, &text).and_then(|_| std::fs::write(&file, &new_text)) {
        eprintln!("no se pudo escribir '{}': {e}", file.display());
        return ExitCode::from(2);
    }
    println!("{} actualizado (la version anterior queda en {})", file.display(), bak.display());
    ExitCode::SUCCESS
}

// ---- pyxpic devices ----------------------------------------------------------------------------------------------------------------

/// Los Device Packs de las mismas raices que `locate_pack`, uno por nombre y en orden.
fn all_packs(extra: Option<&str>) -> Vec<DevicePack> {
    let mut roots: Vec<String> = Vec::new();
    roots.extend(extra.map(String::from));
    roots.extend(std::env::var("PYXPIC_DEVICES").ok());
    roots.push("devices".into());
    let mut packs = std::collections::BTreeMap::new();
    for root in &roots {
        let Ok(rd) = std::fs::read_dir(root) else { continue };
        for dir in rd.flatten().map(|e| e.path()).filter(|p| p.join("device.json").is_file()) {
            match load_pack_text(&dir.to_string_lossy()) {
                Ok(p) => {
                    packs.entry(p.device.name.clone()).or_insert(p);
                }
                Err((m, _)) => eprintln!("aviso: {m}"),
            }
        }
    }
    packs.into_values().collect()
}

/// Relojes que se ofrecen para un dispositivo, como los escribe `pyxpic.toml`: primero el del reset, luego el resto del oscilador interno y
/// los cristales habituales que admite. Cada uno se valida con las mismas reglas que el fuente.
fn clock_choices(pack: &DevicePack) -> Vec<(String, String, String)> {
    let target = pack.target_profile();
    let mut out = Vec::new();
    if let Some(osc) = &target.oscillator {
        let mut internal: Vec<u32> = osc.internal.iter().map(|f| f.0).collect();
        internal.sort_by_key(|&hz| (hz != osc.reset_hz, std::cmp::Reverse(hz)));
        for hz in internal {
            out.push((format!("Oscilador interno · {}", pyxpic_frontend::clock::fmt_hz(hz)), format!("Internal {}", hz_expr(hz)), "Sin componentes externos".to_string()));
        }
    }
    for hz in [20_000_000u32, 8_000_000, 4_000_000] {
        out.push((format!("Cristal · {}", pyxpic_frontend::clock::fmt_hz(hz)), format!("Crystal {}", hz_expr(hz)), "Cristal o resonador en OSC1/OSC2; esos pines dejan de ser E/S".to_string()));
    }
    out.retain(|(_, value, _)| pyxpic_frontend::parse_clock(value, &target).is_ok());
    out
}

/// `pyxpic devices [--format text|json] [--devices-dir dir]`: lo que usa el asistente de la extension.
pub fn devices_cmd(args: &[String]) -> ExitCode {
    let (mut json, mut devices_dir) = (false, None::<String>);
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--devices-dir" => devices_dir = it.next().cloned(),
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
    let packs = all_packs(devices_dir.as_deref());
    if json {
        use serde_json::json;
        let devices: Vec<_> = packs
            .iter()
            .map(|p| {
                let name = p.device.name.as_str();
                json!({
                    "name": name,
                    "family": p.device.family,
                    "program_words": p.memory.program.words,
                    "ram_bytes": p.memory.data.ram_bytes,
                    "eeprom_bytes": p.memory.eeprom.bytes,
                    "io_pins": p.device.io_pins,
                    "support_level": p.device.pyxpic.support_level,
                    "buildable": pyxpic_pic14::is_supported(name),
                    "limits": pyxpic_pic14::device_limits(name),
                    "clocks": clock_choices(p).into_iter().map(|(label, value, detail)| json!({ "label": label, "value": value, "detail": detail })).collect::<Vec<_>>(),
                })
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&json!({ "version": REPORT_VERSION, "devices": devices })).expect("el informe es JSON valido"));
    } else {
        for p in &packs {
            let name = p.device.name.as_str();
            let state = if !pyxpic_pic14::is_supported(name) { "sin backend: solo check" } else { pyxpic_pic14::device_limits(name).unwrap_or("compila") };
            println!("{name}: {} palabras, {} B de RAM, {} pines de E/S; {state}", p.memory.program.words, p.memory.data.ram_bytes, p.device.io_pins);
        }
    }
    if packs.is_empty() {
        eprintln!("no se encontro ningun Device Pack (usa --devices-dir o $PYXPIC_DEVICES)");
        return ExitCode::from(1);
    }
    ExitCode::SUCCESS
}

// ---- pyxpic project retarget -------------------------------------------------------------------------------------------------------

/// El programa con el dispositivo declarado cambiado a `device`; `None` si no declara ninguno. Solo toca la linea de la declaracion.
fn redeclare(lang: Language, src: &str, device: &str) -> Option<String> {
    let old = declared_device_in(lang, src)?;
    let mut done = false;
    let out: String = src
        .split_inclusive('\n')
        .map(|line| {
            let head = line.trim_start().to_ascii_lowercase();
            let is_decl = match lang {
                Language::Basic => head.starts_with("device"),
                _ => head.starts_with("from pyxpic.device import"),
            };
            match line.to_ascii_uppercase().rfind(&old.to_ascii_uppercase()) {
                Some(at) if is_decl && !done => {
                    done = true;
                    format!("{}{device}{}", &line[..at], &line[at + old.len()..])
                }
                _ => line.to_string(),
            }
        })
        .collect();
    done.then_some(out)
}

/// Compila en memoria para el destino del perfil: detecta lo que el analisis acepta pero el backend de ese PIC no genera todavia.
fn try_build(project: &Project, t: &Target, pack: &DevicePack, lang: Language, src: &str) -> Result<(), String> {
    let mut target = pack.target_profile();
    if !declares_clock_in(lang, src) {
        if let Some(text) = &t.clock {
            target.project_clock = Some(pyxpic_frontend::parse_clock(text, &target)?);
        }
    }
    let Some(program) = analyze_in(lang, src, &target).program else {
        return Err(format!("{} no es valido para {}", project.main.as_ref().map(|m| m.display().to_string()).unwrap_or_default(), pack.device.name));
    };
    let module = pyxpic_ir::lower(&program);
    let mut settings = t.config.clone();
    if let Some(cs) = &module.clock_source {
        settings.push(("FOSC".to_string(), cs.fosc.to_string()));
        settings.extend(cs.extra_config.iter().cloned());
    }
    let opts = crate::backend_options(pack, &settings)?;
    pyxpic_pic14::compile(&module, &opts).map(|_| ()).map_err(|e| e.to_string())
}

/// `pyxpic project retarget PIC [--clock DECL] [--project f] [--target perfil] [--dry-run] [--devices-dir dir]`
///
/// Cambia la declaracion del programa principal y el perfil a otro PIC. **Compila el resultado en memoria antes de escribir**: si algo no
/// vale para el chip nuevo (un pin que no existe, un reloj, un periferico) no se escribe nada. Guarda un `.bak` de cada archivo que cambia.
fn retarget_cmd(args: &[String]) -> ExitCode {
    let (mut device, mut clock, mut project_arg, mut target_arg, mut devices_arg, mut dry) = (None::<String>, None::<String>, None::<String>, None::<String>, None::<String>, false);
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--clock" => clock = it.next().cloned(),
            "--project" => project_arg = it.next().cloned(),
            "--target" => target_arg = it.next().cloned(),
            "--devices-dir" => devices_arg = it.next().cloned(),
            "--dry-run" => dry = true,
            s if s.starts_with("--") => return crate::usage(),
            _ if device.is_none() => device = Some(crate::pic_name(a)),
            _ => return crate::usage(),
        }
    }
    let Some(device) = device else { return crate::usage() };
    let fail = |m: String, code: u8| {
        eprintln!("{m}");
        ExitCode::from(code)
    };
    let Some(file) = project_arg.map(PathBuf::from).or_else(|| project::find(Path::new("."))) else {
        return fail(format!("no hay un {} en esta carpeta ni en las de arriba (crea uno con: pyxpic init)", project::FILE), 2);
    };
    let text = match std::fs::read_to_string(&file) {
        Ok(t) => t,
        Err(e) => return fail(format!("no se pudo leer '{}': {e}", file.display()), 2),
    };
    let mut old_project = match project::parse(&file, &text) {
        Ok(p) => p,
        Err(e) => return fail(e, 2),
    };
    if let Some(d) = &devices_arg {
        old_project.devices_dir = Some(PathBuf::from(d));
    }
    let (old_name, old_target) = match old_project.target(target_arg.as_deref()) {
        Ok(Some(t)) => t,
        Ok(None) => return fail(format!("{}: no hay perfiles de destino que cambiar", file.display()), 2),
        Err(e) => return fail(e, 2),
    };
    let devices_dir = old_project.devices_dir.as_ref().map(|d| d.to_string_lossy().into_owned());
    let pack = match locate_pack(&device, devices_dir.as_deref()) {
        Ok(p) => p,
        Err((m, c)) => return fail(m, c),
    };
    let device = pack.device.name.clone();
    let Some(main) = old_project.main.clone() else {
        return fail(format!("{}: el proyecto no define 'main': no hay programa que cambiar", file.display()), 2);
    };
    let src = match std::fs::read_to_string(&main) {
        Ok(s) => s,
        Err(e) => return fail(format!("no se pudo leer el programa '{}': {e}", main.display()), 2),
    };
    let lang = Language::from_path(&main.to_string_lossy());
    // un programa sin declaracion toma el dispositivo del perfil: no se toca
    let new_src = redeclare(lang, &src, &device).unwrap_or_else(|| src.clone());

    let mut doc: DocumentMut = match text.parse() {
        Ok(d) => d,
        Err(e) => return fail(format!("{}: {e}", file.display()), 2),
    };
    let old_device = old_target.device.clone().or_else(|| declared_device_in(lang, &src));
    // `init` llama al perfil como el dispositivo: ese nombre se actualiza tambien
    let new_name = match &old_device {
        Some(d) if d.eq_ignore_ascii_case(&old_name) && !old_project.targets.contains_key(&device.to_lowercase()) => device.to_lowercase(),
        _ => old_name.clone(),
    };
    {
        let Some(targets) = doc.get_mut("target").and_then(Item::as_table_like_mut) else {
            return fail("'target' no es una tabla".into(), 2);
        };
        if new_name != old_name {
            if let Some(item) = targets.remove(&old_name) {
                targets.insert(&new_name, item);
            }
        }
        let Some(table) = targets.get_mut(&new_name).and_then(Item::as_table_like_mut) else {
            return fail(format!("[target.{old_name}] no es una tabla"), 2);
        };
        put(table, "device", Value::from(device.as_str()));
        if let Some(c) = &clock {
            put(table, "clock", Value::from(c.as_str()));
        }
    }
    if new_name != old_name && old_project.default_target.as_deref() == Some(old_name.as_str()) {
        if let Some(p) = doc.get_mut("project").and_then(Item::as_table_like_mut) {
            put(p, "default_target", Value::from(new_name.as_str()));
        }
    }
    let new_text = doc.to_string();
    let mut new_project = match project::parse(&file, &new_text) {
        Ok(p) => p,
        Err(e) => return fail(format!("el cambio dejaria el archivo invalido, no se escribe nada:\n{e}"), 1),
    };
    new_project.devices_dir = old_project.devices_dir.clone();
    let Some(new_target) = new_project.targets.get(&new_name).cloned() else {
        return fail(format!("el perfil '{new_name}' ya no existe tras el cambio, no se escribe nada"), 1);
    };
    let report = check_target_with(&new_project, &new_name, &new_target, Some(&new_src));
    let mut errors = report.errors;
    if errors.is_empty() {
        if let Err(e) = try_build(&new_project, &new_target, &pack, lang, &new_src) {
            errors.push(e);
        }
    }
    if !errors.is_empty() {
        eprintln!("el proyecto no se puede pasar a {device}, no se escribe nada:");
        for e in &errors {
            eprintln!("  error: {e}");
        }
        eprintln!("corrige el programa (pines, perifericos) o el perfil (--clock) y repite");
        return ExitCode::from(1);
    }
    for w in &report.warnings {
        println!("  aviso: {w}");
    }
    println!("  dispositivo: {} -> {device}", old_device.as_deref().unwrap_or("(sin definir)"));
    if new_name != old_name {
        println!("  perfil: {old_name} -> {new_name}");
    }
    if let Some(l) = pyxpic_pic14::device_limits(&device) {
        println!("  limites de {device}: {l}");
    }
    if dry {
        println!("--dry-run: no se escribio nada");
        return ExitCode::SUCCESS;
    }
    for (path, old, new) in [(&main, &src, &new_src), (&file, &text, &new_text)] {
        if old == new {
            continue;
        }
        let bak = path.with_file_name(format!("{}.bak", path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()));
        if let Err(e) = std::fs::write(&bak, old).and_then(|_| std::fs::write(path, new)) {
            return fail(format!("no se pudo escribir '{}': {e}", path.display()), 2);
        }
        println!("  {} actualizado (la version anterior queda en {})", path.display(), bak.display());
    }
    ExitCode::SUCCESS
}
