//! `pypic monitor`: la terminal serial de PyPIC (docs/monitor-serial.md, #72).
//!
//! Lee un puerto serie (o la salida por UART de un programa en el simulador) y la muestra como texto, hexadecimal o JSON por lineas, con marcas de tiempo, filtro y
//! registro en archivo; lo que se escribe en la entrada estandar se envia al puerto. El nucleo (decodificar, partir en lineas, filtrar, marcar el tiempo) no sabe de
//! puertos: trabaja sobre cualquier `Read`, asi que se prueba sin hardware.

use std::fs::OpenOptions;
use std::io::{BufRead, ErrorKind, Read, Write};
use std::process::ExitCode;
use std::time::{Duration, Instant};

use regex::Regex;
use serde_json::{json, Value};

/// La version del formato `--format jsonl` y `--list --format json`: sube si cambia el significado de un campo; agregar campos no la cambia.
pub const JSON_VERSION: u32 = 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Encoding {
    Ascii,
    Utf8,
    Hex,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Eol {
    None,
    Cr,
    Lf,
    CrLf,
}

impl Eol {
    fn bytes(self) -> &'static [u8] {
        match self {
            Eol::None => b"",
            Eol::Cr => b"\r",
            Eol::Lf => b"\n",
            Eol::CrLf => b"\r\n",
        }
    }
}

// ---------------------------------------------------------------------------------------------------------- el nucleo (sin puertos)

/// `[MM:SS.mmm]` desde que empezo la sesion (no la hora del reloj: no depende de la zona horaria y se puede probar).
pub fn timestamp(ms: u64) -> String {
    format!("[{:02}:{:02}.{:03}]", ms / 60_000, (ms / 1000) % 60, ms % 1000)
}

/// `48 6F 6C` en mayusculas.
pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02X}")).collect::<Vec<_>>().join(" ")
}

/// ASCII visible: lo que no se imprime (salvo el tabulador) sale como `\xNN`; un byte sobre 127 tambien.
pub fn ascii(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|&b| match b {
            b'\t' | 0x20..=0x7E => (b as char).to_string(),
            _ => format!("\\x{b:02X}"),
        })
        .collect()
}

pub struct Settings {
    pub encoding: Encoding,
    pub timestamps: bool,
    pub filter: Option<Regex>,
}

/// Parte lo que llega en lineas completas y les da formato; lo que falta de una linea se guarda hasta que llegue su `\n` (o se vuelve a pedir con `flush`).
pub struct Formatter<'a> {
    settings: &'a Settings,
    partial: Vec<u8>,
}

impl<'a> Formatter<'a> {
    pub fn new(settings: &'a Settings) -> Self {
        Formatter { settings, partial: Vec::new() }
    }

    fn line(&self, text: &str, t_ms: u64) -> Option<String> {
        if self.settings.filter.as_ref().is_some_and(|f| !f.is_match(text)) {
            return None;
        }
        Some(if self.settings.timestamps { format!("{} {text}", timestamp(t_ms)) } else { text.to_string() })
    }

    fn decode(&self, bytes: &[u8]) -> String {
        match self.settings.encoding {
            Encoding::Utf8 => String::from_utf8_lossy(bytes).into_owned(),
            _ => ascii(bytes),
        }
    }

    /// Lo recien recibido: devuelve las lineas listas para imprimir. En hexadecimal cada tramo recibido es una linea.
    pub fn feed(&mut self, bytes: &[u8], t_ms: u64) -> Vec<String> {
        if self.settings.encoding == Encoding::Hex {
            return self.line(&hex(bytes), t_ms).into_iter().collect();
        }
        self.partial.extend_from_slice(bytes);
        let mut out = Vec::new();
        while let Some(pos) = self.partial.iter().position(|&b| b == b'\n') {
            let mut raw: Vec<u8> = self.partial.drain(..=pos).collect();
            raw.pop();
            if raw.last() == Some(&b'\r') {
                raw.pop();
            }
            let text = self.decode(&raw);
            out.extend(self.line(&text, t_ms));
        }
        out
    }

