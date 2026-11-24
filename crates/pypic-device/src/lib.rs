//! Device Packs de PyPIC: esquema JSON versionado, cargador y validador.
//!
//! Un pack describe un MCU con datos verificados contra su datasheet oficial.
//! Directorio `devices/<mcu>/` con `device.json`, `memory.json`, `pins.json` y
//! `config.json` (los registros SFR y perifericos llegaran en packs posteriores).
//! Todo corre en el host; nada de esto va al MCU.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::Path;

use pypic_frontend::{PinInfo, TargetProfile};
use serde::{Deserialize, Deserializer};

/// Version del esquema que este crate sabe leer.
pub const SCHEMA_VERSION: u32 = 1;

/// Direccion/valor numerico: acepta `4096` o `"0x1000"` en el JSON.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Num(pub u32);

impl<'de> Deserialize<'de> for Num {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Raw {
            N(u32),
            S(String),
        }
        match Raw::deserialize(d)? {
            Raw::N(n) => Ok(Num(n)),
            Raw::S(s) => {
                let t = s.trim();
                let r = match t.strip_prefix("0x").or_else(|| t.strip_prefix("0X")) {
                    Some(h) => u32::from_str_radix(h, 16),
                    None => t.parse(),
                };
                r.map(Num).map_err(|_| serde::de::Error::custom(format!("numero invalido: '{s}'")))
            }
        }
    }
}

