//! Interprete de referencia de la IR (solo host, para tests).
//!
//! Sirve de oraculo de semantica: los backends deben producir el mismo
//! resultado observable. NO es un interprete de Python ni se ejecuta en el MCU
//! (ver docs/spec/00_PRINCIPIOS_NO_INTERPRETE.md): interpreta la IR ya compilada.

use crate::ir::*;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Trap {
    StepLimit,
    NoSuchFunction(String),
    BadArgs,
    /// Las funciones `@asm_function` solo existen en el dispositivo: el interprete de referencia no las ejecuta.
    AsmFunction(String),
    /// Los temporizadores son hardware: el interprete de referencia no los modela (se prueban en el simulador del nucleo).
    Timer(String),
    /// Se leyo la UART sin ningun byte recibido: el modelo de referencia no puede esperar (en el chip `read` se quedaria sondeando).
    UartEmpty,
}

pub struct Machine<'m> {
    module: &'m Module,
    pub globals: Vec<i128>,
    steps_left: u64,
    /// Estado de cada pin (en el orden de `module.pins`): latch, direccion y entrada externa.
    pin_latch: Vec<bool>,
    pin_out: Vec<bool>,
    pin_input: Vec<bool>,
    pin_last: Vec<Option<bool>>,
    /// (pin, nivel) cada vez que un pin excitado cambia de estado (incluye el paso de entrada a salida).
    pub pin_events: Vec<(usize, bool)>,
    /// Esperas ejecutadas (unidad, cantidad) en orden; el interprete no mide tiempo, solo las registra.
    pub delays: Vec<(DelayUnit, i128)>,
    /// Registros tocados por el programa, por direccion de 9 bits (0 al empezar; el interprete no modela efectos de hardware).
    pub regs: std::collections::BTreeMap<u16, u8>,
    /// La bandera C que modelan los intrinsecos de `pypic.bits`.
    pub carry: bool,
    /// El ADC de referencia: el valor (0..=1023) que lee cada canal; lo fija la prueba con `set_adc`.
    pub adc_inputs: [i128; 16],
    adc_channel: u8,
    adc_result: i128,
    /// La UART de referencia: lo que el programa envio y lo que se le va a dar a leer (la prueba lo carga con `uart_input`).
    pub uart_tx: Vec<u8>,
    pub uart_rx: std::collections::VecDeque<u8>,
    /// Los caracteres de 9 bits (`write9` y `read9`): lo que el programa envio y lo que se le va a dar a leer.
    pub uart_tx9: Vec<u16>,
    pub uart_rx9: std::collections::VecDeque<u16>,
    /// Los arreglos de RAM (todo en cero al empezar) y los accesos con un indice fuera de la capacidad (el indice se satura a la
    /// ultima posicion, igual que en el PIC): el intérprete los anota para que una prueba los vea; el simulador del PIC no puede.
    pub arrays: Vec<Vec<u8>>,
    pub array_oob: Vec<String>,
}

struct Frame {
    locals: Vec<i128>,
    temps: Vec<i128>,
}

impl<'m> Machine<'m> {
    pub fn new(module: &'m Module) -> Self {
        let n = module.pins.len();
        Machine {
            module,
            globals: module.globals.iter().map(|g| g.init).collect(),
            steps_left: 10_000_000,
            pin_latch: vec![false; n],
            pin_out: vec![false; n],
            pin_input: vec![false; n],
            pin_last: vec![None; n],
            pin_events: Vec::new(),
            delays: Vec::new(),
            regs: Default::default(),
            carry: false,
            adc_inputs: [0; 16],
            adc_channel: 0,
            adc_result: 0,
            uart_tx: Vec::new(),
            uart_rx: Default::default(),
            uart_tx9: Vec::new(),
            uart_rx9: Default::default(),
            arrays: module.arrays.iter().map(|(_, n)| vec![0; *n]).collect(),
            array_oob: Vec::new(),
        }
    }

