//! EEPROM de datos (`pypic.periph.eeprom`): tipos compartidos entre el checker, el IR y el backend.
//!
//! Fuente: DS40001291H cap. 10 (pp. 110-113, 119-120) y la tabla del apartado 17.5 (p.246, parametros D120-D124), leidas de la pagina del PDF:
//!  * 256 bytes de EEPROM de datos, direccion 0..=0xFF en `EEADR`; `EEPGD = 0` elige la memoria de datos;
//!  * lectura: direccion en `EEADR`, `RD = 1`, el dato esta en `EEDAT` en el ciclo siguiente;
//!  * escritura: `EEADR`, `EEDAT`, `WREN = 1` y la secuencia **55h, AAh a `EECON2` y `WR = 1`**, con las interrupciones inhabilitadas;
//!    una escritura borra y escribe el byte; `WR` baja por hardware al terminar, y `EEIF` (PIR2) sube (hay que bajarlo por programa);
//!  * `WRERR` sube si la escritura se interrumpio (MCLR, WDT o BOR);
//!  * D122 `TDEW` (borrado y escritura): **5 ms tipico, 6 ms maximo**; D120 resistencia 100 000 ciclos minimos por byte (1 000 000 tipico) a
//!    -40..85 C, 10 000 minimos a 85..125 C; D123 retencion 40 anos; D124 refresco: 1 000 000 de escrituras totales antes de refrescar.

/// Tamano de la EEPROM de datos del PIC16F887.
pub const SIZE: usize = 256;

/// TDEW maximo (D122) en nanosegundos: lo que el simulador tarda en terminar una escritura.
pub const WRITE_TIME_NS: u64 = 6_000_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EepromOp {
    /// `EEPROM.read(a)`: el byte de la direccion `a`.
    Read,
    /// `EEPROM.write(a, v)`: la secuencia completa y espera a que termine.
    Write,
    /// `EEPROM.start_write(a, v)`: la secuencia completa sin esperar; `busy()` dice cuando termino.
    StartWrite,
    /// `EEPROM.busy()`: hay una escritura en curso (`WR = 1`).
    Busy,
    /// `EEPROM.failed()`: la ultima escritura se interrumpio (`WRERR = 1`).
    Failed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EepromCall {
    pub op: EepromOp,
}