    /// Una linea a medias que no termina (un `>` de espera sin salto de linea): se muestra tal cual.
    pub fn flush(&mut self, t_ms: u64) -> Vec<String> {
        if self.partial.is_empty() {
            return Vec::new();
        }
        let raw = std::mem::take(&mut self.partial);
        let text = self.decode(&raw);
        self.line(&text, t_ms).into_iter().collect()
    }
}

/// El evento `data` del formato JSON por lineas: los bytes en hexadecimal y su texto (con `\xNN` en lo que no se imprime).
pub fn data_event(bytes: &[u8], t_ms: u64) -> Value {
    json!({ "version": JSON_VERSION, "type": "data", "t_ms": t_ms, "hex": hex(bytes), "text": ascii(bytes), "bytes": bytes.len() })
}

/// Como terminó la lectura.
#[derive(Debug, PartialEq, Eq)]
pub enum End {
    /// El origen se acabó (la simulación terminó o se cerró la entrada).
    Eof,
    /// El dispositivo se desconecto o fallo la lectura.
    Disconnected(String),
}

/// Lee hasta el final: cada tramo va al formateador (texto) o a `emit` como evento (JSON por lineas). `now` da los milisegundos desde que empezó la sesión.
pub fn pump<R: Read>(reader: &mut R, settings: &Settings, jsonl: bool, now: &dyn Fn() -> u64, emit: &mut dyn FnMut(&str)) -> End {
    let mut fmt = Formatter::new(settings);
    let mut buf = [0u8; 1024];
    let mut last_data = now();
    loop {
        match reader.read(&mut buf) {
            Ok(0) => {
                for l in fmt.flush(now()) {
                    emit(&l);
                }
                return End::Eof;
            }
            Ok(n) => {
                last_data = now();
                if jsonl {
                    emit(&data_event(&buf[..n], last_data).to_string());
                } else {
                    for l in fmt.feed(&buf[..n], last_data) {
                        emit(&l);
                    }
                }
            }
            Err(e) if matches!(e.kind(), ErrorKind::TimedOut | ErrorKind::WouldBlock | ErrorKind::Interrupted) => {
                // sin datos: una linea que lleva rato sin cerrarse se muestra
                if !jsonl && now().saturating_sub(last_data) >= 250 {
                    for l in fmt.flush(now()) {
                        emit(&l);
                    }
                }
            }
            Err(e) => {
                for l in fmt.flush(now()) {
                    emit(&l);
                }
                return End::Disconnected(e.to_string());
            }
        }
    }
}

/// Interpreta `48 65 6C` (o `486C`, con o sin `0x`) como bytes; un error dice cual fragmento no es hexadecimal.
pub fn parse_hex(text: &str) -> Result<Vec<u8>, String> {
    let digits: String = text.split_whitespace().map(|w| w.trim_start_matches("0x").trim_start_matches("0X")).collect();
    if !digits.len().is_multiple_of(2) {
        return Err(format!("'{text}' no es hexadecimal valido: faltan cifras (cada byte son dos)"));
    }
    (0..digits.len()).step_by(2).map(|i| u8::from_str_radix(&digits[i..i + 2], 16).map_err(|_| format!("'{}' no es hexadecimal", &digits[i..i + 2]))).collect()
}

// ------------------------------------------------------------------------------------------------------------------ las opciones

#[derive(Debug)]
pub struct Options {
    pub list: bool,
    pub port: Option<String>,
    pub baud: u32,
    pub data_bits: u8,
    pub parity: String,
    pub stop_bits: u8,
    pub flow: String,
    pub encoding: Encoding,
    pub timestamps: bool,
    pub log: Option<String>,
    pub filter: Option<String>,
    pub jsonl: bool,
    pub list_json: bool,
    pub reconnect: bool,
    pub eol: Eol,
    pub send_hex: bool,
    pub send_file: Option<String>,
    pub sim: Option<String>,
    pub seconds: u64,
    /// Opciones de `pypic build` que `--sim` reenvia (`--devices-dir`, `--project`, `--target`...).
    pub build_args: Vec<String>,
}

