//! IR tipada de PyxPic: independiente del destino, sin runtime Python.
//!
//! Frontend (`Program` tipado) -> `lower` -> `Module` -> `verify` -> backends.

pub mod eval;
pub mod interp;
pub mod ir;
pub mod lower;
pub mod opt;
pub mod print;
pub mod verify;

pub use interp::{Machine, Trap};
pub use ir::Module;
pub use lower::{lower, lower_unoptimized};
pub use print::print;
pub use verify::verify;