// ----------------------------------------------------------------- device.json
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeviceInfo {
    pub schema_version: u32,
    pub name: String,
    pub vendor: String,
    pub family: String,
    pub cpu: Cpu,
    pub io_pins: u32,
    pub peripherals: BTreeMap<String, u32>,
    pub pypic: PypicInfo,
    pub sources: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Cpu {
    pub core: String,
    /// Ancho de dato nativo (8 en PIC16, 16 en PIC24/dsPIC, 32 en PIC32).
    pub data_bits: u32,
    pub instruction_bits: u32,
    pub instruction_count: u32,
    pub max_clock_hz: u64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PypicInfo {
    /// experimental | compila | simula | probado en hardware | depuracion fisica
    pub support_level: String,
    /// Ancho maximo de entero admitido por PyPIC (nativo o emulado).
    pub max_int_bits: u32,
}

// ----------------------------------------------------------------- memory.json
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Memory {
    pub schema_version: u32,
    pub program: ProgramMemory,
    pub data: DataMemory,
    pub eeprom: Eeprom,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProgramMemory {
    pub words: u32,
    pub word_bits: u32,
    pub reset_vector: Num,
    pub interrupt_vector: Num,
    pub stack_levels: u32,
    pub source: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DataMemory {
    pub banks: u32,
    pub bank_size: u32,
    /// Rango de SFR de cada banco (en los PIC16F886/887 los bancos 2 y 3 tienen solo 16).
    pub sfr: Vec<Range>,
    pub bank_select: BankSelect,
    pub gpr: Vec<Range>,
    pub common: Common,
    pub ram_bytes: u32,
    pub source: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BankSelect {
    pub register: String,
    pub bits: Vec<String>,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Range {
    pub start: Num,
    pub end: Num,
}

impl Range {
    pub fn len(&self) -> u32 {
        self.end.0 + 1 - self.start.0
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Common {
    pub start: Num,
    pub end: Num,
    pub mirrors: Vec<Num>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Eeprom {
    pub bytes: u32,
}

// -------------------------------------------------------------------- pins.json
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Pins {
    pub schema_version: u32,
    pub packages: Vec<Package>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Package {
    pub name: String,
    pub pin_count: u32,
    pub source: String,
    pub pins: Vec<Pin>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Pin {
    pub number: u32,
    pub name: String,
    #[serde(default)]
    pub power: bool,
    pub port: Option<String>,
    pub bit: Option<u32>,
    #[serde(default)]
    pub input_only: bool,
    #[serde(default)]
    pub functions: Vec<String>,
}

// ------------------------------------------------------------------ config.json
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub schema_version: u32,
    pub source: String,
    pub word_bits: u32,
    pub space: ConfigSpace,
    pub words: Vec<ConfigWord>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConfigSpace {
    pub start: Num,
    pub end: Num,
    pub note: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConfigWord {
    pub name: String,
    pub address: Num,
    pub default: Num,
    pub fields: Vec<ConfigField>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConfigField {
    pub name: String,
    pub lsb: u32,
    pub width: u32,
    /// Valor (como texto decimal) -> significado.
    pub values: BTreeMap<String, String>,
}

// ------------------------------------------------------------------------ pack
#[derive(Debug, Clone)]
pub struct DevicePack {
    pub device: DeviceInfo,
    pub memory: Memory,
    pub pins: Pins,
    pub config: Config,
}

#[derive(Debug)]
pub struct LoadError(pub String);

impl fmt::Display for LoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

fn read<T: for<'de> Deserialize<'de>>(dir: &Path, file: &str) -> Result<T, LoadError> {
    let path = dir.join(file);
    let text = std::fs::read_to_string(&path).map_err(|e| LoadError(format!("{}: {e}", path.display())))?;
    serde_json::from_str(&text).map_err(|e| LoadError(format!("{}: {e}", path.display())))
}

impl DevicePack {
    pub fn load(dir: &Path) -> Result<DevicePack, LoadError> {
        Ok(DevicePack {
            device: read(dir, "device.json")?,
            memory: read(dir, "memory.json")?,
            pins: read(dir, "pins.json")?,
            config: read(dir, "config.json")?,
        })
    }

    /// Perfil que consume el frontend: ancho nativo y maximo del dispositivo.
    pub fn target_profile(&self) -> TargetProfile {
        let mut pins = BTreeMap::new();
        if let Some(pk) = self.pins.packages.first() {
            for p in &pk.pins {
                if let (Some(port), Some(bit)) = (&p.port, p.bit) {
                    let analog = p.functions.iter().find_map(|f| f.strip_prefix("AN").and_then(|n| n.parse::<u8>().ok()));
                    pins.insert(p.name.clone(), PinInfo { port: port.chars().next().unwrap_or('?'), bit: bit as u8, input_only: p.input_only, analog });
                }
            }
        }
        TargetProfile::new(self.device.name.clone(), self.device.cpu.data_bits, self.device.pypic.max_int_bits)
            .with_stack_levels(self.memory.program.stack_levels)
            .with_pins(pins)
    }

    /// Comprobaciones de coherencia interna. Vacio = pack valido.
    pub fn validate(&self) -> Vec<String> {
        let mut e = Vec::new();
        self.validate_schema(&mut e);
        self.validate_device(&mut e);
        self.validate_memory(&mut e);
        self.validate_pins(&mut e);
        self.validate_config(&mut e);
        e
    }

    fn validate_schema(&self, e: &mut Vec<String>) {
        for (file, v) in [
            ("device.json", self.device.schema_version),
            ("memory.json", self.memory.schema_version),
            ("pins.json", self.pins.schema_version),
            ("config.json", self.config.schema_version),
        ] {
            if v != SCHEMA_VERSION {
                e.push(format!("{file}: schema_version {v} no soportada (se espera {SCHEMA_VERSION})"));
            }
        }
    }

    fn validate_device(&self, e: &mut Vec<String>) {
        let d = &self.device;
        if d.name.trim().is_empty() {
            e.push("device.json: name vacio".into());
        }
        if !matches!(d.cpu.data_bits, 8 | 16 | 32) {
            e.push(format!("device.json: cpu.data_bits {} no es 8, 16 ni 32", d.cpu.data_bits));
        }
        if d.pypic.max_int_bits < d.cpu.data_bits || !matches!(d.pypic.max_int_bits, 8 | 16 | 32) {
            e.push(format!(
                "device.json: pypic.max_int_bits {} debe ser 8, 16 o 32 y no menor que cpu.data_bits {}",
                d.pypic.max_int_bits, d.cpu.data_bits
            ));
        }
        const LEVELS: [&str; 5] = ["experimental", "compila", "simula", "probado en hardware", "depuracion fisica"];
        if !LEVELS.contains(&d.pypic.support_level.as_str()) {
            e.push(format!("device.json: pypic.support_level '{}' desconocido", d.pypic.support_level));
        }
        if d.sources.is_empty() {
            e.push("device.json: falta al menos una fuente (datasheet)".into());
        }
        if d.cpu.instruction_bits != self.memory.program.word_bits {
            e.push("device.json/memory.json: instruction_bits y program.word_bits no coinciden".into());
        }
    }

    fn validate_memory(&self, e: &mut Vec<String>) {
        let m = &self.memory;
        let p = &m.program;
        if p.reset_vector.0 >= p.words || p.interrupt_vector.0 >= p.words {
            e.push("memory.json: un vector esta fuera de la memoria de programa".into());
        }
        if p.interrupt_vector.0 <= p.reset_vector.0 {
            e.push("memory.json: el vector de interrupcion debe ser posterior al de reset".into());
        }
        let d = &m.data;
        if d.sfr.len() as u32 != d.banks || d.bank_size == 0 {
            e.push("memory.json: debe haber un rango sfr por banco".into());
        } else {
            self.validate_bank_cover(e);
        }
        let mut total = 0;
        for r in &d.gpr {
            if r.end.0 < r.start.0 {
                e.push(format!("memory.json: rango GPR invertido {:#x}..{:#x}", r.start.0, r.end.0));
            } else {
                total += r.len();
            }
        }
        if total != d.ram_bytes {
            e.push(format!("memory.json: la suma de rangos GPR ({total}) no coincide con ram_bytes ({})", d.ram_bytes));
        }
        // la zona comun debe caer dentro de un GPR del banco 0 y sus espejos en los demas bancos
        let c = &d.common;
        let width = c.end.0.wrapping_sub(c.start.0).wrapping_add(1);
        if c.end.0 < c.start.0 || !d.gpr.iter().any(|r| r.start.0 <= c.start.0 && c.end.0 <= r.end.0) {
            e.push("memory.json: la zona comun no esta contenida en un rango GPR".into());
        }
        if c.mirrors.len() as u32 != d.banks.saturating_sub(1) {
            e.push("memory.json: debe haber un espejo de la zona comun por cada banco restante".into());
        }
        for (i, mir) in c.mirrors.iter().enumerate() {
            let expect = (i as u32 + 1) * d.bank_size + c.start.0;
            if mir.0 != expect || mir.0 + width > (i as u32 + 2) * d.bank_size {
                e.push(format!("memory.json: espejo {:#x} inconsistente (se esperaba {expect:#x})", mir.0));
            }
        }
        if m.eeprom.bytes != self.device.peripherals.get("eeprom_bytes").copied().unwrap_or(m.eeprom.bytes) {
            e.push("memory.json/device.json: eeprom.bytes no coincide con peripherals.eeprom_bytes".into());
        }
    }

    /// Cada banco debe quedar cubierto exactamente por SFR + GPR (+ espejo de la zona comun),
    /// sin huecos ni solapes.
    fn validate_bank_cover(&self, e: &mut Vec<String>) {
        let d = &self.memory.data;
        let c = &d.common;
        let common_len = c.end.0.wrapping_sub(c.start.0).wrapping_add(1);
        for b in 0..d.banks {
            let (base, top) = (b * d.bank_size, (b + 1) * d.bank_size - 1);
            let mut parts: Vec<(u32, u32, &str)> = Vec::new();
            let sfr = d.sfr[b as usize];
            parts.push((sfr.start.0, sfr.end.0, "SFR"));
            for r in d.gpr.iter().filter(|r| r.start.0 >= base && r.start.0 <= top) {
                parts.push((r.start.0, r.end.0, "GPR"));
            }
            if b > 0 {
                if let Some(m) = c.mirrors.get(b as usize - 1) {
                    parts.push((m.0, m.0 + common_len - 1, "espejo"));
                }
            }
            parts.sort();
            let mut next = base;
            for (s, en, what) in &parts {
                if *s != next || *en < *s || *en > top {
                    e.push(format!(
                        "memory.json: banco {b}: {what} {s:#05x}..{en:#05x} deja un hueco, se solapa o sale del banco (se esperaba desde {next:#05x})"
                    ));
                    break;
                }
                next = en + 1;
            }
            if next != top + 1 {
                e.push(format!("memory.json: banco {b} no queda cubierto por completo (llega hasta {:#05x})", next.wrapping_sub(1)));
            }
        }
    }

    fn validate_pins(&self, e: &mut Vec<String>) {
        if self.pins.packages.is_empty() {
            e.push("pins.json: sin encapsulados".into());
        }
        for pk in &self.pins.packages {
            let ctx = format!("pins.json[{}]", pk.name);
            if pk.pins.len() as u32 != pk.pin_count {
                e.push(format!("{ctx}: pin_count {} pero hay {} pines", pk.pin_count, pk.pins.len()));
            }
            let mut numbers = BTreeSet::new();
            let mut port_bits = BTreeSet::new();
            let mut io = 0;
            for p in &pk.pins {
                if p.number == 0 || p.number > pk.pin_count || !numbers.insert(p.number) {
                    e.push(format!("{ctx}: numero de pin invalido o repetido ({})", p.number));
                }
                match (&p.port, p.bit, p.power) {
                    (Some(port), Some(bit), false) => {
                        if bit > 7 || p.name != format!("R{port}{bit}") {
                            e.push(format!("{ctx}: pin {} '{}' inconsistente con port/bit", p.number, p.name));
                        }
                        if !port_bits.insert((port.clone(), bit)) {
                            e.push(format!("{ctx}: {} repetido", p.name));
                        }
                        if !p.input_only {
                            io += 1;
                        }
                    }
                    (None, None, true) => {
                        if !matches!(p.name.as_str(), "VDD" | "VSS") {
                            e.push(format!("{ctx}: pin de alimentacion desconocido '{}'", p.name));
                        }
                    }
                    _ => e.push(format!("{ctx}: pin {} debe ser de E/S (port+bit) o de alimentacion", p.number)),
                }
            }
            if io != self.device.io_pins {
                e.push(format!("{ctx}: {io} pines de E/S pero device.json declara {}", self.device.io_pins));
            }
            for rail in ["VDD", "VSS"] {
                if !pk.pins.iter().any(|p| p.name == rail) {
                    e.push(format!("{ctx}: falta {rail}"));
                }
            }
        }
    }

    fn validate_config(&self, e: &mut Vec<String>) {
        let c = &self.config;
        let mask = (1u64 << c.word_bits) - 1;
        let mut addrs = BTreeSet::new();
        for w in &c.words {
            let ctx = format!("config.json[{}]", w.name);
            if w.address.0 < c.space.start.0 || w.address.0 > c.space.end.0 {
                e.push(format!("{ctx}: direccion {:#x} fuera del espacio de configuracion", w.address.0));
            }
            if !addrs.insert(w.address) {
                e.push(format!("{ctx}: direccion repetida"));
            }
            if w.default.0 as u64 > mask {
                e.push(format!("{ctx}: default {:#x} no cabe en {} bits", w.default.0, c.word_bits));
            }
            let mut used = 0u64;
            for f in &w.fields {
                if f.width == 0 || f.lsb + f.width > c.word_bits {
                    e.push(format!("{ctx}.{}: bits {}..{} fuera de la palabra", f.name, f.lsb, f.lsb + f.width));
                    continue;
                }
                let fm = ((1u64 << f.width) - 1) << f.lsb;
                if used & fm != 0 {
                    e.push(format!("{ctx}.{}: se solapa con otro campo", f.name));
                }
                used |= fm;
                let mut seen = BTreeSet::new();
                for k in f.values.keys() {
                    match k.parse::<u64>() {
                        Ok(v) if v < (1 << f.width) => {
                            seen.insert(v);
                        }
                        _ => e.push(format!("{ctx}.{}: valor '{k}' no cabe en {} bit(s)", f.name, f.width)),
                    }
                }
                if seen.len() as u64 != 1 << f.width {
                    e.push(format!("{ctx}.{}: faltan valores (hay {} de {})", f.name, seen.len(), 1u64 << f.width));
                }
            }
        }
    }
}
