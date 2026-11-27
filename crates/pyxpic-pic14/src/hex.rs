//! Imagen de memoria de programa y formato Intel HEX.
//!
//! Cada palabra de 14 bits ocupa 2 bytes (byte bajo primero) y la direccion en
//! bytes es el doble de la direccion de palabra, como hacen MPLAB y los
//! programadores PIC (p. ej. la palabra de configuracion en `2007h` queda en la
//! direccion de bytes `400Eh`). Todo corre en el host.

use std::collections::BTreeMap;
use std::fmt;

/// Mayor valor de una palabra de programa PIC14.
pub const WORD_MASK: u16 = 0x3FFF;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HexError {
    /// La palabra no cabe en 14 bits.
    WordTooWide { addr: u32, word: u16 },
    /// Dos escrituras a la misma direccion con valores distintos.
    Conflict { addr: u32 },
    /// Direccion fuera de la memoria de programa y de la zona de configuracion.
    OutOfBounds { addr: u32 },
    /// Linea mal formada al leer HEX (numero de linea, motivo).
    Parse { line: usize, reason: String },
}

impl fmt::Display for HexError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            HexError::WordTooWide { addr, word } => write!(f, "la palabra {word:#06x} en {addr:#06x} no cabe en 14 bits"),
            HexError::Conflict { addr } => write!(f, "dos valores distintos para la direccion {addr:#06x}"),
            HexError::OutOfBounds { addr } => write!(f, "la direccion {addr:#06x} esta fuera de la memoria del dispositivo"),
            HexError::Parse { line, reason } => write!(f, "HEX linea {line}: {reason}"),
        }
    }
}

impl std::error::Error for HexError {}

/// Palabras de 14 bits indexadas por direccion de palabra.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Image {
    words: BTreeMap<u32, u16>,
}

impl Image {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn len(&self) -> usize {
        self.words.len()
    }

    pub fn is_empty(&self) -> bool {
        self.words.is_empty()
    }

    pub fn get(&self, addr: u32) -> Option<u16> {
        self.words.get(&addr).copied()
    }

