//! SPI maestro por hardware (`pyxpic.periph.spi`): tipos compartidos entre el checker, el IR y el backend.
//!
//! Fuente: DS40001291H cap. 13 (pp. 176-184), modo SPI maestro:
//!  * `SSPM<3:0>` = 0000, 0001 o 0010: reloj FOSC/4, FOSC/16 o FOSC/64 (p. 177; el 0011, TMR2/2, no se usa);
//!  * `CKP` (SSPCON<4>) es el nivel de reposo de SCK y `CKE` (SSPSTAT<6>) el flanco en que cambia SDO; la tabla 13-1 (p. 184) los
//!    relaciona con los modos 0 a 3; `SMP` = 0 muestrea SDI a la mitad del tiempo de dato;
//!  * SCK (RC3) y SDO (RC5) son salidas (TRIS = 0) y SDI (RC4) entrada (13.3.2, p. 180);
//!  * escribir SSPBUF inicia el intercambio de 8 bits (MSb primero); `BF` (SSPSTAT<0>) indica que llego el byte y leer SSPBUF lo borra.

/// Los divisores de FOSC disponibles y su `SSPM<3:0>`, del mas rapido al mas lento.
pub const DIVIDERS: [(u32, u8); 3] = [(4, 0), (16, 1), (64, 2)];

/// La configuracion resuelta con el reloj del sistema.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SpiStart {
    /// La frecuencia maxima pedida de SCK en Hz.
    pub freq: u32,
    /// Modo SPI estandar, 0 a 3.
    pub mode: u8,
    /// `None` hasta resolver el reloj del sistema.
    pub sspm: Option<u8>,
}

impl SpiStart {
    /// CKP: nivel de reposo de SCK (tabla 13-1).
    pub fn ckp(&self) -> bool {
        self.mode & 2 != 0
    }

    /// CKE (tabla 13-1): 1 en los modos 0 y 2.
    pub fn cke(&self) -> bool {
        self.mode & 1 == 0
    }

    /// Frecuencia real de SCK con `fosc_hz`.
    pub fn achieved(&self, fosc_hz: u32) -> u32 {
        DIVIDERS.iter().find(|(_, m)| Some(*m) == self.sspm).map_or(0, |(d, _)| fosc_hz / d)
    }
}

/// El `SSPM<3:0>` mas rapido cuyo SCK **no supera** `freq`; `None` si incluso FOSC/64 es mas rapido.
pub fn solve(fosc_hz: u32, freq: u32) -> Option<u8> {
    DIVIDERS.iter().find(|(d, _)| fosc_hz as u64 <= freq as u64 * *d as u64).map(|(_, m)| *m)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpiOp {
    /// `SPI.start(freq=N, mode=M)`: SSPSTAT, direcciones de los pines y SSPCON.
    Start(SpiStart),
    /// `SPI.transfer(b)`: envia `b` y devuelve el byte recibido a la vez.
    Transfer,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SpiCall {
    pub op: SpiOp,
}
