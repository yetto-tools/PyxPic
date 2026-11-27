//! `pyxpic clean`: borra lo que `pyxpic build` escribe, y solo eso (docs/compilar-pyxpic.md).
//!
//! `build` escribe un unico archivo, el `.hex`: en `-o`, o en la `output` del perfil, o junto al programa con la extension `.hex`. `clean`
//! calcula **la misma ruta** que `build` calcularia y borra ese archivo; no recorre carpetas ni borra por patron. Nunca borra un `.py`, el
//! `pyxpic.toml` ni un directorio, y `--dry-run` lista lo que borraria sin tocar nada.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use crate::project;

/// Las rutas que `build` escribiria con estos argumentos (sin duplicados, en orden).
fn outputs(file: Option<&Path>, out: Option<&Path>, project: Option<&project::Project>, target: Option<&str>) -> Result<Vec<PathBuf>, String> {
    let mut paths: Vec<PathBuf> = Vec::new();
    let mut add = |p: PathBuf| {
        if !paths.contains(&p) {
            paths.push(p);
        }
    };
    if let Some(out) = out {
        // -o manda sobre el perfil, como en build
        add(out.to_path_buf());
        return Ok(paths);
    }
    match project {
        Some(pj) => {
            let profiles: Vec<project::Target> = match target {
                Some(t) => pj.target(Some(t))?.into_iter().map(|(_, t)| t).collect(),
                None => pj.targets.values().cloned().collect(),
            };
            let main = file.map(Path::to_path_buf).or_else(|| pj.main.clone());
            for t in &profiles {
                match (&t.output, &main) {
                    (Some(o), _) => add(o.clone()),
                    // un perfil sin output escribe el .hex junto al programa principal
                    (None, Some(m)) => add(m.with_extension("hex")),
                    (None, None) => {}
                }
            }
            if profiles.is_empty() {
                if let Some(m) = &main {
                    add(m.with_extension("hex"));
                }
            }
        }
        None => {
            if let Some(f) = file {
                add(f.with_extension("hex"));
            }
        }
    }
    Ok(paths)
}

/// `Some(motivo)` si `path` no es algo que `build` pueda haber escrito y por tanto `clean` no debe tocar.
fn protected(path: &Path, project_file: Option<&Path>) -> Option<&'static str> {
    let ext = path.extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase);
    if matches!(ext.as_deref(), Some("py" | "pyi" | "toml" | "s" | "asm" | "rs")) {
        return Some("es un archivo fuente o de configuracion");
    }
    if project_file.is_some_and(|f| f.file_name() == path.file_name() && f.parent() == path.parent()) {
        return Some("es el archivo de proyecto");
    }
    if path.is_dir() {
        return Some("es un directorio");
    }
    None
}

pub fn clean(args: &[String]) -> ExitCode {
    let (mut file, mut out, mut project_arg, mut target_arg, mut dry) = (None::<String>, None::<String>, None::<String>, None::<String>, false);
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "-o" => out = it.next().cloned(),
            "--project" => project_arg = it.next().cloned(),
            "--target" => target_arg = it.next().cloned(),
            "--dry-run" => dry = true,
            s if s.starts_with('-') => return crate::usage(),
            _ if file.is_none() => file = Some(a.clone()),
            _ => return crate::usage(),
        }
    }
    let start = file.as_deref().map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."));
    let found = project_arg.map(PathBuf::from).or_else(|| project::find(&start));
    let project = match found.as_deref().map(project::load) {
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
    if project.is_none() && file.is_none() && out.is_none() {
        eprintln!("pyxpic clean necesita un programa (pyxpic clean archivo.py), un -o salida.hex o un pyxpic.toml cerca");
        return ExitCode::from(2);
    }
    let paths = match outputs(file.as_deref().map(Path::new), out.as_deref().map(Path::new), project.as_ref(), target_arg.as_deref()) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::from(2);
        }
    };
    if paths.is_empty() {
        println!("No hay nada que limpiar: el proyecto no define ninguna salida.");
        return ExitCode::SUCCESS;
    }
    let project_file = project.as_ref().map(|p| p.file.as_path());
    let project_dir = project_file.and_then(Path::parent).map(Path::to_path_buf);
    // primero se revisa todo: si una ruta esta protegida no se borra nada
    for p in &paths {
        if let Some(why) = protected(p, project_file) {
            eprintln!("no se borra '{}': {why} (clean solo borra la salida de build)", p.display());
            return ExitCode::from(2);
        }
    }
    let (mut deleted, mut missing, mut failed) = (0, 0, 0);
    for p in &paths {
        if !p.exists() {
            println!("  no existe: {}", p.display());
            missing += 1;
        } else if dry {
            println!("  se borraria: {}", p.display());
            deleted += 1;
        } else {
            match std::fs::remove_file(p) {
                Ok(()) => {
                    println!("  borrado: {}", p.display());
                    deleted += 1;
                    // la carpeta de salida (build/) se quita si quedo vacia; la del proyecto y la actual nunca
                    if let Some(dir) = p.parent().filter(|d| !d.as_os_str().is_empty() && *d != Path::new(".") && Some(d.to_path_buf()) != project_dir) {
                        let _ = std::fs::remove_dir(dir);
                    }
                }
                Err(e) => {
                    eprintln!("  no se pudo borrar {}: {e}", p.display());
                    failed += 1;
                }
            }
        }
    }
    let verb = if dry { "se borrarian" } else { "borrados" };
    println!("Limpieza: {deleted} archivo(s) {verb}, {missing} no existian{}", if failed > 0 { format!(", {failed} con error") } else { String::new() });
    if failed > 0 {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    }
}