    /// El indice efectivo de un acceso a un arreglo: saturado a la ultima posicion, anotando el acceso fuera de rango.
    fn array_index(&mut self, arr: usize, raw: i128) -> usize {
        let n = self.arrays[arr].len();
        if raw < 0 || raw as usize >= n {
            self.array_oob.push(format!("{}[{}] con {} elementos", self.module.arrays[arr].0, raw, n));
            return n - 1;
        }
        raw as usize
    }

    /// Nivel externo de un pin configurado como entrada.
    pub fn set_input(&mut self, pin: &str, level: bool) {
        if let Some(i) = self.module.pins.iter().position(|p| p.name == pin) {
            self.pin_input[i] = level;
        }
    }

    /// Nivel que excita un pin (`None` si es entrada).
    pub fn pin_level(&self, pin: &str) -> Option<bool> {
        let i = self.module.pins.iter().position(|p| p.name == pin)?;
        self.pin_out[i].then_some(self.pin_latch[i])
    }

    fn refresh_pin(&mut self, pin: usize) {
        let now = self.pin_out[pin].then_some(self.pin_latch[pin]);
        if now != self.pin_last[pin] {
            if let Some(level) = now {
                self.pin_events.push((pin, level));
            }
            self.pin_last[pin] = now;
        }
    }

    pub fn with_step_limit(mut self, n: u64) -> Self {
        self.steps_left = n;
        self
    }

    pub fn global(&self, name: &str) -> Option<i128> {
        self.module.globals.iter().position(|g| g.name == name).map(|i| self.globals[i])
    }

    /// Arranque completo: la funcion `@init_device` (si existe) y despues `main`, como el codigo de arranque del PIC.
    pub fn run_main(&mut self) -> Result<Option<i128>, Trap> {
        let inits: Vec<usize> = self.module.functions.iter().enumerate().filter(|(_, f)| f.is_init).map(|(i, _)| i).collect();
        for i in inits {
            self.run(i, &[])?;
        }
        self.call("main", &[])
    }

    pub fn call(&mut self, name: &str, args: &[i128]) -> Result<Option<i128>, Trap> {
        let idx = self.module.functions.iter().position(|f| f.name == name).ok_or_else(|| Trap::NoSuchFunction(name.into()))?;
        self.run(idx, args)
    }

    fn run(&mut self, fi: usize, args: &[i128]) -> Result<Option<i128>, Trap> {
        let f = &self.module.functions[fi];
        if f.asm.is_some() {
            return Err(Trap::AsmFunction(f.name.clone()));
        }
        if args.len() != f.num_params {
            return Err(Trap::BadArgs);
        }
        let mut fr = Frame { locals: vec![0; f.locals.len()], temps: vec![0; f.temps.len()] };
        for (i, a) in args.iter().enumerate() {
            fr.locals[i] = f.locals[i].ty.wrap(*a);
        }
        let mut bb = 0;
        loop {
            let block = &f.blocks[bb];
            for inst in &block.insts {
                self.tick()?;
                self.exec(f, &mut fr, inst)?;
            }
            self.tick()?;
            match &block.term {
                Terminator::Jump(t) => bb = *t,
                Terminator::Branch { cond, then_bb, else_bb } => {
                    bb = if self.read(&fr, cond) != 0 { *then_bb } else { *else_bb };
                }
                Terminator::Return(v) => return Ok(v.as_ref().map(|o| self.read(&fr, o))),
                Terminator::Unreachable => panic!("la IR llego a un 'unreachable' en {}", f.name),
            }
        }
    }

    fn tick(&mut self) -> Result<(), Trap> {
        if self.steps_left == 0 {
            return Err(Trap::StepLimit);
        }
        self.steps_left -= 1;
        Ok(())
    }

    fn read(&self, fr: &Frame, o: &Operand) -> i128 {
        match o {
            Operand::Const { value, .. } => *value,
            Operand::Place(Place::Temp(i)) => fr.temps[*i as usize],
            Operand::Place(Place::Local(i)) => fr.locals[*i as usize],
            Operand::Place(Place::Global(i)) => self.globals[*i as usize],
        }
    }

