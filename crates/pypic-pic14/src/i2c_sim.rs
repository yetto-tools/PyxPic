//! El MSSP en modo I²C maestro (transmision) y un bus con esclavos virtuales, para el simulador del nucleo (docs/sdk-i2c.md).
//!
//! Fuente: DS40001291H cap. 13, pp. 176-178 (SSPSTAT, SSPCON, SSPCON2) y 189-193 (modo maestro). Modelado:
//!  * `SEN`, `RSEN` y `PEN` (SSPCON2<0>, <1>, <2>) inician START, START repetido y STOP; el hardware los baja solo al terminar y sube `SSPIF`
//!    (PIR1<3>); `S` y `P` de SSPSTAT reflejan la ultima condicion. Solo se aceptan con el MSSP en reposo (nota 1 de SSPCON2, p.178): si no, no
//!    se activan;
//!  * escribir SSPBUF en reposo transmite 8 bits y lee el ACK: `ACKSTAT` (SSPCON2<6>) = 0 si el esclavo respondio; `R/W` (SSPSTAT<2>) y `BF`
//!    (SSPSTAT<0>) valen 1 mientras transmite. Escribir SSPBUF sin estar en reposo sube `WCOL` (SSPCON<7>) y no escribe (13.4.6.1);
//!  * el primer byte tras un START es la direccion de 7 bits y el bit R/W; el esclavo responde ACK si su direccion coincide y, despues, ACK a cada
//!    dato de escritura.
//!
//! Tiempos (aproximados: no medidos en el chip): el reloj de SCL es Fosc/(4*(SSPADD+1)), o sea un periodo de SSPADD+1 ciclos de instruccion; un
//! START, START repetido o STOP dura un periodo y un byte con su ACK, nueve. Sin recepcion (RCEN, ACKEN), arbitraje, colision de bus ni esclavo.

const SSPBUF: usize = 0x13;
const SSPCON: usize = 0x14;
const PIR1: usize = 0x0C;
const SSPCON2: usize = 0x91;
const SSPADD: usize = 0x93;
const SSPSTAT: usize = 0x94;
const SSPIF: u8 = 0x08;

/// Lo que paso en el bus, en orden.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum I2cEvent {
    Start,
    RepeatedStart,
    /// Direccion de 7 bits, `true` si el bit R/W pide lectura, y si algun esclavo respondio ACK.
    Address {
        addr: u8,
        read: bool,
        ack: bool,
    },
    /// Un byte de datos que el maestro envio y si se le respondio ACK.
    Data {
        byte: u8,
        ack: bool,
    },
    Stop,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Op {
    Start,
    Repeated,
    Stop,
    Byte(u8),
}

#[derive(Clone, Debug, Default)]
pub struct I2cBus {
    /// Direcciones de 7 bits de los esclavos que responden ACK.
    pub slaves: Vec<u8>,
    pub events: Vec<I2cEvent>,
    /// Reglas del datasheet que el programa incumplio.
    pub violations: Vec<String>,
    busy: Option<(Op, u64)>,
    /// El siguiente byte es una direccion (justo despues de un START).
    expect_address: bool,
    /// El esclavo direccionado en la transaccion en curso respondio ACK.
    selected: bool,
}

impl I2cBus {
    pub fn reset(&mut self) {
        self.busy = None;
        self.expect_address = false;
        self.selected = false;
    }

    /// Los bytes de datos que recibio el esclavo `addr` en toda la simulacion.
    pub fn received_by(&self, addr: u8) -> Vec<u8> {
        let mut out = Vec::new();
        let mut current = None;
        for e in &self.events {
            match e {
                I2cEvent::Address { addr: a, read: false, ack: true } => current = Some(*a),
                I2cEvent::Address { .. } | I2cEvent::Stop | I2cEvent::Start | I2cEvent::RepeatedStart => current = None,
                I2cEvent::Data { byte, ack: true } if current == Some(addr) => out.push(*byte),
                I2cEvent::Data { .. } => {}
            }
        }
        out
    }

    fn period_cycles(ram: &[u8]) -> u64 {
        (ram[SSPADD] as u64 & 0x7F) + 1
    }

    fn enabled(&self, ram: &[u8]) -> bool {
        ram[SSPCON] & 0x20 != 0 && ram[SSPCON] & 0x0F == 0x08
    }

    /// El MSSP esta en reposo: ninguna operacion en curso y `SEN`, `RSEN`, `PEN`, `RCEN`, `ACKEN` y `R/W` en 0.
    fn idle(&self, ram: &[u8]) -> bool {
        self.busy.is_none() && ram[SSPCON2] & 0x1F == 0 && ram[SSPSTAT] & 0x04 == 0
    }

