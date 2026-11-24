//! Backend PIC14 de PyPIC (PIC16 clasica). Todo corre en el host.
//!
//! Hoy: modelo del conjunto de instrucciones, codificador/decodificador de
//! palabras de 14 bits, desensamblador (verificado contra DS40001291H p.227) y
//! escritor/lector de Intel HEX.

pub mod hex;
pub mod isa;
pub mod samples;
pub mod sim;

pub use hex::{HexError, Image};
pub use isa::{disassemble, Cycles, Dest, EncodeError, Flag, Inst};