    fn write(&mut self, f: &Function, fr: &mut Frame, p: Place, v: i128) {
        let v = crate::eval::stored(self.module.place_ty(f, p), v);
        match p {
            Place::Temp(i) => fr.temps[i as usize] = v,
            Place::Local(i) => fr.locals[i as usize] = v,
            Place::Global(i) => self.globals[i as usize] = v,
        }
    }

    fn exec(&mut self, f: &Function, fr: &mut Frame, inst: &Inst) -> Result<(), Trap> {
        match inst {
            Inst::Copy { dst, src } => {
                let v = self.read(fr, src);
                self.write(f, fr, *dst, v);
            }
            Inst::Unary { dst, op, src } => {
                let v = self.read(fr, src);
                self.write(f, fr, *dst, crate::eval::unary(*op, v));
            }
            Inst::Binary { dst, op, lhs, rhs } => {
                let (a, b) = (self.read(fr, lhs), self.read(fr, rhs));
                self.write(f, fr, *dst, crate::eval::binary(*op, a, b));
            }
            Inst::Compare { dst, op, lhs, rhs } => {
                let (a, b) = (self.read(fr, lhs), self.read(fr, rhs));
                self.write(f, fr, *dst, crate::eval::compare(*op, a, b) as i128);
            }
            Inst::Cast { dst, src } => {
                let v = self.read(fr, src);
                self.write(f, fr, *dst, v);
            }
            Inst::Uart { call, dst, arg } => match call.op {
                UartOp::Start(_) | UartOp::ClearOverrun => {}
                UartOp::Write | UartOp::Send => {
                    let v = self.read(fr, arg.as_ref().unwrap());
                    self.uart_tx.push(v as u8);
                }
                UartOp::Read | UartOp::Recv => {
                    let b = self.uart_rx.pop_front().ok_or(Trap::UartEmpty)?;
                    self.write(f, fr, dst.unwrap(), b as i128);
                }
                UartOp::TxReady => self.write(f, fr, dst.unwrap(), 1),
                UartOp::WriteText(_) | UartOp::WriteFmt | UartOp::ReadLine(_) | UartOp::WriteBuf(_) => unreachable!("la IR desarma write_text en un bucle"),
                UartOp::Write9 | UartOp::Send9 => {
                    let v = self.read(fr, arg.as_ref().unwrap());
                    self.uart_tx9.push((v & 0x1FF) as u16);
                }
                UartOp::Read9 | UartOp::Recv9 => {
                    let b = self.uart_rx9.pop_front().ok_or(Trap::UartEmpty)?;
                    self.write(f, fr, dst.unwrap(), b as i128);
                }
                UartOp::RxReady => self.write(f, fr, dst.unwrap(), (!self.uart_rx.is_empty() || !self.uart_rx9.is_empty()) as i128),
                UartOp::Sending | UartOp::Overrun | UartOp::FramingError => self.write(f, fr, dst.unwrap(), 0),
            },
            Inst::Adc { call, dst } => match call.op {
                AdcOp::Start(_) => {}
                AdcOp::Select(n) => self.adc_channel = n,
                // la conversion es instantanea en el modelo de referencia: go captura la entrada del canal elegido
                AdcOp::Go => self.adc_result = self.adc_inputs[self.adc_channel as usize],
                AdcOp::Done => self.write(f, fr, dst.unwrap(), 1),
                AdcOp::Result => self.write(f, fr, dst.unwrap(), self.adc_result),
                AdcOp::Read(n) => {
                    self.adc_channel = n;
                    self.adc_result = self.adc_inputs[n as usize];
                    self.write(f, fr, dst.unwrap(), self.adc_result);
                }
            },
            Inst::ArrGet { dst, arr, index } => {
                let i = self.array_index(*arr, self.read(fr, index));
                let v = self.arrays[*arr][i];
                self.write(f, fr, *dst, v as i128);
            }
            Inst::ArrSet { arr, index, value } => {
                let i = self.array_index(*arr, self.read(fr, index));
                let v = self.read(fr, value);
                self.arrays[*arr][i] = v as u8;
            }
            Inst::RomGet { dst, rom, index } => {
                let t = &self.module.roms[*rom].1;
                let i = (self.read(fr, index) as usize).min(t.len() - 1);
                self.write(f, fr, *dst, t[i] as i128);
            }
            Inst::Bits { op, dst, src } => {
                let v = src.as_ref().map(|s| self.read(fr, s));
                match op {
                    BitsOp::Swap => self.write(f, fr, dst.unwrap(), ((v.unwrap() as u8).rotate_left(4)) as i128),
                    BitsOp::RolC | BitsOp::RorC => {
                        let b = v.unwrap() as u8;
                        let (new, out) = if *op == BitsOp::RolC { ((b << 1) | self.carry as u8, b >> 7 & 1) } else { ((b >> 1) | (self.carry as u8) << 7, b & 1) };
                        self.carry = out == 1;
                        if let Some(d) = dst {
                            self.write(f, fr, *d, new as i128);
                        } else if let Some(Operand::Place(p)) = src {
                            self.write(f, fr, *p, new as i128);
                        }
                    }
                    BitsOp::ClearCarry => self.carry = false,
                    BitsOp::SetCarry => self.carry = true,
                    BitsOp::Carry => self.write(f, fr, dst.unwrap(), self.carry as i128),
                    BitsOp::BitSet(n) | BitsOp::BitClear(n) => {
                        let Some(Operand::Place(p)) = src else { unreachable!() };
                        let cur = v.unwrap();
                        self.write(f, fr, *p, if matches!(op, BitsOp::BitSet(_)) { cur | (1i128 << n) } else { cur & !(1i128 << n) });
                    }
                    BitsOp::BitTest(n) => self.write(f, fr, dst.unwrap(), (v.unwrap() >> n & 1) as i128),
                }
            }
            Inst::Reg { call, dst, arg } => {
                let cur = self.regs.get(&call.addr).copied().unwrap_or(0);
                let mask = call.bit.map_or(0, |b| 1u8 << b);
                match call.op {
                    RegOp::Read => self.write(f, fr, dst.unwrap(), cur as i128),
                    RegOp::Value => self.write(f, fr, dst.unwrap(), (cur & mask != 0) as i128),
                    RegOp::Write => {
                        let v = self.read(fr, arg.as_ref().unwrap());
                        self.regs.insert(call.addr, v as u8);
                    }
                    RegOp::Set => {
                        self.regs.insert(call.addr, cur | mask);
                    }
                    RegOp::Clear => {
                        self.regs.insert(call.addr, cur & !mask);
                    }
                    RegOp::Toggle => {
                        self.regs.insert(call.addr, cur ^ mask);
                    }
                }
            }
            Inst::Timer { call, .. } => return Err(Trap::Timer(call.timer.name().to_string())),
            Inst::Delay { unit, amount } => {
                let v = self.read(fr, amount);
                self.delays.push((*unit, v));
            }
            Inst::PinCfg { pin, output } => {
                self.pin_out[*pin] = *output;
                self.refresh_pin(*pin);
            }
            Inst::PinSet { pin, value } => {
                self.pin_latch[*pin] = self.read(fr, value) != 0;
                self.refresh_pin(*pin);
            }
            Inst::PinToggle { pin } => {
                self.pin_latch[*pin] = !self.pin_latch[*pin];
                self.refresh_pin(*pin);
            }
            Inst::PinGet { dst, pin } => {
                let v = if self.pin_out[*pin] { self.pin_latch[*pin] } else { self.pin_input[*pin] };
                self.write(f, fr, *dst, v as i128);
            }
            Inst::Call { dst, func, args } => {
                let a: Vec<i128> = args.iter().map(|o| self.read(fr, o)).collect();
                let r = self.run(*func, &a)?;
                if let (Some(d), Some(v)) = (dst, r) {
                    self.write(f, fr, *d, v);
                }
            }
        }
        Ok(())
    }
}