pub fn parse_args(args: &[String]) -> Result<Options, String> {
    let mut o = Options {
        list: false,
        port: None,
        baud: 9600,
        data_bits: 8,
        parity: "none".into(),
        stop_bits: 1,
        flow: "none".into(),
        encoding: Encoding::Ascii,
        timestamps: false,
        log: None,
        filter: None,
        jsonl: false,
        list_json: false,
        reconnect: false,
        eol: Eol::Lf,
        send_hex: false,
        send_file: None,
        sim: None,
        seconds: 5,
        build_args: Vec::new(),
    };
    let mut it = args.iter();
    let value = |name: &str, it: &mut std::slice::Iter<String>| it.next().cloned().ok_or_else(|| format!("{name} necesita un valor"));
    while let Some(a) = it.next() {
        match a.as_str() {
            "--list" => o.list = true,
            "--port" => o.port = Some(value(a, &mut it)?),
            "--baud" => o.baud = value(a, &mut it)?.parse().map_err(|_| "--baud es un numero de baudios (9600, 115200...)".to_string())?,
            "--data-bits" => o.data_bits = value(a, &mut it)?.parse().ok().filter(|b| (5..=8).contains(b)).ok_or("--data-bits es 5, 6, 7 u 8")?,
            "--parity" => {
                o.parity = value(a, &mut it)?.to_ascii_lowercase();
                if !matches!(o.parity.as_str(), "none" | "even" | "odd") {
                    return Err("--parity es none, even u odd".into());
                }
            }
            "--stop-bits" => o.stop_bits = value(a, &mut it)?.parse().ok().filter(|b| (1..=2).contains(b)).ok_or("--stop-bits es 1 o 2")?,
            "--flow" => {
                o.flow = value(a, &mut it)?.to_ascii_lowercase();
                if !matches!(o.flow.as_str(), "none" | "rts" | "xon") {
                    return Err("--flow es none, rts (RTS/CTS) o xon (XON/XOFF)".into());
                }
            }
            "--encoding" => {
                o.encoding = match value(a, &mut it)?.to_ascii_lowercase().as_str() {
                    "ascii" => Encoding::Ascii,
                    "utf8" | "utf-8" => Encoding::Utf8,
                    "hex" => Encoding::Hex,
                    _ => return Err("--encoding es ascii, utf8 o hex".into()),
                }
            }
            "--timestamps" => o.timestamps = true,
            "--log" => o.log = Some(value(a, &mut it)?),
            "--filter" => o.filter = Some(value(a, &mut it)?),
            "--reconnect" => o.reconnect = true,
            "--eol" => {
                o.eol = match value(a, &mut it)?.to_ascii_lowercase().as_str() {
                    "none" => Eol::None,
                    "cr" => Eol::Cr,
                    "lf" => Eol::Lf,
                    "crlf" => Eol::CrLf,
                    _ => return Err("--eol es none, cr, lf o crlf".into()),
                }
            }
            "--send-encoding" => {
                o.send_hex = match value(a, &mut it)?.to_ascii_lowercase().as_str() {
                    "ascii" | "utf8" => false,
                    "hex" => true,
                    _ => return Err("--send-encoding es ascii o hex".into()),
                }
            }
            "--send-file" => o.send_file = Some(value(a, &mut it)?),
            "--sim" => o.sim = Some(value(a, &mut it)?),
            "--seconds" => o.seconds = value(a, &mut it)?.parse().ok().filter(|s| *s > 0).ok_or("--seconds es un numero de segundos mayor que cero")?,
            "--format" => match value(a, &mut it)?.as_str() {
                "jsonl" | "json" => {
                    o.jsonl = true;
                    o.list_json = true;
                }
                "text" => {
                    o.jsonl = false;
                    o.list_json = false;
                }
                _ => return Err("--format es text, jsonl (o json con --list)".into()),
            },
            "--devices-dir" | "--project" | "--target" | "--device" | "--config" => {
                o.build_args.push(a.clone());
                o.build_args.push(value(a, &mut it)?);
            }
            "--no-opt" => o.build_args.push(a.clone()),
            other if other.starts_with('-') => return Err(format!("la opcion '{other}' no existe (ver pypic monitor sin argumentos)")),
            other => {
                if o.sim.is_none() {
                    // un archivo suelto es lo mismo que --sim archivo
                    o.sim = Some(other.to_string());
                } else {
                    return Err(format!("sobra '{other}'"));
                }
            }
        }
    }
    if !o.list && o.port.is_none() && o.sim.is_none() {
        return Err("hace falta --port PUERTO (o --list para verlos, o --sim programa para la simulacion)".into());
    }
    if o.port.is_some() && o.sim.is_some() {
        return Err("--port y --sim no se combinan: uno es el puerto fisico y el otro la simulacion".into());
    }
    Ok(o)
}

