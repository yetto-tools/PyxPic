//! Backend PIC14 de PyPIC (PIC16 clasica). Todo corre en el host.
//!
//! Hoy: modelo del conjunto de instrucciones, codificador/decodificador de
//! palabras de 14 bits, desensamblador (verificado contra DS40001291H p.227) y
//! escritor/lector de Intel HEX.

// Los generadores de codigo recorren varios arreglos por indice a la vez (bytes de origen y destino): un iterador no lo hace mas claro.
#![allow(clippy::needless_range_loop)]

pub mod asm;
pub mod codegen;
pub mod hex;
pub mod i2c_sim;
pub mod isa;
pub mod samples;
pub mod sim;
pub mod spi_sim;

pub use asm::{assemble, assemble_fragment, assemble_with, declared_processor, render_errors, xc_inc, AsmEnv, AsmError, Assembled, EnvReg, FragItem, Fragment};
pub use codegen::{compile, CodegenError, Compiled, DelayReport, Options};
pub use hex::{HexError, Image};
pub use isa::{disassemble, Cycles, Dest, EncodeError, Flag, Inst};