    /// El programa escribio SSPCON2: `old` es el valor anterior y `ram[SSPCON2]` ya trae el nuevo.
    pub fn sspcon2_write(&mut self, ram: &mut [u8; 512], old: u8, now: u64) {
        let requested = ram[SSPCON2] & 0x07 & !old;
        if requested == 0 {
            // el programa no puede bajar un bit de operacion en curso: lo conserva el hardware
            ram[SSPCON2] = (ram[SSPCON2] & !0x07) | (old & 0x07);
            return;
        }
        if !self.enabled(ram) {
            self.violations.push("SEN, RSEN o PEN con el MSSP apagado o fuera del modo I2C maestro (SSPEN = 0 o SSPM distinto de 1000): falta I2C.start".into());
            ram[SSPCON2] &= !0x07;
            return;
        }
        if !self.idle_before(ram, old) {
            ram[SSPCON2] = (ram[SSPCON2] & !0x07) | (old & 0x07);
            self.violations.push("evento de bus pedido con el MSSP ocupado: no se activa (no hay cola de eventos, p.189)".into());
            return;
        }
        // un solo evento a la vez: si el programa pide varios, vale el primero (START, RSEN, STOP)
        let (op, bit) = if requested & 1 != 0 {
            (Op::Start, 1)
        } else if requested & 2 != 0 {
            (Op::Repeated, 2)
        } else {
            (Op::Stop, 4)
        };
        ram[SSPCON2] = (ram[SSPCON2] & !0x07) | bit;
        self.busy = Some((op, now + Self::period_cycles(ram)));
    }

    fn idle_before(&self, ram: &[u8], old: u8) -> bool {
        self.busy.is_none() && old & 0x1F == 0 && ram[SSPSTAT] & 0x04 == 0
    }

    /// El programa escribio SSPBUF (`old` es lo que tenia).
    pub fn sspbuf_write(&mut self, ram: &mut [u8; 512], old: u8, now: u64) {
        if !self.enabled(ram) {
            self.violations.push("SSPBUF escrito con el MSSP apagado o fuera del modo I2C maestro: falta I2C.start".into());
            return;
        }
        if !self.idle(ram) {
            ram[SSPCON] |= 0x80; // WCOL
            ram[SSPBUF] = old;
            self.violations.push("SSPBUF escrito con una operacion de bus en curso: no se escribe y sube WCOL (13.4.6.1)".into());
            return;
        }
        let byte = ram[SSPBUF];
        ram[SSPSTAT] |= 0x05; // R/W y BF: transmitiendo
        self.busy = Some((Op::Byte(byte), now + 9 * Self::period_cycles(ram)));
    }

    pub fn tick(&mut self, ram: &mut [u8; 512], now: u64) {
        let Some((op, done)) = self.busy else { return };
        if now < done {
            return;
        }
        self.busy = None;
        match op {
            Op::Start | Op::Repeated => {
                self.events.push(if op == Op::Start { I2cEvent::Start } else { I2cEvent::RepeatedStart });
                ram[SSPCON2] &= !(if op == Op::Start { 0x01 } else { 0x02 });
                ram[SSPSTAT] = (ram[SSPSTAT] & !0x10) | 0x08; // S = 1, P = 0
                self.expect_address = true;
                self.selected = false;
            }
            Op::Stop => {
                self.events.push(I2cEvent::Stop);
                ram[SSPCON2] &= !0x04;
                ram[SSPSTAT] = (ram[SSPSTAT] & !0x08) | 0x10; // P = 1, S = 0
                self.expect_address = false;
                self.selected = false;
            }
            Op::Byte(byte) => {
                ram[SSPSTAT] &= !0x05; // R/W y BF en 0
                let ack = if self.expect_address {
                    let (addr, read) = (byte >> 1, byte & 1 != 0);
                    let ack = self.slaves.contains(&addr);
                    self.events.push(I2cEvent::Address { addr, read, ack });
                    self.expect_address = false;
                    self.selected = ack && !read;
                    ack
                } else {
                    self.events.push(I2cEvent::Data { byte, ack: self.selected });
                    self.selected
                };
                // ACKSTAT: 1 = no hubo ACK (p.178)
                ram[SSPCON2] = (ram[SSPCON2] & !0x40) | if ack { 0 } else { 0x40 };
            }
        }
        ram[PIR1] |= SSPIF;
    }
}