pub const HELP: &str = "uso:
  pypic monitor --list [--format json]                  los puertos serie disponibles
  pypic monitor --port COM3 --baud 9600 [opciones]      terminal serial: lo recibido sale por la salida estandar; lo que se escribe en la entrada se envia
  pypic monitor --sim programa.py|.bas [--seconds N]    la salida por UART del programa en el simulador, con el mismo formato (sin hardware)

opciones: --data-bits 5..8  --parity none|even|odd  --stop-bits 1|2  --flow none|rts|xon  --encoding ascii|utf8|hex  --timestamps  --log archivo
          --filter REGEX  --format text|jsonl  --eol none|cr|lf|crlf  --send-encoding ascii|hex  --send-file archivo  --reconnect
          (con --sim tambien --devices-dir, --project, --target, --device, --config, --no-opt, como build)";

// -------------------------------------------------------------------------------------------------------------------- los puertos

fn port_json(p: &serialport::SerialPortInfo) -> Value {
    let (kind, extra) = match &p.port_type {
        serialport::SerialPortType::UsbPort(u) => {
            ("usb", json!({ "vid": format!("{:04X}", u.vid), "pid": format!("{:04X}", u.pid), "manufacturer": u.manufacturer, "product": u.product, "serial_number": u.serial_number }))
        }
        serialport::SerialPortType::BluetoothPort => ("bluetooth", json!({})),
        serialport::SerialPortType::PciPort => ("pci", json!({})),
        serialport::SerialPortType::Unknown => ("unknown", json!({})),
    };
    let mut v = json!({ "name": p.port_name, "type": kind });
    if let (Some(o), Some(e)) = (v.as_object_mut(), extra.as_object()) {
        o.extend(e.iter().map(|(k, x)| (k.clone(), x.clone())));
    }
    v
}

fn list(json_out: bool) -> ExitCode {
    let ports = match serialport::available_ports() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("no se pudieron listar los puertos serie: {e}");
            return ExitCode::from(2);
        }
    };
    if json_out {
        println!("{}", serde_json::to_string_pretty(&json!({ "version": JSON_VERSION, "ports": ports.iter().map(port_json).collect::<Vec<_>>() })).expect("el informe es JSON valido"));
    } else if ports.is_empty() {
        println!("no hay puertos serie (conecta el adaptador USB-UART)");
    } else {
        for p in &ports {
            let detail = match &p.port_type {
                serialport::SerialPortType::UsbPort(u) => format!("USB {:04X}:{:04X} {}", u.vid, u.pid, u.product.clone().unwrap_or_default()),
                _ => String::new(),
            };
            println!("{}  {}", p.port_name, detail.trim());
        }
    }
    ExitCode::SUCCESS
}

