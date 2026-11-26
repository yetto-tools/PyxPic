//! IR tipada e independiente del destino.
//!
//! Modelo de memoria: no hay pila de datos. Cada local/parametro es un slot
//! estatico (el backend decide su direccion y banco). Los temporales son
//! registros virtuales que el backend asigna.

pub use pypic_frontend::ast::{BinOp, CmpOp};
pub use pypic_frontend::{
    AdcCall, AdcOp, BitsOp, CmpId, CmpRef, CompCall, CompOp, CvrefOp, DelayUnit, EepromCall, EepromOp, PwmCall, PwmId, PwmOp, PwmTiming, RegCall, RegOp, TimerCall, TimerId, TimerOp, TimerStart, Type,
    UartCall, UartOp,
};

pub type BlockId = usize;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnaryOp {
    Neg,
    Invert,
    Not,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Place {
    Temp(u32),
    Local(u32),
    Global(u32),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Operand {
    Const { value: i128, ty: Type },
    Place(Place),
}

#[derive(Clone, Debug, PartialEq)]
pub enum Inst {
    Copy {
        dst: Place,
        src: Operand,
    },
    /// El tipo de la operacion es el de `dst`.
    Unary {
        dst: Place,
        op: UnaryOp,
        src: Operand,
    },
    /// Aritmetica modular en el tipo de `dst`. En desplazamientos `rhs` es un entero sin signo.
    Binary {
        dst: Place,
        op: BinOp,
        lhs: Operand,
        rhs: Operand,
    },
    /// `dst` es bool; `lhs` y `rhs` tienen el mismo tipo.
    Compare {
        dst: Place,
        op: CmpOp,
        lhs: Operand,
        rhs: Operand,
    },
    /// Conversion explicita (trunca/extiende) del tipo de `src` al de `dst`.
    Cast {
        dst: Place,
        src: Operand,
    },
    Call {
        dst: Option<Place>,
        func: usize,
        args: Vec<Operand>,
    },
    /// `RD0.output()` / `RD0.input()`: direccion del pin (y pasarlo a digital si era analogico).
    PinCfg {
        pin: usize,
        output: bool,
    },
    /// `RD0.on()/off()/value(x)`: escribe el latch del pin (`value` es bool o u8; distinto de 0 = alto).
    PinSet {
        pin: usize,
        value: Operand,
    },
    PinToggle {
        pin: usize,
    },
    /// `RD0.value()`: lee el nivel del pin como u8 (0 o 1).
    PinGet {
        dst: Place,
        pin: usize,
    },
    /// Espera activa de `amount` en la unidad dada. Con `Cycles` y `Us` la cantidad es una constante; con `Ms` y `S`, una
    /// constante o un u16. El backend la convierte en ciclos de instruccion exactos segun el reloj.
    Delay {
        unit: DelayUnit,
        amount: Operand,
    },
    /// Operacion sobre un temporizador (`TMR2.start(...)`, `TMR1.value()`...). `dst` recibe el resultado de `Value`
    /// (u8/u16) y `Expired` (bool); `arg` es el valor de `Set`.
    Timer {
        call: TimerCall,
        dst: Option<Place>,
        arg: Option<Operand>,
    },
    /// Acceso a un registro (`STATUS.read()`, `OPTION_REG.write(v)`, `STATUS.Z.set()`...). `dst` recibe el resultado de
    /// `Read` y `Value` (u8); `arg` es el valor de `Write`.
    Reg {
        call: RegCall,
        dst: Option<Place>,
        arg: Option<Operand>,
    },
    /// Intrinseco de `pypic.bits`. `src` es la variable (o el valor de `Swap`); `dst` recibe el resultado de `Swap`, `RolC`,
    /// `RorC` (u8), `Carry` y `BitTest` (bool). `BitSet`/`BitClear` modifican `src` en su sitio.
    Bits {
        op: BitsOp,
        dst: Option<Place>,
        src: Option<Operand>,
    },
    /// `dst = TABLA[index]`: lectura de una tabla constante de la memoria de programa. Un indice mayor que el ultimo
    /// devuelve la ultima entrada (el backend lo comprueba; la consulta cuesta lo mismo con cualquier indice).
    RomGet {
        dst: Place,
        rom: usize,
        index: Operand,
    },
    /// `dst = ARREGLO[index]`: un indice mayor que la capacidad se satura a la ultima posicion (comportamiento definido).
    ArrGet {
        dst: Place,
        arr: usize,
        index: Operand,
    },
    /// `ARREGLO[index] = value`, con el mismo indice saturado: nunca escribe fuera del arreglo.
    ArrSet {
        arr: usize,
        index: Operand,
        value: Operand,
    },
    /// Operacion del ADC (`ADC.start()`, `ADC.read(AN0)`...). `dst` recibe el resultado de `Done` (bool), `Result` y `Read` (u16).
    Adc {
        call: AdcCall,
        dst: Option<Place>,
    },
    /// Operacion de un modulo PWM (`PWM1.start(...)`, `PWM1.duty(v)`, `PWM1.stop()`). En `Duty`, `hi` es `CCPRxL` (los ocho bits
    /// altos del ciclo de trabajo) y `dc` es `DCxB` (los dos bajos, 0 a 3): el IR ya los separo, ambos u8.
    Pwm {
        call: PwmCall,
        hi: Option<Operand>,
        dc: Option<Operand>,
    },
    /// Operacion de la EEPROM de datos (`EEPROM.read(a)`, `EEPROM.write(a, v)`...). `addr` y `val` son u8; `dst` recibe el resultado
    /// de `Read` (u8) y de `Busy`/`Failed` (bool).
    Eeprom {
        call: EepromCall,
        dst: Option<Place>,
        addr: Option<Operand>,
        val: Option<Operand>,
    },
    /// Operacion de un comparador (`CMP1.start(...)`, `CMP1.out()`, `CMP1.stop()`) o de CVREF (`CVREF.start(step=8)`...). `dst` recibe el
    /// resultado de `Out` (bool).
    Comp {
        call: CompCall,
        dst: Option<Place>,
    },
    /// Operacion de la UART (`UART.start(baud=9600)`, `UART.write(x)`...). `arg` es el byte de `Write`/`Send`; `dst` recibe
    /// el resultado de `Read`/`Recv` (u8) y de las consultas de bandera (bool).
    Uart {
        call: UartCall,
        dst: Option<Place>,
        arg: Option<Operand>,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub enum Terminator {
    Jump(BlockId),
    Branch {
        cond: Operand,
        then_bb: BlockId,
        else_bb: BlockId,
    },
    Return(Option<Operand>),
    /// Fin de camino que el frontend garantiza inalcanzable.
    Unreachable,
}

#[derive(Clone, Debug)]
pub struct Block {
    pub insts: Vec<Inst>,
    pub term: Terminator,
}

#[derive(Clone, Debug)]
pub struct Local {
    pub name: String,
    pub ty: Type,
    pub is_param: bool,
}

/// Cuerpo de una funcion `@asm_function`: texto ensamblador que el backend ensambla en su sitio.
#[derive(Clone, Debug)]
pub struct AsmBlock {
    pub text: String,
    /// Linea del fuente donde empieza `text`, para ubicar los errores del ensamblador.
    pub first_line: usize,
}

#[derive(Clone, Debug)]
pub struct Function {
    pub name: String,
    pub is_interrupt: bool,
    /// Funcion `@init_device`: el arranque la ejecuta antes de `main`.
    pub is_init: bool,
    /// Los primeros `num_params` locales son los parametros, en orden.
    pub num_params: usize,
    pub ret: Type,
    pub locals: Vec<Local>,
    pub temps: Vec<Type>,
    /// El bloque 0 es la entrada.
    pub blocks: Vec<Block>,
    /// Funcion `@asm_function`: su cuerpo es ensamblador (y `blocks` solo tiene un bloque vacio).
    pub asm: Option<AsmBlock>,
}

#[derive(Clone, Debug)]
pub struct Global {
    pub name: String,
    pub ty: Type,
    pub init: i128,
    pub is_const: bool,
}

/// Pin de E/S usado por el programa (importado de `pypic.periph.gpio`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pin {
    pub name: String,
    pub port: char,
    pub bit: u8,
    /// Canal analogico `ANn`, si el pin lo tiene.
    pub analog: Option<u8>,
}

#[derive(Clone, Debug, Default)]
pub struct Module {
    pub globals: Vec<Global>,
    /// Adquisicion del ADC en nanosegundos (`ADC.start(acquisition_us=...)`; por defecto el valor tipico de 11,5 us).
    pub adc_acq_ns: u32,
    /// Tablas constantes: nombre y entradas (u8).
    pub roms: Vec<(String, Vec<u8>)>,
    /// Arreglos de RAM (`buf: uint8[16]`): nombre y capacidad. Empiezan en cero.
    pub arrays: Vec<(String, usize)>,
    /// Los valores iniciales de cada arreglo; vacio si empieza en cero.
    pub array_inits: Vec<Vec<u8>>,
    pub functions: Vec<Function>,
    pub pins: Vec<Pin>,
    /// Dispositivo declarado en el programa (`from pypic.device import PIC16F887`).
    pub device: Option<String>,
    /// Reloj efectivo (declarado o el del dispositivo por defecto).
    pub clock_hz: Option<u32>,
    pub clock_defaulted: bool,
    /// La fuente del reloj declarada con `CLOCK: Internal[...]` (y las demas): fija `FOSC` y `OSCCON`.
    pub clock_source: Option<pypic_frontend::ClockSource>,
}

impl Module {
    pub fn operand_ty(&self, f: &Function, op: &Operand) -> Type {
        match op {
            Operand::Const { ty, .. } => *ty,
            Operand::Place(p) => self.place_ty(f, *p),
        }
    }

    pub fn place_ty(&self, f: &Function, p: Place) -> Type {
        match p {
            Place::Temp(i) => f.temps[i as usize],
            Place::Local(i) => f.locals[i as usize].ty,
            Place::Global(i) => self.globals[i as usize].ty,
        }
    }
}
