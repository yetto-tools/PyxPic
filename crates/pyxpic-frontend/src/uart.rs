//! EUSART asincrono 8N1 (`pyxpic.periph.uart`): tipos compartidos entre el checker, el IR y el backend.
//!
//! Fuente: DS40001291H cap. 12 (p.147-160). La tabla 12-3 (p.160, leida de la imagen de la pagina) da la frecuencia de baudios:
//!
//! | BRG16 | BRGH | formula |
//! |---|---|---|
//! | 0 | 0 | FOSC / [64 (n+1)] |
//! | 0 | 1 | FOSC / [16 (n+1)] |
//! | 1 | 0 | FOSC / [16 (n+1)] |
//! | 1 | 1 | FOSC / [4 (n+1)] |
//!
//! con `n` = `SPBRG` (8 bits) o `SPBRGH:SPBRG` (16 bits, solo con BRG16).
//!
//! El USART del PIC16F628A (DS40044G p.75) no tiene BRG16 ni `SPBRGH`: solo las dos primeras filas (ver `solve_with`).
//!
//! **Tolerancia al error de baudios** (decision de PyxPic, no del datasheet: el datasheet no da un limite): una trama 8N1 son 10
//! bits y el receptor muestrea cada bit hacia su centro; si el error total entre los dos extremos pasa de ~4,7 % el ultimo bit se
//! lee fuera de su ventana. Se avisa por encima de **2,0 %** (el margen para que el otro extremo tambien tenga su error) y es un
//! error por encima de **4,5 %** (aunque el otro extremo fuera exacto, la comunicacion fallaria).

use crate::ast::{FKind, FSpec};

/// El generador de baudios resuelto: el valor pedido, los bits de modo y `n`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UartBrg {
    pub baud: u32,
    pub brgh: bool,
    pub brg16: bool,
    pub n: u16,
}

pub const WARN_PCT: f64 = 2.0;
pub const MAX_PCT: f64 = 4.5;

impl UartBrg {
    /// El divisor de FOSC de la formula de la tabla 12-3.
    pub fn divisor(&self) -> u32 {
        match (self.brg16, self.brgh) {
            (false, false) => 64,
            (false, true) | (true, false) => 16,
            (true, true) => 4,
        }
    }

    /// Frecuencia de baudios que da a `fosc_hz`.
    pub fn achieved(&self, fosc_hz: u32) -> f64 {
        fosc_hz as f64 / (self.divisor() as f64 * (self.n as f64 + 1.0))
    }

    /// Error en porcentaje con signo respecto al pedido (el del ejemplo 12-1: (calculado - deseado) / deseado).
    pub fn error_pct(&self, fosc_hz: u32) -> f64 {
        (self.achieved(fosc_hz) - self.baud as f64) / self.baud as f64 * 100.0
    }
}

/// La configuracion de menor error: prueba los cuatro modos con el `n` entero mas cercano por arriba y por abajo; a igual error
/// gana el modo mas simple (8 bits, BRGH = 0).
pub fn solve(fosc_hz: u32, baud: u32) -> Option<UartBrg> {
    solve_with(fosc_hz, baud, true)
}