fn open(o: &Options, name: &str) -> Result<Box<dyn serialport::SerialPort>, (String, u8)> {
    use serialport::{DataBits, FlowControl, Parity, StopBits};
    let builder = serialport::new(name, o.baud)
        .timeout(Duration::from_millis(100))
        .data_bits(match o.data_bits {
            5 => DataBits::Five,
            6 => DataBits::Six,
            7 => DataBits::Seven,
            _ => DataBits::Eight,
        })
        .parity(match o.parity.as_str() {
            "even" => Parity::Even,
            "odd" => Parity::Odd,
            _ => Parity::None,
        })
        .stop_bits(if o.stop_bits == 2 { StopBits::Two } else { StopBits::One })
        .flow_control(match o.flow.as_str() {
            "rts" => FlowControl::Hardware,
            "xon" => FlowControl::Software,
            _ => FlowControl::None,
        });
    builder.open().map_err(|e| {
        let text = e.to_string();
        let lower = text.to_lowercase();
        let msg = match e.kind {
            serialport::ErrorKind::NoDevice => format!("el puerto '{name}' no existe o se desconecto (puertos disponibles: pypic monitor --list)"),
            serialport::ErrorKind::InvalidInput => format!("el puerto '{name}' no admite esa configuracion ({text}): prueba otra velocidad u otros bits"),
            _ if matches!(&e.kind, serialport::ErrorKind::Io(k) if *k == ErrorKind::PermissionDenied) || lower.contains("denied") || lower.contains("busy") || lower.contains("in use") => {
                format!("el puerto '{name}' esta ocupado (otro programa lo tiene abierto: MicroBrn, PuTTY, otra terminal) o no tienes permiso: {text}")
            }
            _ => format!("no se pudo abrir el puerto '{name}': {text}"),
        };
        (msg, 2)
    })
}

// -------------------------------------------------------------------------------------------------------------------- la sesion

struct Output {
    log: Option<std::fs::File>,
}

impl Output {
    fn new(path: Option<&str>) -> Result<Output, String> {
        let log = match path {
            Some(p) => Some(OpenOptions::new().create(true).append(true).open(p).map_err(|e| format!("no se pudo abrir el registro '{p}': {e}"))?),
            None => None,
        };
        Ok(Output { log })
    }

    fn line(&mut self, text: &str) {
        println!("{text}");
        let _ = std::io::stdout().flush();
        if let Some(f) = &mut self.log {
            let _ = writeln!(f, "{text}");
        }
    }
}

fn settings(o: &Options) -> Result<Settings, String> {
    let filter = match &o.filter {
        Some(f) => Some(Regex::new(f).map_err(|e| format!("--filter no es una expresion regular valida: {e}"))?),
        None => None,
    };
    Ok(Settings { encoding: o.encoding, timestamps: o.timestamps, filter })
}

fn event(out: &mut Output, jsonl: bool, kind: &str, message: &str) {
    if jsonl {
        out.line(&json!({ "version": JSON_VERSION, "type": kind, "message": message }).to_string());
    } else {
        eprintln!("{message}");
    }
}

/// Envia lo que se escribe en la entrada estandar (y un archivo si se pidio) por el puerto.
fn spawn_sender(mut port: Box<dyn serialport::SerialPort>, o: &Options) -> Result<(), String> {
    if let Some(path) = &o.send_file {
        let data = std::fs::read(path).map_err(|e| format!("no se pudo leer '{path}': {e}"))?;
        port.write_all(&data).map_err(|e| format!("no se pudo enviar '{path}': {e}"))?;
    }
    let (eol, hex_mode) = (o.eol, o.send_hex);
    std::thread::spawn(move || {
        for line in std::io::stdin().lock().lines().map_while(Result::ok) {
            let data = if hex_mode {
                match parse_hex(&line) {
                    Ok(d) => d,
                    Err(e) => {
                        eprintln!("{e}");
                        continue;
                    }
                }
            } else {
                let mut d = line.into_bytes();
                d.extend_from_slice(eol.bytes());
                d
            };
            if port.write_all(&data).and_then(|_| port.flush()).is_err() {
                break;
            }
        }
    });
    Ok(())
}

