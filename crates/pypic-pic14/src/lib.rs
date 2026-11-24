//! Backend PIC14 de PyPIC (PIC16 clasica). Todo corre en el host.
//!
//! Hoy: modelo del conjunto de instrucciones, codificador/decodificador de
//! palabras de 14 bits y desensamblador. Verificado contra DS40001291H p.227.

pub mod isa;

pub use isa::{disassemble, Cycles, Dest, EncodeError, Flag, Inst};