/// Como `solve`; con `has_brg16 = false` solo prueba los modos de 8 bits (dispositivos sin `SPBRGH`).
pub fn solve_with(fosc_hz: u32, baud: u32, has_brg16: bool) -> Option<UartBrg> {
    let mut best: Option<(f64, UartBrg)> = None;
    for (brg16, brgh) in [(false, false), (false, true), (true, false), (true, true)] {
        if brg16 && !has_brg16 {
            continue;
        }
        let probe = UartBrg { baud, brgh, brg16, n: 0 };
        let max_n: i64 = if brg16 { 65_535 } else { 255 };
        let exact = fosc_hz as f64 / (probe.divisor() as f64 * baud as f64) - 1.0;
        for cand in [exact.floor() as i64, exact.ceil() as i64] {
            if !(0..=max_n).contains(&cand) {
                continue;
            }
            let c = UartBrg { n: cand as u16, ..probe };
            let err = c.error_pct(fosc_hz).abs();
            if best.as_ref().is_none_or(|(e, _)| err < *e - 1e-9) {
                best = Some((err, c));
            }
        }
    }
    best.map(|(_, c)| c)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UartStart {
    pub baud: u32,
    /// `bits=9`: caracteres de 9 bits (TX9/RX9); solo valen `write9`/`send9`/`read9`/`recv9`.
    pub bits9: bool,
    /// `None` hasta resolver el reloj del sistema.
    pub brg: Option<UartBrg>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UartOp {
    /// `UART.start(baud=N)`: BAUDCTL, SPBRGH:SPBRG, TXSTA (TXEN) y RCSTA (SPEN, CREN).
    Start(UartStart),
    /// `UART.write(x)`: espera `TXIF`, escribe `TXREG`.
    Write,
    /// `UART.read()`: espera `RCIF`, lee `RCREG`.
    Read,
    /// `UART.tx_ready()`: `TXIF`.
    TxReady,
    /// `UART.rx_ready()`: `RCIF`.
    RxReady,
    /// `UART.send(x)`: escribe `TXREG` sin esperar (y un NOP: `TXIF` es valido dos ciclos despues).
    Send,
    /// `UART.recv()`: lee `RCREG` sin esperar.
    Recv,
    /// `UART.sending()`: el registro de desplazamiento sigue ocupado (`TRMT` = 0).
    Sending,
    /// `UART.overrun()`: `OERR`.
    Overrun,
    /// `UART.framing_error()`: `FERR` del caracter que esta a punto de leerse.
    FramingError,
    /// `UART.clear_overrun()`: `CREN` a 0 y a 1 (la unica forma de borrar `OERR`).
    ClearOverrun,
    /// `UART.write9(x)`: espera `TXIF`, pone `TX9D` con el bit 8 de `x` y escribe `TXREG` con el byte bajo.
    Write9,
    /// `UART.send9(x)`: lo mismo sin esperar (y con el `NOP`).
    Send9,
    /// `UART.read9()`: espera `RCIF`, lee `RX9D` (antes de `RCREG`) y `RCREG`.
    Read9,
    /// `UART.recv9()`: lo mismo sin esperar.
    Recv9,
    /// `UART.write_text(texto)`: envia los bytes de la tabla `Rom` (indice en `Program.roms`) con `write`; la IR lo desarma en un bucle.
    WriteText(usize),
    /// `UART.write_text(f"...")`: las partes estan en `Program.uart_fmt` (por id de expresion); la IR lo desarma en bucles y aritmetica.
    WriteFmt,
    /// `UART.read_line(buf)`: lee hasta `\n` en el arreglo (indice en `Program.arrays`); devuelve cuantos bytes guardo.
    ReadLine(usize),
    /// `UART.write_buf(buf, n)`: envia los primeros `n` bytes del arreglo.
    WriteBuf(usize),
}

impl UartOp {
    /// Operaciones de caracteres de 9 bits.
    pub fn is_9bit(self) -> bool {
        matches!(self, UartOp::Write9 | UartOp::Send9 | UartOp::Read9 | UartOp::Recv9)
    }

    /// Operaciones de caracteres de 8 bits (las que no valen con `bits=9`).
    pub fn is_8bit_data(self) -> bool {
        matches!(self, UartOp::Write | UartOp::Send | UartOp::Read | UartOp::Recv | UartOp::WriteText(_) | UartOp::WriteFmt | UartOp::ReadLine(_) | UartOp::WriteBuf(_))
    }

    /// Operaciones que devuelven un valor sin efectos laterales: sin destino no se emiten.
    pub fn is_pure_read(self) -> bool {
        matches!(self, UartOp::TxReady | UartOp::RxReady | UartOp::Sending | UartOp::Overrun | UartOp::FramingError)
    }
}

/// Una parte de `write_text(f"...")` ya resuelta por el checker.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FmtPart {
    /// Texto constante (tambien los campos de valor constante, ya formateados): indice de la `Rom` que lo guarda.
    Lit(usize),
    /// Un campo de variable: `ast` es su posicion entre las partes de la f-string del AST, `bits` el ancho del entero (8 o 16) y
    /// `tables` las `Rom` que usa su conversion: los digitos hexadecimales (X y x) o las potencias de diez, byte bajo y byte alto
    /// (decimal; el alto solo con 16 bits).
    Field { ast: usize, bits: u8, spec: FSpec, tables: [Option<usize>; 2] },
}

/// Potencias de diez de un decimal de 8 bits (100, 10, 1) y de 16 bits (10000, 1000, 100, 10, 1): byte bajo y byte alto.
pub const POW10_8_LO: &[u8] = &[100, 10, 1];
pub const POW10_16_LO: &[u8] = &[0x10, 0xE8, 100, 10, 1];
pub const POW10_16_HI: &[u8] = &[0x27, 0x03, 0, 0, 0];

pub const HEX_UPPER: &[u8; 16] = b"0123456789ABCDEF";
pub const HEX_LOWER: &[u8; 16] = b"0123456789abcdef";

/// El formato de referencia de un entero sin signo de `bits` bits (8 o 16): es lo que generan el compilador (para los campos de
/// valor constante) y el codigo del PIC (para los variables), y lo que las pruebas comparan.
///
/// * `Dec`: decimal; con ancho, relleno a la izquierda con espacios (o ceros con `zero`); un numero mas ancho no se recorta.
/// * `X`, `x`, `b`: siempre con ceros a la izquierda, al menos los digitos del tipo (2 o 4 hex, 8 o 16 binarios): ancho fijo.
pub fn format_uint(v: u16, bits: u8, spec: FSpec) -> Vec<u8> {
    let v = if bits == 8 { v & 0xFF } else { v };
    let (radix_digits, shift): (usize, u32) = match spec.kind {
        FKind::Dec => (0, 0),
        FKind::HexUpper | FKind::HexLower => (bits as usize / 4, 4),
        FKind::Bin => (bits as usize, 1),
    };
    if spec.kind == FKind::Dec {
        let s = v.to_string().into_bytes();
        let w = spec.width as usize;
        let pad = if spec.zero { b'0' } else { b' ' };
        let mut out = vec![pad; w.saturating_sub(s.len())];
        out.extend(s);
        return out;
    }
    let table = if spec.kind == FKind::HexLower { HEX_LOWER } else { HEX_UPPER };
    let n = radix_digits.max(spec.width as usize);
    (0..n).rev().map(|j| table[((v as u32 >> (shift * j as u32).min(31)) & ((1 << shift) - 1)) as usize]).collect()
}

/// Decodifica el contenido de un literal de texto a bytes ASCII: escapes `\n`, `\r`, `\t`, `\0`, `\\`, `\"` y `\'`; cualquier
/// otro escape o caracter fuera de ASCII es un error (UTF-8 necesitaria un contrato propio).
pub fn decode_text(raw: &str) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    let mut it = raw.chars();
    while let Some(c) = it.next() {
        let b = if c == '\\' {
            match it.next() {
                Some('n') => b'\n',
                Some('r') => b'\r',
                Some('t') => b'\t',
                Some('0') => 0,
                Some('\\') => b'\\',
                Some('"') => b'"',
                Some('\'') => b'\'',
                Some(o) => return Err(format!("el escape '\\{o}' no existe (admite: \\n \\r \\t \\0 \\\\ \\\" \\')")),
                None => return Err("el texto termina en una barra invertida".into()),
            }
        } else if c.is_ascii() {
            c as u8
        } else {
            return Err(format!("el caracter '{c}' no es ASCII: por ahora el texto es ASCII (UTF-8 necesita un contrato propio)"));
        };
        out.push(b);
    }
    Ok(out)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UartCall {
    pub op: UartOp,
}
