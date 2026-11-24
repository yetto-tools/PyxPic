//! `pypic.toml`: el archivo de proyecto (docs/proyecto.md). Da al compilador el programa principal, la carpeta de Device Packs y uno o
//! mas perfiles por destino (dispositivo, reloj, palabras de configuracion y salida) sin editar el fuente.
//!
//! Precedencia: **el fuente manda** sobre el perfil en lo que declara (dispositivo, reloj); el perfil completa lo que el fuente no
//! dice, y las opciones de la linea de comandos mandan sobre el perfil.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub const FILE: &str = "pypic.toml";

/// Un perfil de destino (`[target.NOMBRE]`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Target {
    pub device: Option<String>,
    /// La declaracion del reloj como en el fuente: `Crystal[20 * MHz]`.
    pub clock: Option<String>,
    /// Palabras de configuracion por nombre, como `--config`.
    pub config: Vec<(String, String)>,
    pub output: Option<PathBuf>,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Project {
    pub file: PathBuf,
    pub name: Option<String>,
    pub main: Option<PathBuf>,
    pub devices_dir: Option<PathBuf>,
    pub default_target: Option<String>,
    pub targets: BTreeMap<String, Target>,
}

/// El `pypic.toml` mas cercano subiendo desde `start`.
pub fn find(start: &Path) -> Option<PathBuf> {
    let start = if start.is_dir() { start.to_path_buf() } else { start.parent()?.to_path_buf() };
    let start = if start.as_os_str().is_empty() { PathBuf::from(".") } else { start };
    let start = strip_verbatim(start.canonicalize().ok()?);
    start.ancestors().map(|d| d.join(FILE)).find(|f| f.is_file())
}

/// En Windows `canonicalize` devuelve rutas con el prefijo `\?` (verbatim): se muestran y se unen sin ese prefijo.
fn strip_verbatim(p: PathBuf) -> PathBuf {
    match p.to_str().and_then(|s| s.strip_prefix(r"\\?\")) {
        Some(rest) => PathBuf::from(rest),
        None => p,
    }
}

fn bad_key(file: &Path, place: &str, key: &str, allowed: &[&str]) -> String {
    format!("{}: [{place}] la clave '{key}' no existe (admite: {})", file.display(), allowed.join(", "))
}

fn string(file: &Path, place: &str, key: &str, v: &toml::Value) -> Result<String, String> {
    match v {
        toml::Value::String(s) if !s.trim().is_empty() => Ok(s.trim().to_string()),
        _ => Err(format!("{}: [{place}] '{key}' es un texto no vacio", file.display())),
    }
}

pub fn load(path: &Path) -> Result<Project, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("no se pudo leer '{}': {e}", path.display()))?;
    parse(path, &text)
}

pub fn parse(path: &Path, text: &str) -> Result<Project, String> {
    let table: toml::Table = text.parse().map_err(|e: toml::de::Error| format!("{}: {}", path.display(), e.to_string().trim_end()))?;
    let dir = path.parent().map(Path::to_path_buf).unwrap_or_default();
    let mut p = Project { file: path.to_path_buf(), ..Default::default() };
    for (key, v) in &table {
        match (key.as_str(), v) {
            ("project", toml::Value::Table(t)) => {
                for (k, v) in t {
                    match k.as_str() {
                        "name" => p.name = Some(string(path, "project", k, v)?),
                        "main" => p.main = Some(dir.join(string(path, "project", k, v)?)),
                        "devices_dir" => p.devices_dir = Some(dir.join(string(path, "project", k, v)?)),
                        "default_target" => p.default_target = Some(string(path, "project", k, v)?),
                        _ => return Err(bad_key(path, "project", k, &["name", "main", "devices_dir", "default_target"])),
                    }
                }
            }
            ("target", toml::Value::Table(t)) => {
                for (name, v) in t {
                    let toml::Value::Table(tt) = v else { return Err(format!("{}: [target.{name}] debe ser una tabla", path.display())) };
                    let place = format!("target.{name}");
                    let mut tg = Target::default();
                    for (k, v) in tt {
                        match k.as_str() {
                            "device" => tg.device = Some(string(path, &place, k, v)?),
                            "clock" => tg.clock = Some(string(path, &place, k, v)?),
                            "output" => tg.output = Some(dir.join(string(path, &place, k, v)?)),
                            "config" => {
                                let toml::Value::Table(c) = v else { return Err(format!("{}: [{place}] 'config' es una tabla: config = {{ WDTE = \"ON\" }}", path.display())) };
                                for (n, val) in c {
                                    let val = match val {
                                        toml::Value::String(s) => s.trim().to_string(),
                                        toml::Value::Integer(i) => i.to_string(),
                                        toml::Value::Boolean(b) => (if *b { "ON" } else { "OFF" }).to_string(),
                                        _ => return Err(format!("{}: [{place}] config.{n} es un texto o un numero", path.display())),
                                    };
                                    tg.config.push((n.clone(), val));
                                }
                            }
                            _ => return Err(bad_key(path, &place, k, &["device", "clock", "config", "output"])),
                        }
                    }
                    p.targets.insert(name.clone(), tg);
                }
            }
            ("project" | "target", _) => return Err(format!("{}: '{key}' debe ser una tabla ([{key}])", path.display())),
            _ => return Err(format!("{}: la seccion '{key}' no existe (admite: project, target)", path.display())),
        }
    }
    if let Some(d) = &p.default_target {
        if !p.targets.contains_key(d) {
            return Err(format!("{}: [project] default_target = \"{d}\" pero no hay [target.{d}] (hay: {})", path.display(), names(&p)));
        }
    }
    Ok(p)
}