fn run_port(o: &Options, name: &str) -> ExitCode {
    let (settings, mut out) = match (settings(o), Output::new(o.log.as_deref())) {
        (Ok(s), Ok(out)) => (s, out),
        (Err(e), _) | (_, Err(e)) => {
            eprintln!("{e}");
            return ExitCode::from(2);
        }
    };
    let start = Instant::now();
    let now = move || start.elapsed().as_millis() as u64;
    loop {
        let mut port = match open(o, name) {
            Ok(p) => p,
            Err((msg, code)) => {
                if o.reconnect && code == 2 {
                    event(&mut out, o.jsonl, "waiting", &format!("{msg}; se reintenta cada segundo"));
                    std::thread::sleep(Duration::from_secs(1));
                    continue;
                }
                eprintln!("{msg}");
                return ExitCode::from(code);
            }
        };
        event(&mut out, o.jsonl, "open", &format!("{name} a {} baudios ({}{}{})", o.baud, o.data_bits, o.parity.chars().next().unwrap_or('n').to_ascii_uppercase(), o.stop_bits));
        match port.try_clone() {
            Ok(clone) => {
                if let Err(e) = spawn_sender(clone, o) {
                    eprintln!("{e}");
                    return ExitCode::from(2);
                }
            }
            Err(e) => eprintln!("no se puede enviar por este puerto: {e}"),
        }
        let end = pump(&mut port, &settings, o.jsonl, &now, &mut |l| out.line(l));
        match end {
            End::Eof => return ExitCode::SUCCESS,
            End::Disconnected(why) => {
                event(&mut out, o.jsonl, "closed", &format!("el puerto '{name}' se desconecto: {why}"));
                if !o.reconnect {
                    return ExitCode::from(3);
                }
                std::thread::sleep(Duration::from_secs(1));
            }
        }
    }
}

// ----------------------------------------------------------------------------------------------------------------- el simulador

/// Compila el programa como `pypic build` y devuelve su imagen y el reloj en Hz.
fn build_for_sim(o: &Options, file: &str) -> Result<(pypic_pic14::Image, u32), String> {
    let exe = std::env::current_exe().map_err(|e| format!("no se encontro el ejecutable de pypic: {e}"))?;
    let hex_path = std::env::temp_dir().join(format!("pypic-monitor-{}.hex", std::process::id()));
    let result = std::process::Command::new(exe).arg("build").arg(file).arg("-o").arg(&hex_path).args(&o.build_args).output().map_err(|e| format!("no se pudo compilar: {e}"))?;
    let stdout = String::from_utf8_lossy(&result.stdout).into_owned();
    if !result.status.success() {
        return Err(format!("{}{}", stdout, String::from_utf8_lossy(&result.stderr)).trim_end().to_string());
    }
    let text = std::fs::read_to_string(&hex_path).map_err(|e| format!("no se pudo leer la imagen: {e}"));
    let _ = std::fs::remove_file(&hex_path);
    let image = pypic_pic14::Image::from_intel_hex(&text?).map_err(|e| format!("imagen invalida: {e:?}"))?;
    let hz = stdout.lines().find_map(|l| l.trim().strip_prefix("Reloj:")).and_then(|r| r.split_whitespace().next()?.parse::<u32>().ok()).unwrap_or(4_000_000);
    Ok((image, hz))
}

/// Lo que la UART del programa envia en `seconds` segundos de tiempo del PIC: (milisegundos, bytes) en tramos de bytes seguidos.
pub fn simulate(image: &pypic_pic14::Image, hz: u32, seconds: u64) -> Vec<(u64, Vec<u8>)> {
    let mut sim = pypic_pic14::sim::Sim::new(image);
    sim.fosc_hz = hz;
    // un ciclo de instruccion son 4 ciclos de reloj
    let cycles = seconds * hz as u64 / 4;
    sim.run_until(cycles, |_| false);
    let to_ms = |cycle: u64| cycle * 4_000 / hz as u64;
    let mut chunks: Vec<(u64, Vec<u8>)> = Vec::new();
    for (byte, cycle) in &sim.uart_tx {
        let ms = to_ms(*cycle);
        match chunks.last_mut() {
            Some((last_ms, bytes)) if ms.saturating_sub(*last_ms) <= 2 => {
                bytes.push(*byte);
                *last_ms = ms;
            }
            _ => chunks.push((ms, vec![*byte])),
        }
    }
    chunks
}

/// Una lectura que entrega los tramos de la simulacion y marca el tiempo de cada uno.
struct Replay {
    chunks: std::collections::VecDeque<(u64, Vec<u8>)>,
    clock: std::rc::Rc<std::cell::Cell<u64>>,
}

