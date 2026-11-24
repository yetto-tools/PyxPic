//! EUSART asincrono 8N1 (`pypic.periph.uart`): tipos compartidos entre el checker, el IR y el backend.
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
//! **Tolerancia al error de baudios** (decision de PyPIC, no del datasheet: el datasheet no da un limite): una trama 8N1 son 10
//! bits y el receptor muestrea cada bit hacia su centro; si el error total entre los dos extremos pasa de ~4,7 % el ultimo bit se
//! lee fuera de su ventana. Se avisa por encima de **2,0 %** (el margen para que el otro extremo tambien tenga su error) y es un
//! error por encima de **4,5 %** (aunque el otro extremo fuera exacto, la comunicacion fallaria).

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
    let mut best: Option<(f64, UartBrg)> = None;
    for (brg16, brgh) in [(false, false), (false, true), (true, false), (true, true)] {
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
}

impl UartOp {
    /// Operaciones que devuelven un valor sin efectos laterales: sin destino no se emiten.
    pub fn is_pure_read(self) -> bool {
        matches!(self, UartOp::TxReady | UartOp::RxReady | UartOp::Sending | UartOp::Overrun | UartOp::FramingError)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UartCall {
    pub op: UartOp,
}
