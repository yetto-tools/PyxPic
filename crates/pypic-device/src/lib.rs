//! Device Packs de PyPIC: esquema JSON versionado, cargador y validador.
//!
//! Un pack describe un MCU con datos verificados contra su datasheet oficial.
//! Directorio `devices/<mcu>/` con `device.json`, `memory.json`, `pins.json` y
//! `config.json`, `registers.json` y `peripherals.json`.
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
    /// Relojes del oscilador por ciclo de instruccion (4 en PIC16/PIC18; DS40001291H p.1: 20 MHz y 200 ns).
    pub clocks_per_instruction: u32,
    /// Reloj tras el reset con la configuracion por defecto (PIC16F887: OSCCON<IRCF> = 110 = 4 MHz, DS40001291H p.64).
    pub default_clock_hz: u32,
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

// ---------------------------------------------------------------- registers.json
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Registers {
    pub schema_version: u32,
    pub source: String,
    /// Direcciones dentro de la zona SFR que no tienen registro.
    pub unimplemented: Vec<Num>,
    pub registers: Vec<Register>,
}

fn yes() -> bool {
    true
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Register {
    pub name: String,
    /// Todas las direcciones donde aparece (un registro puede verse en varios bancos).
    pub addresses: Vec<Num>,
    /// Nombre de cada bit, del 0 al 7; `null` si no tiene nombre o no esta implementado.
    pub bits: Vec<Option<String>>,
    /// Valor tras POR/BOR tal como lo escribe el datasheet (`x` desconocido, `-` no implementado, `q` depende).
    pub por: String,
    pub page: u32,
    /// `false` en INDF y EECON2: no son registros fisicos.
    #[serde(default = "yes")]
    pub physical: bool,
    /// Condicion bajo la que esta direccion es este registro y no otro (SSPMSK frente a SSPADD).
    pub selected_by: Option<String>,
    pub note: Option<String>,
}

impl Register {
    /// Desplazamiento de 7 bits de la primera direccion: lo que codifican las instrucciones `f`.
    pub fn offset(&self) -> u8 {
        (self.addresses[0].0 & 0x7F) as u8
    }
}

impl Registers {
    pub fn parse(json: &str) -> Result<Registers, LoadError> {
        serde_json::from_str(json).map_err(|e| LoadError(format!("registers.json: {e}")))
    }

    pub fn get(&self, name: &str) -> Option<&Register> {
        self.registers.iter().find(|r| r.name == name)
    }

    /// Simbolos del ensamblador: cada registro vale su desplazamiento de 7 bits (el banco lo elige el codigo
    /// con RP0/RP1) y cada bit con nombre su numero de bit. Si un nombre de bit se repite o coincide con el
    /// de un registro, se omite.
    pub fn asm_symbols(&self) -> Vec<(String, u8)> {
        let mut out: Vec<(String, u8)> = Vec::new();
        let mut seen: BTreeSet<&str> = BTreeSet::new();
        for r in &self.registers {
            if seen.insert(&r.name) {
                out.push((r.name.clone(), r.offset()));
            }
        }
        for r in &self.registers {
            for (i, b) in r.bits.iter().enumerate() {
                if let Some(b) = b {
                    if seen.insert(b) {
                        out.push((b.clone(), i as u8));
                    }
                }
            }
        }
        out
    }
}

// -------------------------------------------------------------- peripherals.json
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Peripherals {
    pub schema_version: u32,
    pub source: String,
    pub peripherals: Vec<Peripheral>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Peripheral {
    pub name: String,
    /// `timer`, `analog`, `memory`, `ccp`, `serial`, `system` o `interrupt`.
    pub kind: String,
    pub description: String,
    pub page: u32,
    /// Ancho del contador o del resultado, si tiene sentido.
    pub bits: Option<u32>,
    /// Registros que lo controlan (nombres de `registers.json`).
    pub registers: Vec<String>,
    /// Bits que lo encienden (uno por cada instancia: los comparadores tienen dos).
    #[serde(default)]
    pub enable: Vec<String>,
    #[serde(default)]
    pub interrupts: Vec<Interrupt>,
    #[serde(default)]
    pub pins: Vec<PeripheralPin>,
    /// Otros perifericos de los que depende (los CCP usan Timer1 y Timer2).
    #[serde(default)]
    pub uses: Vec<String>,
    /// Modos que se excluyen entre si y reparten sus pines (MSSP: SPI o I2C).
    #[serde(default)]
    pub modes: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Interrupt {
    /// Bit de bandera (PIR1.TMR1IF...) y bit de habilitacion (PIE1.TMR1IE...), por nombre.
    pub flag: String,
    pub enable: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PeripheralPin {
    /// Funcion alterna tal como figura en `pins.json` (`T1CKI`, `SCK`...).
    pub function: String,
    /// `in`, `out`, `io` o `analog`.
    pub direction: String,
    pub mode: Option<String>,
}

impl Peripherals {
    pub fn get(&self, name: &str) -> Option<&Peripheral> {
        self.peripherals.iter().find(|p| p.name == name)
    }

    /// Periferico que posee una funcion alterna de pin.
    pub fn owner_of(&self, function: &str) -> Option<&Peripheral> {
        self.peripherals.iter().find(|p| p.pins.iter().any(|x| x.function == function))
    }
}

// ------------------------------------------------------------------------ pack
#[derive(Debug, Clone)]
pub struct DevicePack {
    pub device: DeviceInfo,
    pub memory: Memory,
    pub pins: Pins,
    pub config: Config,
    pub registers: Registers,
    pub peripherals: Peripherals,
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
            registers: read(dir, "registers.json")?,
            peripherals: read(dir, "peripherals.json")?,
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
            .with_peripherals(self.peripherals.peripherals.iter().map(|p| p.name.clone()))
            .with_pins(pins)
            .with_clocks_per_cycle(self.device.cpu.clocks_per_instruction)
            .with_default_clock(self.device.cpu.default_clock_hz)
            .with_device(self.device.name.clone(), self.device.cpu.max_clock_hz.min(u32::MAX as u64) as u32)
    }

    /// Funciones alternas de un pin y el periferico que las posee: `(funcion, periferico)`.
    /// Un pin usado por un periferico no puede ser a la vez E/S digital del programa.
    pub fn pin_uses(&self, pin: &str) -> Vec<(String, String)> {
        let Some(pk) = self.pins.packages.first() else { return Vec::new() };
        let Some(p) = pk.pins.iter().find(|p| p.name == pin) else { return Vec::new() };
        p.functions.iter().filter_map(|f| self.peripherals.owner_of(f).map(|o| (f.clone(), o.name.clone()))).collect()
    }

    /// Comprobaciones de coherencia interna. Vacio = pack valido.
    pub fn validate(&self) -> Vec<String> {
        let mut e = Vec::new();
        self.validate_schema(&mut e);
        self.validate_device(&mut e);
        self.validate_memory(&mut e);
        self.validate_pins(&mut e);
        self.validate_config(&mut e);
        self.validate_registers(&mut e);
        self.validate_peripherals(&mut e);
        e
    }

    fn validate_schema(&self, e: &mut Vec<String>) {
        for (file, v) in [
            ("device.json", self.device.schema_version),
            ("memory.json", self.memory.schema_version),
            ("pins.json", self.pins.schema_version),
            ("config.json", self.config.schema_version),
            ("registers.json", self.registers.schema_version),
            ("peripherals.json", self.peripherals.schema_version),
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
        if d.cpu.default_clock_hz == 0 || d.cpu.default_clock_hz as u64 > d.cpu.max_clock_hz {
            e.push("device.json: cpu.default_clock_hz debe ser > 0 y no mayor que max_clock_hz".into());
        }
        if d.cpu.clocks_per_instruction == 0 {
            e.push("device.json: cpu.clocks_per_instruction debe ser >= 1".into());
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

    fn validate_registers(&self, e: &mut Vec<String>) {
        let rg = &self.registers;
        let ident = |s: &str| !s.is_empty() && !s.starts_with(|c: char| c.is_ascii_digit()) && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
        let in_sfr = |a: u32| self.memory.data.sfr.iter().enumerate().any(|(b, r)| a >= r.start.0 && a <= r.end.0 && a / self.memory.data.bank_size == b as u32);
        let mut names: BTreeSet<&str> = BTreeSet::new();
        let mut by_addr: BTreeMap<u32, Vec<&Register>> = BTreeMap::new();
        for r in &rg.registers {
            let ctx = format!("registers.json: {}", r.name);
            if !ident(&r.name) || !names.insert(&r.name) {
                e.push(format!("{ctx}: nombre invalido o repetido"));
            }
            if r.addresses.is_empty() {
                e.push(format!("{ctx}: sin direcciones"));
            }
            for a in &r.addresses {
                if !in_sfr(a.0) {
                    e.push(format!("{ctx}: la direccion {:#05x} no esta en la zona SFR de su banco (memory.json)", a.0));
                }
                by_addr.entry(a.0).or_default().push(r);
            }
            let mut sorted: Vec<u32> = r.addresses.iter().map(|a| a.0).collect();
            sorted.sort_unstable();
            sorted.dedup();
            if sorted.len() != r.addresses.len() {
                e.push(format!("{ctx}: direcciones repetidas"));
            }
            if r.addresses.iter().any(|a| (a.0 & 0x7F) != (r.addresses[0].0 & 0x7F)) {
                e.push(format!("{ctx}: las direcciones de un mismo registro deben tener el mismo desplazamiento dentro del banco"));
            }
            if r.bits.len() != 8 {
                e.push(format!("{ctx}: 'bits' debe tener 8 entradas (bit 0 a bit 7), tiene {}", r.bits.len()));
            }
            let ok_por = r.por.len() == 9 && r.por.char_indices().all(|(i, c)| if i == 4 { c == ' ' } else { "01xuq-".contains(c) });
            if !ok_por {
                e.push(format!("{ctx}: valor de reset '{}' invalido (se espera 'xxxx xxxx' con 0,1,x,u,q,-)", r.por));
            }
            if r.page == 0 {
                e.push(format!("{ctx}: falta la pagina del datasheet"));
            }
        }
        // un nombre de bit identifica a un solo bit y no puede ser el nombre de un registro
        let mut bit_names: BTreeSet<&str> = BTreeSet::new();
        for r in &rg.registers {
            for (i, b) in r.bits.iter().enumerate() {
                if let Some(b) = b {
                    if !ident(b) {
                        e.push(format!("registers.json: {}: bit {i} tiene un nombre invalido '{b}'", r.name));
                    } else if names.contains(b.as_str()) {
                        e.push(format!("registers.json: el bit {}.{b} tiene el nombre de un registro", r.name));
                    } else if !bit_names.insert(b) {
                        e.push(format!("registers.json: el nombre de bit '{b}' esta repetido"));
                    }
                }
            }
        }
        // una direccion pertenece a un solo registro, salvo un par donde exactamente uno dice cuando se selecciona
        for (a, rs) in &by_addr {
            if rs.len() > 1 && rs.iter().filter(|r| r.selected_by.is_none()).count() != 1 {
                e.push(format!("registers.json: la direccion {a:#05x} la usan {} registros sin condicion de seleccion", rs.len()));
            }
        }
        // cobertura: cada direccion SFR tiene registro o esta declarada sin implementar
        let unimpl: BTreeSet<u32> = rg.unimplemented.iter().map(|n| n.0).collect();
        for a in &unimpl {
            if !in_sfr(*a) {
                e.push(format!("registers.json: sin implementar {a:#05x} esta fuera de la zona SFR"));
            }
            if by_addr.contains_key(a) {
                e.push(format!("registers.json: {a:#05x} esta declarada sin implementar y tambien tiene registro"));
            }
        }
        for r in &self.memory.data.sfr {
            for a in r.start.0..=r.end.0 {
                if !by_addr.contains_key(&a) && !unimpl.contains(&a) {
                    e.push(format!("registers.json: la direccion SFR {a:#05x} no tiene registro ni esta declarada sin implementar"));
                }
            }
        }
        // referencias cruzadas con memory.json y pins.json
        let bs = &self.memory.data.bank_select;
        match rg.get(&bs.register) {
            None => e.push(format!("registers.json: falta el registro de seleccion de banco {}", bs.register)),
            Some(r) => {
                for b in &bs.bits {
                    if !r.bits.iter().any(|x| x.as_deref() == Some(b.as_str())) {
                        e.push(format!("registers.json: {} no tiene el bit de seleccion de banco {b}", bs.register));
                    }
                }
            }
        }
        for pk in &self.pins.packages {
            for p in &pk.pins {
                if let Some(port) = &p.port {
                    for prefix in ["PORT", "TRIS"] {
                        if rg.get(&format!("{prefix}{port}")).is_none() {
                            e.push(format!("registers.json: falta {prefix}{port} (lo usa el pin {})", p.name));
                        }
                    }
                    let bit_ok = |reg: &str| rg.get(&format!("{reg}{port}")).is_some_and(|r| p.bit.is_some_and(|b| r.bits.get(b as usize).is_some_and(|x| x.as_deref() == Some(format!("{reg}{port}{b}").as_str()) || reg == "PORT" && x.as_deref() == Some(p.name.as_str()))));
                    if !bit_ok("PORT") || !bit_ok("TRIS") {
                        e.push(format!("registers.json: el pin {} no tiene su bit con nombre en PORT{port}/TRIS{port}", p.name));
                    }
                }
                for f in &p.functions {
                    if let Some(n) = f.strip_prefix("AN").and_then(|n| n.parse::<u32>().ok()) {
                        let reg = if n < 8 { "ANSEL" } else { "ANSELH" };
                        if !rg.get(reg).is_some_and(|r| r.bits.iter().any(|x| x.as_deref() == Some(format!("ANS{n}").as_str()))) {
                            e.push(format!("registers.json: el pin {} usa {f} pero {reg} no tiene el bit ANS{n}", p.name));
                        }
                    }
                }
            }
        }
    }

    fn validate_peripherals(&self, e: &mut Vec<String>) {
        const KINDS: [&str; 7] = ["timer", "analog", "memory", "ccp", "serial", "system", "interrupt"];
        const DIRS: [&str; 4] = ["in", "out", "io", "analog"];
        let pr = &self.peripherals;
        let rg = &self.registers;
        let has_bit = |name: &str| rg.registers.iter().any(|r| r.bits.iter().any(|b| b.as_deref() == Some(name)));
        let mut names: BTreeSet<&str> = BTreeSet::new();
        let mut claimed: BTreeMap<&str, &str> = BTreeMap::new();
        for p in &pr.peripherals {
            let ctx = format!("peripherals.json: {}", p.name);
            if p.name.is_empty() || !names.insert(&p.name) {
                e.push(format!("{ctx}: nombre vacio o repetido"));
            }
            if !KINDS.contains(&p.kind.as_str()) {
                e.push(format!("{ctx}: tipo '{}' desconocido", p.kind));
            }
            if p.page == 0 {
                e.push(format!("{ctx}: falta la pagina del datasheet"));
            }
            for r in &p.registers {
                if rg.get(r).is_none() {
                    e.push(format!("{ctx}: el registro {r} no existe en registers.json"));
                }
            }
            for b in &p.enable {
                if !has_bit(b) {
                    e.push(format!("{ctx}: el bit de habilitacion {b} no existe en registers.json"));
                }
            }
            for i in &p.interrupts {
                for b in [&i.flag, &i.enable] {
                    if !has_bit(b) {
                        e.push(format!("{ctx}: el bit de interrupcion {b} no existe en registers.json"));
                    }
                }
            }
            for x in &p.pins {
                if !DIRS.contains(&x.direction.as_str()) {
                    e.push(format!("{ctx}: direccion '{}' invalida en {}", x.direction, x.function));
                }
                if let Some(m) = &x.mode {
                    if !p.modes.contains(m) {
                        e.push(format!("{ctx}: el pin {} usa el modo '{m}' que el periferico no declara", x.function));
                    }
                }
                if let Some(other) = claimed.insert(&x.function, &p.name) {
                    e.push(format!("{ctx}: la funcion {} ya la posee {other}", x.function));
                }
            }
        }
        for p in &pr.peripherals {
            for u in &p.uses {
                if !names.contains(u.as_str()) {
                    e.push(format!("peripherals.json: {} usa {u}, que no existe", p.name));
                }
            }
        }
        // cada funcion alterna de pins.json la posee un periferico y viceversa
        let mut functions: BTreeSet<&str> = BTreeSet::new();
        for pk in &self.pins.packages {
            for p in &pk.pins {
                for f in &p.functions {
                    functions.insert(f);
                    if !claimed.contains_key(f.as_str()) {
                        e.push(format!("peripherals.json: la funcion {f} del pin {} no la posee ningun periferico", p.name));
                    }
                }
            }
        }
        for f in claimed.keys() {
            if !functions.contains(f) {
                e.push(format!("peripherals.json: la funcion {f} no esta en ningun pin de pins.json"));
            }
        }
        // coherencia con las cantidades de device.json
        let d = &self.device.peripherals;
        let want = |k: &str| d.get(k).copied().unwrap_or(0);
        let count = |kind: &str, bits: u32| pr.peripherals.iter().filter(|p| p.kind == kind && p.bits == Some(bits)).count() as u32;
        if count("timer", 8) != want("timers_8bit") || count("timer", 16) != want("timers_16bit") {
            e.push("peripherals.json/device.json: la cantidad de temporizadores no coincide".into());
        }
        let adc_pins = pr.get("ADC").map_or(0, |a| a.pins.iter().filter(|x| x.function.starts_with("AN")).count() as u32);
        if adc_pins != want("adc_channels") || pr.get("ADC").and_then(|a| a.bits) != Some(want("adc_bits")) {
            e.push("peripherals.json/device.json: canales o bits del ADC no coinciden".into());
        }
        if pr.get("COMPARATORS").map_or(0, |c| c.enable.len() as u32) != want("comparators") {
            e.push("peripherals.json/device.json: la cantidad de comparadores no coincide".into());
        }
        let n = |name: &str| u32::from(pr.get(name).is_some());
        if n("ECCP") != want("eccp") || n("CCP2") != want("ccp") || n("EUSART") != want("eusart") || n("MSSP") != want("mssp") {
            e.push("peripherals.json/device.json: CCP, ECCP, EUSART o MSSP no coinciden con device.json".into());
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