fn run_sim(o: &Options, file: &str) -> ExitCode {
    let (settings, mut out) = match (settings(o), Output::new(o.log.as_deref())) {
        (Ok(s), Ok(out)) => (s, out),
        (Err(e), _) | (_, Err(e)) => {
            eprintln!("{e}");
            return ExitCode::from(2);
        }
    };
    let (image, hz) = match build_for_sim(o, file) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::from(1);
        }
    };
    event(&mut out, o.jsonl, "open", &format!("simulacion de {file} a {hz} Hz, {} s de tiempo del PIC", o.seconds));
    let replay = Replay { chunks: simulate(&image, hz, o.seconds).into(), clock: Default::default() };
    struct Source(Replay);
    impl Read for Source {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            match self.0.chunks.pop_front() {
                Some((ms, bytes)) => {
                    self.0.clock.set(ms);
                    let n = bytes.len().min(buf.len());
                    buf[..n].copy_from_slice(&bytes[..n]);
                    Ok(n)
                }
                None => Ok(0),
            }
        }
    }
    // el reloj de la sesion es el tiempo simulado del ultimo tramo leido
    let clock = replay.clock.clone();
    let mut source = Source(replay);
    let now = move || clock.get();
    pump(&mut source, &settings, o.jsonl, &now, &mut |l| out.line(l));
    ExitCode::SUCCESS
}

