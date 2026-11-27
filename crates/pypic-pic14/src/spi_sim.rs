//! El MSSP en modo SPI maestro, para el simulador del nucleo (docs/sdk-spi.md).
//!
//! Fuente: DS40001291H cap. 13, pp. 176-181. Modelado:
//!  * con `SSPEN` = 1 y `SSPM` = 0000, 0001 o 0010, escribir SSPBUF intercambia 8 bits; al terminar, SSPBUF recibe el byte de SDI y suben
//!    `BF` (SSPSTAT<0>) y `SSPIF` (PIR1<3>); leer SSPBUF baja `BF`;
//!  * escribir SSPBUF durante un intercambio sube `WCOL` (SSPCON<7>) y se ignora (p. 179).
//!
//! Tiempos (aproximados: no medidos en el chip): un bit dura 1, 4 o 16 ciclos de instruccion (FOSC/4, /16, /64). No se modelan las formas
//! de onda (CKP, CKE, SMP), el modo esclavo ni el reloj de Timer2.

use std::collections::VecDeque;

const SSPBUF: usize = 0x13;
const SSPCON: usize = 0x14;
const PIR1: usize = 0x0C;
const TRISC: usize = 0x87;
const SSPSTAT: usize = 0x94;
const SSPIF: u8 = 0x08;

#[derive(Clone, Debug, Default)]
pub struct SpiBus {
    /// Los bytes que el esclavo pondra en SDI, uno por intercambio; vacia, SDI se lee como `0xFF`.
    pub miso: VecDeque<u8>,
    /// SDO unido a SDI: se recibe lo mismo que se envia (tiene prioridad sobre `miso`).
    pub loopback: bool,
    /// Los bytes que el maestro envio, en orden.
    pub mosi: Vec<u8>,
    /// Reglas del datasheet que el programa incumplio.
    pub violations: Vec<String>,
    busy: Option<(u8, u64)>,
}

impl SpiBus {
    pub fn reset(&mut self) {
        self.busy = None;
    }

    /// El MSSP esta encendido en un modo SPI maestro que se modela.
    pub fn enabled(ram: &[u8]) -> bool {
        ram[SSPCON] & 0x20 != 0 && ram[SSPCON] & 0x0F <= 2
    }

    /// El programa escribio SSPBUF (`old` es lo que tenia).
    pub fn sspbuf_write(&mut self, ram: &mut [u8; 512], old: u8, now: u64) {
        if self.busy.is_some() {
            ram[SSPCON] |= 0x80; // WCOL
            ram[SSPBUF] = old;
            self.violations.push("SSPBUF escrito durante un intercambio SPI: se ignora y sube WCOL (p. 179)".into());
            return;
        }
        if ram[TRISC] & 0x28 != 0 {
            self.violations.push("SCK (RC3) y SDO (RC5) deben ser salidas en SPI maestro (13.3.2)".into());
        }
        let byte = ram[SSPBUF];
        let bit_cycles = 1u64 << (2 * (ram[SSPCON] & 0x03));
        self.busy = Some((byte, now + 8 * bit_cycles));
    }

    /// Una instruccion leyo SSPBUF.
    pub fn sspbuf_read(&mut self, ram: &mut [u8; 512]) {
        ram[SSPSTAT] &= !0x01; // BF
    }

    pub fn tick(&mut self, ram: &mut [u8; 512], now: u64) {
        let Some((sent, done)) = self.busy else { return };
        if now < done {
            return;
        }
        self.busy = None;
        self.mosi.push(sent);
        ram[SSPBUF] = if self.loopback { sent } else { self.miso.pop_front().unwrap_or(0xFF) };
        ram[SSPSTAT] |= 0x01;
        ram[PIR1] |= SSPIF;
    }
}
