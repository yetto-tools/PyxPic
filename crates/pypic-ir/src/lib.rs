//! IR tipada de PyPIC: independiente del destino, sin runtime Python.
//!
//! Frontend (`Program` tipado) -> `lower` -> `Module` -> `verify` -> backends.

pub mod interp;
pub mod ir;
pub mod lower;
pub mod print;
pub mod verify;

pub use interp::{Machine, Trap};
pub use ir::Module;
pub use lower::lower;
pub use print::print;
pub use verify::verify;