pub fn monitor(args: &[String]) -> ExitCode {
    if args.is_empty() || args.iter().any(|a| a == "--help" || a == "-h") {
        println!("{HELP}");
        return if args.is_empty() { ExitCode::from(2) } else { ExitCode::SUCCESS };
    }
    let o = match parse_args(args) {
        Ok(o) => o,
        Err(e) => {
            eprintln!("{e}\n\n{HELP}");
            return ExitCode::from(2);
        }
    };
    if o.list {
        return list(o.list_json);
    }
    match (&o.port, &o.sim) {
        (Some(p), _) => run_port(&o, p),
        (_, Some(f)) => run_sim(&o, f),
        _ => ExitCode::from(2),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings(encoding: Encoding, timestamps: bool, filter: Option<&str>) -> Settings {
        Settings { encoding, timestamps, filter: filter.map(|f| Regex::new(f).unwrap()) }
    }

    fn run(bytes: &[u8], s: &Settings, jsonl: bool) -> Vec<String> {
        let mut out = Vec::new();
        let mut cursor = std::io::Cursor::new(bytes.to_vec());
        let end = pump(&mut cursor, s, jsonl, &|| 1_234, &mut |l| out.push(l.to_string()));
        assert_eq!(end, End::Eof);
        out
    }

    #[test]
    fn timestamps_are_minutes_seconds_milliseconds_since_the_start() {
        assert_eq!(timestamp(0), "[00:00.000]");
        assert_eq!(timestamp(1_125), "[00:01.125]");
        assert_eq!(timestamp(61_005), "[01:01.005]");
    }

    #[test]
    fn ascii_shows_what_does_not_print_as_escapes() {
        assert_eq!(ascii(b"Hola\t\r\n\x00\xFF"), "Hola\t\\x0D\\x0A\\x00\\xFF");
        assert_eq!(hex(b"Ho\x00"), "48 6F 00");
    }

    #[test]
    fn lines_are_split_on_lf_and_cr_lf_is_stripped() {
        let s = settings(Encoding::Ascii, false, None);
        assert_eq!(run(b"uno\r\ndos\ntres", &s, false), vec!["uno", "dos", "tres"], "la ultima linea sin salto se muestra al final");
    }

    #[test]
    fn a_line_can_arrive_in_pieces() {
        let s = settings(Encoding::Ascii, true, None);
        let mut f = Formatter::new(&s);
        assert!(f.feed(b"AD", 10).is_empty());
        assert!(f.feed(b"C0 = 5", 20).is_empty());
        assert_eq!(f.feed(b"12\n", 30), vec!["[00:00.030] ADC0 = 512"]);
        assert!(f.flush(40).is_empty());
        f.feed(b">", 50);
        assert_eq!(f.flush(60), vec!["[00:00.060] >"], "un prompt sin salto de linea se vacia a mano");
    }

    #[test]
    fn the_filter_keeps_only_matching_lines() {
        let s = settings(Encoding::Ascii, false, Some("^ADC"));
        assert_eq!(run(b"ADC0 = 1\nruido\nADC1 = 2\n", &s, false), vec!["ADC0 = 1", "ADC1 = 2"]);
    }

    #[test]
    fn hex_shows_each_chunk_and_utf8_decodes_text() {
        assert_eq!(run(b"Hi\n", &settings(Encoding::Hex, false, None), false), vec!["48 69 0A"]);
        assert_eq!(run("aña\n".as_bytes(), &settings(Encoding::Utf8, false, None), false), vec!["aña"]);
        assert_eq!(run("aña\n".as_bytes(), &settings(Encoding::Ascii, false, None), false), vec!["a\\xC3\\xB1a"], "en ASCII un byte sobre 127 se escribe como \\xNN");
    }

    #[test]
    fn jsonl_events_are_versioned_with_hex_and_text() {
        let lines = run(b"OK\n", &settings(Encoding::Ascii, false, None), true);
        let v: Value = serde_json::from_str(&lines[0]).unwrap();
        assert_eq!((v["version"].as_u64(), v["type"].as_str(), v["hex"].as_str(), v["bytes"].as_u64(), v["t_ms"].as_u64()), (Some(1), Some("data"), Some("4F 4B 0A"), Some(3), Some(1_234)));
        assert_eq!(v["text"], "OK\\x0A");
    }

    #[test]
    fn a_failed_read_is_a_disconnection_and_pending_text_is_still_shown() {
        struct Breaks(bool);
        impl Read for Breaks {
            fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
                if self.0 {
                    return Err(std::io::Error::new(ErrorKind::BrokenPipe, "el adaptador se desconecto"));
                }
                self.0 = true;
                buf[..3].copy_from_slice(b"par");
                Ok(3)
            }
        }
        let s = settings(Encoding::Ascii, false, None);
        let mut out = Vec::new();
        let end = pump(&mut Breaks(false), &s, false, &|| 0, &mut |l| out.push(l.to_string()));
        assert!(matches!(end, End::Disconnected(ref m) if m.contains("desconect")), "{end:?}");
        assert_eq!(out, vec!["par"]);
    }

    #[test]
    fn hex_to_send_accepts_spaces_and_prefixes_and_explains_errors() {
        assert_eq!(parse_hex("48 65 6C").unwrap(), b"Hel");
        assert_eq!(parse_hex("0x48 0x65").unwrap(), b"He");
        assert_eq!(parse_hex("4865").unwrap(), b"He");
        assert!(parse_hex("486").unwrap_err().contains("faltan cifras"));
        assert!(parse_hex("ZZ").unwrap_err().contains("ZZ"));
    }

    #[test]
    fn options_are_parsed_and_validated() {
        let a = |s: &str| s.split_whitespace().map(String::from).collect::<Vec<_>>();
        let o = parse_args(&a("--port COM3 --baud 115200 --parity even --stop-bits 2 --encoding hex --timestamps --filter ADC --eol crlf")).unwrap();
        assert_eq!((o.port.as_deref(), o.baud, o.parity.as_str(), o.stop_bits, o.encoding, o.timestamps, o.eol), (Some("COM3"), 115200, "even", 2, Encoding::Hex, true, Eol::CrLf));
        assert_eq!(parse_args(&a("programa.bas --seconds 3")).unwrap().sim.as_deref(), Some("programa.bas"), "un archivo suelto es --sim");
        assert!(parse_args(&a("--baud 9600")).unwrap_err().contains("hace falta --port"));
        assert!(parse_args(&a("--port A --sim b.py")).unwrap_err().contains("no se combinan"));
        assert!(parse_args(&a("--port A --parity raro")).unwrap_err().contains("none, even u odd"));
        assert!(parse_args(&a("--port A --data-bits 9")).unwrap_err().contains("5, 6, 7 u 8"));
        assert!(parse_args(&a("--port A --baud")).unwrap_err().contains("necesita un valor"));
        assert!(parse_args(&a("--port A --nada")).unwrap_err().contains("--nada"));
        assert!(parse_args(&a("--list")).unwrap().list);
    }
}