    pub fn iter(&self) -> impl Iterator<Item = (u32, u16)> + '_ {
        self.words.iter().map(|(&a, &w)| (a, w))
    }

    /// Escribe una palabra; error si no cabe en 14 bits o contradice otra ya escrita.
    pub fn set(&mut self, addr: u32, word: u16) -> Result<(), HexError> {
        if word > WORD_MASK {
            return Err(HexError::WordTooWide { addr, word });
        }
        match self.words.get(&addr) {
            Some(&prev) if prev != word => Err(HexError::Conflict { addr }), // sin modificar la imagen
            _ => {
                self.words.insert(addr, word);
                Ok(())
            }
        }
    }

    /// Escribe palabras consecutivas desde `origin`.
    pub fn extend(&mut self, origin: u32, words: &[u16]) -> Result<(), HexError> {
        for (i, &w) in words.iter().enumerate() {
            self.set(origin + i as u32, w)?;
        }
        Ok(())
    }

    /// Todas las direcciones deben caer en el programa `0..program_words` o en la
    /// zona de configuracion `config.0..=config.1`.
    pub fn check_bounds(&self, program_words: u32, config: (u32, u32)) -> Result<(), HexError> {
        for &addr in self.words.keys() {
            if addr >= program_words && !(config.0..=config.1).contains(&addr) {
                return Err(HexError::OutOfBounds { addr });
            }
        }
        Ok(())
    }

    /// Escribe el Intel HEX. `max_bytes` (par, >= 2) limita los datos por registro.
    /// Los huecos generan registros separados; se emiten registros de direccion
    /// lineal extendida (tipo 04) solo cuando la direccion de bytes pasa de 64 KiB.
    pub fn to_intel_hex(&self, max_bytes: usize) -> String {
        let max_bytes = (max_bytes.max(2)) & !1;
        let mut out = String::new();
        let mut upper: u32 = 0;
        let mut it = self.words.iter().peekable();
        while let Some((&start, &first)) = it.next() {
            // acumula una racha de palabras consecutivas
            let mut run = vec![first];
            while let Some((&a, &w)) = it.peek() {
                if a == start + run.len() as u32 {
                    run.push(w);
                    it.next();
                } else {
                    break;
                }
            }
            let mut byte_addr = start as u64 * 2;
            let bytes: Vec<u8> = run.iter().flat_map(|w| w.to_le_bytes()).collect();
            let mut pos = 0;
            while pos < bytes.len() {
                let seg_left = 0x1_0000 - (byte_addr & 0xFFFF) as usize; // no cruzar 64 KiB
                let n = (bytes.len() - pos).min(max_bytes).min(seg_left);
                let hi = (byte_addr >> 16) as u32;
                if hi != upper {
                    out.push_str(&record(0, 0x04, &(hi as u16).to_be_bytes()));
                    upper = hi;
                }
                out.push_str(&record((byte_addr & 0xFFFF) as u16, 0x00, &bytes[pos..pos + n]));
                pos += n;
                byte_addr += n as u64;
            }
        }
        out.push_str(&record(0, 0x01, &[]));
        out
    }

    /// Lee un archivo Intel HEX de palabras de 14 bits.
    pub fn from_intel_hex(text: &str) -> Result<Image, HexError> {
        let mut img = Image::new();
        let mut upper: u64 = 0;
        let mut seen_eof = false;
        for (i, raw) in text.lines().enumerate() {
            let line = i + 1;
            let l = raw.trim();
            if l.is_empty() {
                continue;
            }
            let err = |reason: &str| HexError::Parse { line, reason: reason.into() };
            if seen_eof {
                return Err(err("datos despues del registro de fin de archivo"));
            }
            let body = l.strip_prefix(':').ok_or_else(|| err("falta ':' inicial"))?;
            if body.len() % 2 != 0 || body.len() < 10 || !body.bytes().all(|b| b.is_ascii_hexdigit()) {
                return Err(err("longitud o caracteres no validos"));
            }
            let bytes: Vec<u8> = (0..body.len() / 2).map(|k| u8::from_str_radix(&body[2 * k..2 * k + 2], 16).unwrap()).collect();
            let len = bytes[0] as usize;
            if bytes.len() != len + 5 {
                return Err(err("la longitud declarada no coincide con los datos"));
            }
            if bytes.iter().fold(0u8, |s, b| s.wrapping_add(*b)) != 0 {
                return Err(err("checksum incorrecto"));
            }
            let addr = u16::from_be_bytes([bytes[1], bytes[2]]) as u64;
            let data = &bytes[4..4 + len];
            match bytes[3] {
                0x00 => {
                    let base = upper * 0x1_0000 + addr;
                    if !base.is_multiple_of(2) || !data.len().is_multiple_of(2) {
                        return Err(err("los datos deben alinearse a palabras de 2 bytes"));
                    }
                    for (k, pair) in data.chunks(2).enumerate() {
                        let word = u16::from_le_bytes([pair[0], pair[1]]);
                        img.set((base / 2) as u32 + k as u32, word).map_err(|e| err(&e.to_string()))?;
                    }
                }
                // el fin de archivo es exactamente `:00000001FF`: sin datos y con la direccion en cero
                0x01 if len != 0 => return Err(err("registro 01 (fin de archivo) con datos: la longitud debe ser 0")),
                0x01 if addr != 0 => return Err(err("registro 01 (fin de archivo) con direccion distinta de 0000")),
                0x01 => seen_eof = true,
                // direccion lineal extendida: dos bytes de datos y el campo de direccion en 0000
                0x04 if len != 2 => return Err(err("registro 04 con longitud distinta de 2")),
                0x04 if addr != 0 => return Err(err("registro 04 (direccion lineal extendida) con direccion distinta de 0000")),
                0x04 => upper = u16::from_be_bytes([data[0], data[1]]) as u64,
                t => return Err(err(&format!("tipo de registro {t:#04x} no soportado"))),
            }
        }
        if !seen_eof {
            return Err(HexError::Parse { line: text.lines().count(), reason: "falta el registro de fin de archivo (:00000001FF)".into() });
        }
        Ok(img)
    }
}

fn record(addr: u16, kind: u8, data: &[u8]) -> String {
    let mut bytes = vec![data.len() as u8, (addr >> 8) as u8, addr as u8, kind];
    bytes.extend_from_slice(data);
    let sum = bytes.iter().fold(0u8, |s, b| s.wrapping_add(*b));
    bytes.push(0u8.wrapping_sub(sum));
    let mut s = String::from(":");
    for b in bytes {
        s.push_str(&format!("{b:02X}"));
    }
    s.push('\n');
    s
}
