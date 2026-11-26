//! Programas de muestra hechos a mano para poner en marcha simulador y hardware.
//!
//! No son salida del compilador: se reemplazaran por ensamblador/compilador cuando existan.

use crate::hex::Image;
use crate::isa::{Dest::F, Inst::*};

/// Direcciones de palabra de la palabra de configuracion (PIC16F887, DS40001291H p.206-207).
pub const CONFIG1: u32 = 0x2007;
pub const CONFIG2: u32 = 0x2008;

/// CONFIG1 usado por `blink_rd0`: INTOSCIO (FOSC=100), WDT apagado, PWRT desactivado,
/// RE3 como entrada digital (MCLRE=0, MCLR interno), sin proteccion de codigo/datos,
/// BOR apagado, sin monitor de reloj ni cambio de oscilador, RB3 digital (LVP=0),
/// depurador desactivado (DEBUG=1).
pub const BLINK_CONFIG1: u16 = 0x20D4;
/// CONFIG2: sin proteccion de escritura (WRT=11), BOR a 4.0 V; todo a 1 (sin programar).
pub const BLINK_CONFIG2: u16 = 0x3FFF;

/// LED en RD0 (pin 19 del PDIP-40), oscilador interno a 4 MHz (OSCCON arranca en IRCF=110):
/// 1 MHz de ciclo de instruccion. Semiperiodo de unos 0.59 s (192 K iteraciones de una
/// triple cuenta), es decir un parpadeo de ~1.2 s.
///
/// Registros (DS40001291H p.27-28): STATUS=03h (RP0=bit 5), PORTD=08h (banco 0), TRISD=88h (banco 1).
pub fn blink_rd0() -> Image {
    const STATUS: u8 = 0x03;
    const PORTD: u8 = 0x08; // en el banco 1 el mismo desplazamiento (0x08) es TRISD (88h)
    const C1: u8 = 0x20; // contadores en RAM de propósito general, banco 0
    const C2: u8 = 0x21;
    const C3: u8 = 0x22;
    const RP0: u8 = 5;

    let program: &[(u32, crate::isa::Inst)] = &[
        (0x0000, Goto(0x0005)), // vector de reset
        (0x0004, Retfie),       // vector de interrupcion (no se habilitan interrupciones)
        // main
        (0x0005, Bsf(STATUS, RP0)),
        (0x0006, Bcf(PORTD, 0)), // TRISD<0> = 0: RD0 como salida
        (0x0007, Bcf(STATUS, RP0)),
        (0x0008, Bcf(PORTD, 0)), // LED apagado al inicio
        // loop
        (0x0009, Bsf(PORTD, 0)),
        (0x000A, Call(0x0010)),
        (0x000B, Bcf(PORTD, 0)),
        (0x000C, Call(0x0010)),
        (0x000D, Goto(0x0009)),
        // delay: 3 x 256 x 256 iteraciones
        (0x0010, Movlw(3)),
        (0x0011, Movwf(C3)),
        (0x0012, Clrf(C2)),      // d3
        (0x0013, Clrf(C1)),      // d2
        (0x0014, Decfsz(C1, F)), // d1
        (0x0015, Goto(0x0014)),
        (0x0016, Decfsz(C2, F)),
        (0x0017, Goto(0x0013)),
        (0x0018, Decfsz(C3, F)),
        (0x0019, Goto(0x0012)),
        (0x001A, Return),
    ];

    let mut img = Image::new();
    for &(addr, inst) in program {
        img.set(addr, inst.encode().expect("operandos validos")).expect("sin solapes");
    }
    img.set(CONFIG1, BLINK_CONFIG1).unwrap();
    img.set(CONFIG2, BLINK_CONFIG2).unwrap();
    img
}