fn names(p: &Project) -> String {
    if p.targets.is_empty() { "ninguno".to_string() } else { p.targets.keys().cloned().collect::<Vec<_>>().join(", ") }
}

impl Project {
    /// El perfil pedido con `--target`, o el `default_target`, o el unico que haya; `None` si el proyecto no define ninguno.
    pub fn target(&self, wanted: Option<&str>) -> Result<Option<(String, Target)>, String> {
        if let Some(w) = wanted {
            return match self.targets.get(w) {
                Some(t) => Ok(Some((w.to_string(), t.clone()))),
                None => Err(format!("{}: no hay un perfil '{w}' ([target.{w}]); hay: {}", self.file.display(), names(self))),
            };
        }
        if let Some(d) = &self.default_target {
            return Ok(Some((d.clone(), self.targets[d].clone())));
        }
        match self.targets.len() {
            0 => Ok(None),
            1 => Ok(self.targets.iter().next().map(|(n, t)| (n.clone(), t.clone()))),
            _ => Err(format!("{}: hay varios perfiles ({}) y ninguno por defecto: elige uno con --target o fija default_target en [project]", self.file.display(), names(self))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn load(text: &str) -> Result<Project, String> {
        parse(Path::new("/p/pypic.toml"), text)
    }

    #[test]
    fn a_full_project_parses_with_paths_relative_to_the_file() {
        let p = load("[project]\nname = \"termometro\"\nmain = \"src/main.py\"\ndevices_dir = \"packs\"\ndefault_target = \"fino\"\n\n[target.fino]\ndevice = \"PIC16F887\"\nclock = \"Crystal[20 * MHz]\"\noutput = \"build/t.hex\"\nconfig = { WDTE = \"ON\", BOREN = 1, LVP = false }\n\n[target.otro]\ndevice = \"PIC16F886\"\n").unwrap();
        assert_eq!(p.name.as_deref(), Some("termometro"));
        assert_eq!(p.main, Some(PathBuf::from("/p/src/main.py")));
        assert_eq!(p.devices_dir, Some(PathBuf::from("/p/packs")));
        let (n, t) = p.target(None).unwrap().unwrap();
        assert_eq!(n, "fino");
        assert_eq!((t.device.as_deref(), t.clock.as_deref(), t.output), (Some("PIC16F887"), Some("Crystal[20 * MHz]"), Some(PathBuf::from("/p/build/t.hex"))));
        assert_eq!(t.config, vec![("BOREN".to_string(), "1".to_string()), ("LVP".to_string(), "OFF".to_string()), ("WDTE".to_string(), "ON".to_string())]);
        assert_eq!(p.target(Some("otro")).unwrap().unwrap().1.device.as_deref(), Some("PIC16F886"));
    }

    #[test]
    fn typos_and_wrong_types_are_errors_that_name_the_valid_keys() {
        for (text, frag) in [
            ("[proyecto]\n", "la seccion 'proyecto' no existe (admite: project, target)"),
            ("[project]\nmian = \"a.py\"\n", "la clave 'mian' no existe (admite: name, main, devices_dir, default_target)"),
            ("[target.a]\ndevce = \"PIC16F887\"\n", "[target.a] la clave 'devce' no existe (admite: device, clock, config, output)"),
            ("[target.a]\ndevice = 5\n", "'device' es un texto no vacio"),
            ("[target.a]\nconfig = \"WDTE=ON\"\n", "'config' es una tabla"),
            ("[target.a]\nconfig = { WDTE = [1] }\n", "config.WDTE es un texto o un numero"),
            ("[target]\na = 3\n", "[target.a] debe ser una tabla"),
            ("project = 3\n", "'project' debe ser una tabla"),
            ("[project]\ndefault_target = \"x\"\n", "no hay [target.x] (hay: ninguno)"),
            ("[project\n", "pypic.toml"),
        ] {
            let e = load(text).unwrap_err();
            assert!(e.contains(frag), "{text:?}: {e}");
        }
    }

    #[test]
    fn the_target_is_the_requested_one_the_default_or_the_only_one() {
        let two = load("[target.a]\ndevice = \"X\"\n[target.b]\ndevice = \"Y\"\n").unwrap();
        assert!(two.target(None).unwrap_err().contains("hay varios perfiles (a, b)"));
        assert_eq!(two.target(Some("b")).unwrap().unwrap().0, "b");
        assert!(two.target(Some("c")).unwrap_err().contains("no hay un perfil 'c'"));
        let one = load("[target.solo]\ndevice = \"X\"\n").unwrap();
        assert_eq!(one.target(None).unwrap().unwrap().0, "solo");
        assert!(load("[project]\nname = \"x\"\n").unwrap().target(None).unwrap().is_none());
    }
}
