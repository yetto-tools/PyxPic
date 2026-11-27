//! I2C maestro por hardware (`pypic.periph.i2c`): tipos compartidos entre el checker, el IR y el backend.
//!
//! Fuente: DS40001291H cap. 13 (pp. 175-205), modo maestro de 7 bits:
//!  * reloj de SCL = FOSC / (4 * (SSPADD + 1)) (SSPADD<6:0>); con SSPEN = 1 y SSPM = 1000 el MSSP es maestro I2C;
//!  * SCL (RC3) y SDA (RC4) se dejan como entradas (TRISC<3:4> = 1): el modulo maneja el pin, que es de drenador abierto;
//!  * SEN, RSEN y PEN (SSPCON2<0:2>) inician START, START repetido y STOP y el hardware los baja al terminar;
//!  * escribir SSPBUF transmite el byte; R/W (SSPSTAT<2>) vale 1 mientras sale el byte y su ACK; ACKSTAT (SSPCON2<6>) = 0 si hubo ACK;
//!  * RCEN (SSPCON2<3>) recibe un byte en SSPBUF y baja solo; despues ACKDT (SSPCON2<5>, 0 = ACK) y ACKEN (SSPCON2<4>) responden
//!    (13.4.9 y 13.4.10);
//!  * SMP (SSPSTAT<7>): 1 = control de pendiente desactivado (100 kHz y 1 MHz), 0 = activado (400 kHz).

/// El divisor minimo que PyPIC acepta (SSPADD >= 1): decision de PyPIC, el datasheet da 0 solo con relojes mucho mas rapidos.
pub const SSPADD_MIN: u32 = 1;
/// SSPADD es de 7 bits en modo maestro.
pub const SSPADD_MAX: u32 = 127;

/// La configuracion resuelta con el reloj del sistema.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct I2cStart {
    /// La frecuencia pedida de SCL en Hz.
    pub freq: u32,
    /// `None` hasta resolver el reloj del sistema.
    pub sspadd: Option<u8>,
}

impl I2cStart {
    /// Frecuencia real de SCL con `fosc_hz`.
    pub fn achieved(&self, fosc_hz: u32) -> f64 {
        fosc_hz as f64 / (4.0 * (self.sspadd.unwrap_or(0) as f64 + 1.0))
    }

    /// SMP: el control de pendiente solo se activa en el modo de 400 kHz.
    pub fn slew_control(&self, fosc_hz: u32) -> bool {
        let f = self.achieved(fosc_hz);
        f > 100_000.0 && f <= 400_000.0
    }
}

/// El SSPADD que da una frecuencia de SCL **no mayor** que `freq` con `fosc_hz`; `None` si no cabe en `SSPADD_MIN..=SSPADD_MAX`.
pub fn solve(fosc_hz: u32, freq: u32) -> Option<u8> {
    let div = (fosc_hz as u64).div_ceil(4 * freq as u64);
    let n = div.checked_sub(1)?;
    (SSPADD_MIN as u64..=SSPADD_MAX as u64).contains(&n).then_some(n as u8)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum I2cOp {
    /// `I2C.start(freq=N)`: SSPADD, SSPSTAT, SSPCON (SSPEN y SSPM = 1000) y SCL y SDA como entradas.
    Start(I2cStart),
    /// `I2C.begin()`: condicion de START (SEN = 1) y espera a que termine.
    Begin,
    /// `I2C.restart()`: START repetido (RSEN = 1) y espera.
    Restart,
    /// `I2C.end()`: condicion de STOP (PEN = 1) y espera.
    End,
    /// `I2C.write(b)`: envia el byte, espera y devuelve `true` si el esclavo respondio ACK.
    Write,
    /// `I2C.read(ack=x)`: recibe un byte (RCEN), responde ACK si `x` o NACK si no (ACKDT, ACKEN) y lo devuelve.
    Read,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct I2cCall {
    pub op: I2cOp,
}
