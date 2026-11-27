//! El MSSP en modo I²C maestro (transmision y recepcion) y un bus con esclavos virtuales, para el simulador del nucleo (docs/sdk-i2c.md).
//!
//! Fuente: DS40001291H cap. 13, pp. 176-178 (SSPSTAT, SSPCON, SSPCON2) y 189-193 (modo maestro). Modelado:
//!  * `SEN`, `RSEN` y `PEN` (SSPCON2<0>, <1>, <2>) inician START, START repetido y STOP; el hardware los baja solo al terminar y sube `SSPIF`
//!    (PIR1<3>); `S` y `P` de SSPSTAT reflejan la ultima condicion. Solo se aceptan con el MSSP en reposo (nota 1 de SSPCON2, p.178): si no, no
//!    se activan;
//!  * escribir SSPBUF en reposo transmite 8 bits y lee el ACK: `ACKSTAT` (SSPCON2<6>) = 0 si el esclavo respondio; `R/W` (SSPSTAT<2>) y `BF`
//!    (SSPSTAT<0>) valen 1 mientras transmite. Escribir SSPBUF sin estar en reposo sube `WCOL` (SSPCON<7>) y no escribe (13.4.6.1);
//!  * el primer byte tras un START es la direccion de 7 bits y el bit R/W; el esclavo responde ACK si su direccion coincide y, despues, ACK a cada
//!    dato de escritura;
//!  * tras una direccion de lectura con ACK, `RCEN` (SSPCON2<3>) recibe un byte del esclavo en SSPBUF (`BF` = 1) y `ACKEN` (SSPCON2<4>)
//!    responde con `ACKDT` (SSPCON2<5>); despues de un NACK el esclavo suelta el bus (13.4.9 y 13.4.10). El byte dura 8 periodos y el ACK uno.
//!
//! Tiempos (aproximados: no medidos en el chip): el reloj de SCL es Fosc/(4*(SSPADD+1)), o sea un periodo de SSPADD+1 ciclos de instruccion; un
//! START, START repetido o STOP dura un periodo y un byte con su ACK, nueve. Sin arbitraje, colision de bus ni modo esclavo.

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
    /// Un byte que el maestro recibio y si respondio ACK (`false` = NACK, el ultimo).
    Received {
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
    Receive,
    Acknowledge,
}

#[derive(Clone, Debug, Default)]
pub struct I2cBus {
    /// Direcciones de 7 bits de los esclavos que responden ACK.
    pub slaves: Vec<u8>,
    /// Los bytes que entregan los esclavos en las lecturas, en orden; vacia, se lee `0xFF` (SDA suelta).
    pub read_data: std::collections::VecDeque<u8>,
    pub events: Vec<I2cEvent>,
    /// Reglas del datasheet que el programa incumplio.
    pub violations: Vec<String>,
    busy: Option<(Op, u64)>,
    /// El siguiente byte es una direccion (justo despues de un START).
    expect_address: bool,
    /// El esclavo direccionado en la transaccion en curso respondio ACK.
    selected: bool,
    /// El esclavo esta en una lectura y entregara el siguiente byte.
    reading: bool,
    /// El byte recibido que espera el ACK o NACK del maestro.
    unanswered: Option<u8>,
}

impl I2cBus {
    pub fn reset(&mut self) {
        self.busy = None;
        self.expect_address = false;
        self.selected = false;
        self.reading = false;
        self.unanswered = None;
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
                I2cEvent::Data { .. } | I2cEvent::Received { .. } => {}
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
        let requested = ram[SSPCON2] & 0x1F & !old;
        if requested == 0 {
            // el programa no puede bajar un bit de operacion en curso: lo conserva el hardware
            ram[SSPCON2] = (ram[SSPCON2] & !0x1F) | (old & 0x1F);
            return;
        }
        if !self.enabled(ram) {
            self.violations.push("SEN, RSEN, PEN, RCEN o ACKEN con el MSSP apagado o fuera del modo I2C maestro (SSPEN = 0 o SSPM distinto de 1000): falta I2C.start".into());
            ram[SSPCON2] &= !0x1F;
            return;
        }
        if !self.idle_before(ram, old) {
            ram[SSPCON2] = (ram[SSPCON2] & !0x1F) | (old & 0x1F);
            self.violations.push("evento de bus pedido con el MSSP ocupado: no se activa (no hay cola de eventos, p.189)".into());
            return;
        }
        // un solo evento a la vez: si el programa pide varios, vale el de menor peso
        let bit: u8 = 1 << requested.trailing_zeros();
        let op = match bit {
            0x01 => Op::Start,
            0x02 => Op::Repeated,
            0x04 => Op::Stop,
            0x08 => Op::Receive,
            _ => Op::Acknowledge,
        };
        ram[SSPCON2] = (ram[SSPCON2] & !0x1F) | bit;
        match op {
            Op::Receive if !self.reading || self.unanswered.is_some() => {
                self.violations.push("RCEN sin una lectura abierta (direccion con R/W = 1 y ACK, y el byte anterior respondido)".into());
            }
            Op::Acknowledge if self.unanswered.is_none() => self.violations.push("ACKEN sin un byte recibido que responder".into()),
            Op::Stop | Op::Repeated if self.reading || self.unanswered.is_some() => {
                self.violations.push("STOP o START repetido sin responder NACK al ultimo byte: el esclavo aun puede retener SDA".into());
            }
            _ => {}
        }
        let periods = if op == Op::Receive { 8 } else { 1 };
        self.busy = Some((op, now + periods * Self::period_cycles(ram)));
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
                self.reading = false;
                self.unanswered = None;
            }
            Op::Stop => {
                self.events.push(I2cEvent::Stop);
                ram[SSPCON2] &= !0x04;
                ram[SSPSTAT] = (ram[SSPSTAT] & !0x08) | 0x10; // P = 1, S = 0
                self.expect_address = false;
                self.selected = false;
                self.reading = false;
                self.unanswered = None;
            }
            Op::Receive => {
                let byte = if self.reading { self.read_data.pop_front().unwrap_or(0xFF) } else { 0xFF };
                ram[SSPBUF] = byte;
                ram[SSPSTAT] |= 0x01; // BF; leer SSPBUF lo baja
                ram[SSPCON2] &= !0x08;
                self.unanswered = Some(byte);
            }
            Op::Acknowledge => {
                let ack = ram[SSPCON2] & 0x20 == 0;
                if let Some(byte) = self.unanswered.take() {
                    self.events.push(I2cEvent::Received { byte, ack });
                }
                self.reading &= ack;
                ram[SSPCON2] &= !0x10;
            }
            Op::Byte(byte) => {
                ram[SSPSTAT] &= !0x05; // R/W y BF en 0
                let ack = if self.expect_address {
                    let (addr, read) = (byte >> 1, byte & 1 != 0);
                    let ack = self.slaves.contains(&addr);
                    self.events.push(I2cEvent::Address { addr, read, ack });
                    self.expect_address = false;
                    self.selected = ack && !read;
                    self.reading = ack && read;
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
