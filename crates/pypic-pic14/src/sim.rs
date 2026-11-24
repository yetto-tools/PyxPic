//! Simulador del nucleo PIC14 (solo host, para tests): ejecuta las 35 instrucciones sobre una `Image`.
//!
//! Modela W, PC de 13 bits con PCLATH, pila de 8 niveles circular, STATUS (C, DC, Z, TO, PD, RP, IRP),
//! 4 bancos de RAM con los registros espejados y la zona comun 70h-7Fh, direccionamiento indirecto
//! (INDF/FSR/IRP) y ciclos. De los perifericos solo modela Timer0/1/2 con reloj interno (ver `tick_timers`);
//! PORTx/TRISx son registros planos y las interrupciones solo se fuerzan con `interrupt`.
//! Fuente de la semantica: DS40001291H seccion 2 y tabla 15-2 (p.227).

use crate::hex::Image;
use crate::isa::{Dest, Inst};

pub const STATUS: usize = 0x03;
const C: u8 = 0x01;
const DC: u8 = 0x02;
const Z: u8 = 0x04;
const PD: u8 = 0x08;
const TO: u8 = 0x10;
const RP0: u8 = 0x20;
const RP1: u8 = 0x40;
const IRP: u8 = 0x80;

#[derive(Clone, Debug)]
pub struct Sim {
    flash: Vec<u16>,
    ram: [u8; 512],
    pub w: u8,
    pub pc: u16,
    pclath: u8,
    stack: [u16; 8],
    sp: usize,
    pub cycles: u64,
    pub halted: bool,
    /// Palabra programada (las no programadas leen 0x3FFF = ADDLW 0xFF, como la flash borrada).
    pub flash_words: usize,
    /// Destino del PC si la instruccion en curso escribio PCL (salto calculado).
    pcl_write: Option<u16>,
    /// Estado oculto de los temporizadores: preescaladores, ciclos de inhibicion de TMR0 y postescalador de Timer2.
    t0_pre: u32,
    t0_inhibit: u32,
    t1_pre: u32,
    t2_pre: u32,
    t2_post: u32,
    /// Ciclo exacto en que cada temporizador activo su bandera ("T0IF", "TMR1IF", "TMR2IF"); se guardan los primeros 4096.
    pub timer_events: Vec<(&'static str, u64)>,
    /// El ADC: el valor de 10 bits que lee cada canal, la frecuencia del oscilador (da TAD y el ciclo de instruccion) y
    /// las reglas del datasheet que el programa incumplio (TAD fuera de 1,6..6,0 us, adquisicion menor de 5 us, GO con el ADC apagado).
    pub adc_inputs: [u16; 16],
    pub fosc_hz: u32,
    pub adc_violations: Vec<String>,
    adc_prev_go: bool,
    adc_prev_on: bool,
    adc_prev_chs: u8,
    adc_acq_from: u64,
    adc_remaining: u32,
}

impl Sim {
    pub fn new(img: &Image) -> Sim {
        let mut flash = vec![0x3FFF; 8192];
        for (a, w) in img.iter() {
            if (a as usize) < flash.len() {
                flash[a as usize] = w;
            }
        }
        let mut s = Sim { flash, ram: [0; 512], w: 0, pc: 0, pclath: 0, stack: [0; 8], sp: 0, cycles: 0, halted: false, flash_words: img.len(), pcl_write: None, t0_pre: 0, t0_inhibit: 0, t1_pre: 0, t2_pre: 0, t2_post: 0, timer_events: Vec::new(), adc_inputs: [0; 16], fosc_hz: 4_000_000, adc_violations: Vec::new(), adc_prev_go: false, adc_prev_on: false, adc_prev_chs: 0, adc_acq_from: 0, adc_remaining: 0 };
        s.reset();
        s
    }

    pub fn reset(&mut self) {
        self.ram = [0; 512];
        self.ram[STATUS] = TO | PD; // 0001 1xxx
        for tris in 0x85..=0x89 {
            self.ram[tris] = 0xFF; // TRISA..TRISE: entradas
        }
        self.ram[0x188] = 0xFF; // ANSEL: todo analogico tras el reset
        self.ram[0x189] = 0x3F; // ANSELH (--11 1111)
        self.ram[0x81] = 0xFF; // OPTION_REG
        self.ram[0x92] = 0xFF; // PR2
        (self.t0_pre, self.t0_inhibit, self.t1_pre, self.t2_pre, self.t2_post) = (0, 0, 0, 0, 0);
        (self.adc_prev_go, self.adc_prev_on, self.adc_prev_chs, self.adc_remaining) = (false, false, 0, 0);
        self.pc = 0;
        self.pclath = 0;
        self.sp = 0;
        self.halted = false;
    }

    // ---- memoria ------------------------------------------------------------------------
    /// Direccion fisica de una direccion de 9 bits (espejos y zona comun).
    fn phys(addr: usize) -> usize {
        let low = addr & 0x7F;
        match low {
            0x02 | 0x03 | 0x04 | 0x0A | 0x0B => low, // PCL, STATUS, FSR, PCLATH, INTCON en todos los bancos
            0x70..=0x7F => low,                      // RAM comun
            _ => addr & 0x1FF,
        }
    }

    fn bank_base(&self) -> usize {
        let st = self.ram[STATUS];
        (((st & RP1 != 0) as usize) << 8) | (((st & RP0 != 0) as usize) << 7)
    }

    /// Lee el registro `f` (7 bits) con el banco actual; f=0 es INDF.
    pub fn read_f(&self, f: u8) -> u8 {
        if f == 0 {
            return self.read_indirect();
        }
        match f {
            0x02 => ((self.pc + 1) & 0xFF) as u8, // PCL
            0x0A => self.pclath & 0x1F,
            _ => self.ram[Self::phys(self.bank_base() | f as usize)],
        }
    }

    fn indirect_addr(&self) -> usize {
        (((self.ram[STATUS] & IRP != 0) as usize) << 8) | self.ram[0x04] as usize
    }

    fn read_indirect(&self) -> u8 {
        let a = self.indirect_addr();
        if a & 0x7F == 0 { 0 } else { self.ram[Self::phys(a)] } // INDF indirecto lee 00h
    }

    pub fn write_f(&mut self, f: u8, v: u8) {
        if f == 0 {
            let a = self.indirect_addr();
            if a & 0x7F != 0 {
                self.ram[Self::phys(a)] = v;
            }
            return;
        }
        match f {
            0x02 => self.pcl_write = Some((self.pclath as u16 & 0x1F) << 8 | v as u16), // PCH sale de PCLATH<4:0>
            0x0A => self.pclath = v & 0x1F,
            _ => {
                let a = Self::phys(self.bank_base() | f as usize);
                self.ram[a] = v;
                match a & 0xFF {
                    // escribir TMR0 pone a cero su preescalador e inhibe el incremento durante 2 ciclos (p.76)
                    0x01 if a < 0x100 => self.t0_pre = 0,
                    0x0E | 0x0F if a < 0x100 => self.t1_pre = 0,
                    0x11 if a < 0x100 => self.t2_pre = 0,
                    _ => {}
                }
                if a == 0x01 {
                    self.t0_inhibit = 2;
                }
            }
        }
    }

    /// Acceso directo a la RAM por direccion de 9 bits (para tests e inspeccion).
    pub fn read(&self, addr: usize) -> u8 {
        self.ram[Self::phys(addr)]
    }

    pub fn write(&mut self, addr: usize, v: u8) {
        self.ram[Self::phys(addr)] = v;
    }

    /// Nivel del pin del puerto `port` ('A'..'E'), bit `bit`: `None` si es entrada (alta impedancia).
    pub fn pin(&self, port: char, bit: u8) -> Option<bool> {
        let off = 5 + (port as u8 - b'A') as usize; // PORTA=05h ... PORTE=09h
        let tris = self.ram[0x80 + off];
        (tris >> bit & 1 == 0).then(|| self.ram[off] >> bit & 1 == 1)
    }

    // ---- ejecucion ----------------------------------------------------------------------
    /// Fuerza una interrupcion entre dos instrucciones (los perifericos no se modelan): empuja el PC,
    /// salta a 0004h y desactiva GIE hasta el RETFIE. Despierta al nucleo si dormia.
    pub fn interrupt(&mut self) {
        self.push(self.pc);
        self.pc = 4;
        self.ram[0x0B] &= !0x80;
        self.halted = false;
        self.cycles += 3;
    }

    fn set_flag(&mut self, mask: u8, on: bool) {
        if on { self.ram[STATUS] |= mask } else { self.ram[STATUS] &= !mask }
    }

    fn zflag(&mut self, v: u8) {
        self.set_flag(Z, v == 0);
    }

    fn push(&mut self, addr: u16) {
        self.stack[self.sp] = addr;
        self.sp = (self.sp + 1) & 7;
    }

    fn pop(&mut self) -> u16 {
        self.sp = (self.sp + 7) & 7;
        self.stack[self.sp]
    }

    fn put(&mut self, f: u8, d: Dest, v: u8) {
        match d {
            Dest::W => self.w = v,
            Dest::F => self.write_f(f, v),
        }
    }

    /// Ejecuta una instruccion. Devuelve false si el nucleo esta dormido (SLEEP).
    pub fn step(&mut self) -> bool {
        if self.halted {
            return false;
        }
        let word = self.flash[self.pc as usize & 0x1FFF];
        let inst = Inst::decode(word).unwrap_or(Inst::Nop);
        self.pcl_write = None;
        let mut cyc = 1u64;
        let mut skip = false;
        let mut next = (self.pc + 1) & 0x1FFF;
        use Inst::*;
        match inst {
            Addwf(f, d) => {
                let (a, b) = (self.w, self.read_f(f));
                self.add_flags(a, b);
                self.put(f, d, a.wrapping_add(b));
            }
            Andwf(f, d) => {
                let r = self.w & self.read_f(f);
                self.zflag(r);
                self.put(f, d, r);
            }
            Comf(f, d) => {
                let r = !self.read_f(f);
                self.zflag(r);
                self.put(f, d, r);
            }
            Decf(f, d) => {
                let r = self.read_f(f).wrapping_sub(1);
                self.zflag(r);
                self.put(f, d, r);
            }
            Decfsz(f, d) => {
                let r = self.read_f(f).wrapping_sub(1);
                self.put(f, d, r);
                skip = r == 0;
            }
            Incf(f, d) => {
                let r = self.read_f(f).wrapping_add(1);
                self.zflag(r);
                self.put(f, d, r);
            }
            Incfsz(f, d) => {
                let r = self.read_f(f).wrapping_add(1);
                self.put(f, d, r);
                skip = r == 0;
            }
            Iorwf(f, d) => {
                let r = self.w | self.read_f(f);
                self.zflag(r);
                self.put(f, d, r);
            }
            Movf(f, d) => {
                let r = self.read_f(f);
                self.zflag(r);
                self.put(f, d, r);
            }
            Rlf(f, d) => {
                let v = self.read_f(f);
                let r = v << 1 | (self.ram[STATUS] & C);
                self.set_flag(C, v & 0x80 != 0);
                self.put(f, d, r);
            }
            Rrf(f, d) => {
                let v = self.read_f(f);
                let r = v >> 1 | (self.ram[STATUS] & C) << 7;
                self.set_flag(C, v & 1 != 0);
                self.put(f, d, r);
            }
            Subwf(f, d) => {
                let (a, b) = (self.read_f(f), self.w); // f - W
                self.sub_flags(a, b);
                self.put(f, d, a.wrapping_sub(b));
            }
            Swapf(f, d) => {
                let v = self.read_f(f);
                self.put(f, d, v.rotate_left(4));
            }
            Xorwf(f, d) => {
                let r = self.w ^ self.read_f(f);
                self.zflag(r);
                self.put(f, d, r);
            }
            Clrf(f) => {
                self.write_f(f, 0);
                self.set_flag(Z, true);
            }
            Movwf(f) => self.write_f(f, self.w),
            Bcf(f, b) => {
                let v = self.read_f(f) & !(1 << b);
                self.write_f(f, v);
            }
            Bsf(f, b) => {
                let v = self.read_f(f) | 1 << b;
                self.write_f(f, v);
            }
            Btfsc(f, b) => skip = self.read_f(f) >> b & 1 == 0,
            Btfss(f, b) => skip = self.read_f(f) >> b & 1 == 1,
            Addlw(k) => {
                let a = self.w;
                self.add_flags(a, k);
                self.w = a.wrapping_add(k);
            }
            Andlw(k) => {
                self.w &= k;
                self.zflag(self.w);
            }
            Iorlw(k) => {
                self.w |= k;
                self.zflag(self.w);
            }
            Xorlw(k) => {
                self.w ^= k;
                self.zflag(self.w);
            }
            Movlw(k) => self.w = k,
            Sublw(k) => {
                let a = self.w;
                self.sub_flags(k, a); // k - W
                self.w = k.wrapping_sub(a);
            }
            Retlw(k) => {
                self.w = k;
                next = self.pop();
                cyc = 2;
            }
            Return => {
                next = self.pop();
                cyc = 2;
            }
            Retfie => {
                next = self.pop();
                self.ram[0x0B] |= 0x80; // RETFIE reactiva GIE (INTCON<7>)
                cyc = 2;
            }
            Call(k) => {
                self.push((self.pc + 1) & 0x1FFF);
                next = ((self.pclath as u16 >> 3) & 3) << 11 | k;
                cyc = 2;
            }
            Goto(k) => {
                next = ((self.pclath as u16 >> 3) & 3) << 11 | k;
                cyc = 2;
            }
            Clrw => {
                self.w = 0;
                self.set_flag(Z, true);
            }
            Clrwdt => {
                self.set_flag(TO, true);
                self.set_flag(PD, true);
            }
            Nop => {}
            Sleep => {
                self.set_flag(TO, true);
                self.set_flag(PD, false);
                self.halted = true;
            }
        }
        if skip {
            next = (next + 1) & 0x1FFF;
            cyc = 2;
        }
        // escribir PCL cambia el PC: la instruccion cuesta 2 ciclos
        if let Some(target) = self.pcl_write.take() {
            next = target & 0x1FFF;
            cyc = 2;
        }
        self.pc = next;
        self.cycles += cyc;
        for i in 0..cyc {
            self.tick_timers(self.cycles - cyc + i + 1);
        }
        true
    }

    /// Un ciclo de instruccion de Timer0, Timer1 y Timer2 con reloj interno (FOSC/4). Los relojes externos,
    /// la compuerta de Timer1 y el modo asincrono no se modelan. Los ticks de la propia instruccion que escribe
    /// un contador se aplican despues de la escritura (error maximo de 1 ciclo en el valor absoluto, ninguno en los periodos).
    fn tick_timers(&mut self, now: u64) {
        self.tick_adc(now);
        let (option, intcon) = (self.ram[0x81], 0x0B);
        // Timer0: T0CS = 0 (reloj interno); PSA = 0 asigna el preescalador a Timer0
        if option & 0x20 == 0 {
            if self.t0_inhibit > 0 {
                self.t0_inhibit -= 1;
            } else {
                let div = if option & 0x08 == 0 { 2u32 << (option & 7) } else { 1 };
                self.t0_pre += 1;
                if self.t0_pre >= div {
                    self.t0_pre = 0;
                    self.ram[0x01] = self.ram[0x01].wrapping_add(1);
                    if self.ram[0x01] == 0 {
                        self.ram[intcon] |= 0x04; // T0IF
                        self.log("T0IF", now);
                    }
                }
            }
        }
        // Timer1: TMR1ON = 1 y TMR1CS = 0
        let t1con = self.ram[0x10];
        if t1con & 0x01 != 0 && t1con & 0x02 == 0 {
            self.t1_pre += 1;
            if self.t1_pre >= 1 << (t1con >> 4 & 3) {
                self.t1_pre = 0;
                let n = (u16::from(self.ram[0x0F]) << 8 | u16::from(self.ram[0x0E])).wrapping_add(1);
                self.ram[0x0E] = n as u8;
                self.ram[0x0F] = (n >> 8) as u8;
                if n == 0 {
                    self.ram[0x0C] |= 0x01; // TMR1IF
                    self.log("TMR1IF", now);
                }
            }
        }
        // Timer2: TMR2ON = 1; al llegar TMR2 a PR2 se reinicia y cuenta el postescalador
        let t2con = self.ram[0x12];
        if t2con & 0x04 != 0 {
            self.t2_pre += 1;
            let pre = match t2con & 3 {
                0 => 1,
                1 => 4,
                _ => 16,
            };
            if self.t2_pre >= pre {
                self.t2_pre = 0;
                if self.ram[0x11] == self.ram[0x92] {
                    self.ram[0x11] = 0;
                    self.t2_post += 1;
                    if self.t2_post > u32::from(t2con >> 3 & 0xF) {
                        self.t2_post = 0;
                        self.ram[0x0C] |= 0x02; // TMR2IF
                        self.log("TMR2IF", now);
                    }
                } else {
                    self.ram[0x11] = self.ram[0x11].wrapping_add(1);
                }
            }
        }
    }

    /// ADCON0 (1Fh): conversion de 11 TAD + 1 ciclo tras poner GO/DONE con el ADC encendido; al terminar escribe ADRESH:ADRESL
    /// segun ADFM (ADCON1<7>), borra GO y activa ADIF (PIR1<6>).
    fn tick_adc(&mut self, now: u64) {
        let a = self.ram[0x1F];
        let (on, go, chs, adcs) = (a & 1 != 0, a & 2 != 0, a >> 2 & 0xF, a >> 6);
        if chs != self.adc_prev_chs || (on && !self.adc_prev_on) {
            self.adc_acq_from = now; // el condensador de muestreo empieza a cargarse con el canal nuevo
        }
        if go && !self.adc_prev_go {
            let cycle_ns = 4e9 / self.fosc_hz as f64;
            if !on {
                self.adc_violations.push("GO/DONE puesto con el ADC apagado (ADON = 0): falta ADC.start()".into());
            } else {
                let acq_ns = (now - self.adc_acq_from) as f64 * cycle_ns;
                if acq_ns < 5000.0 {
                    self.adc_violations.push(format!("adquisicion de {acq_ns:.0} ns: el datasheet pide 5000 ns tras cambiar de canal"));
                }
                let tad_cycles = match adcs {
                    0 => 2.0 / 4.0,
                    1 => 8.0 / 4.0,
                    2 => 32.0 / 4.0,
                    _ => 4000.0 / cycle_ns,
                };
                if adcs != 3 {
                    let tad_ns = tad_cycles * cycle_ns;
                    if !(1600.0..=6000.0).contains(&tad_ns) {
                        self.adc_violations.push(format!("TAD de {tad_ns:.0} ns: el datasheet exige 1600..6000 ns"));
                    }
                }
                self.adc_remaining = (11.0 * tad_cycles).ceil() as u32 + 1;
            }
        }
        if self.adc_remaining > 0 && !go {
            self.adc_remaining = 0; // GO borrado por el programa: se aborta sin actualizar el resultado
        } else if self.adc_remaining > 0 {
            self.adc_remaining -= 1;
            if self.adc_remaining == 0 {
                let v = self.adc_inputs[chs as usize] & 0x3FF;
                if self.ram[0x9F] & 0x80 != 0 {
                    self.ram[0x1E] = (v >> 8) as u8;
                    self.ram[0x9E] = v as u8;
                } else {
                    self.ram[0x1E] = (v >> 2) as u8;
                    self.ram[0x9E] = ((v & 3) << 6) as u8;
                }
                self.ram[0x1F] &= !2;
                self.ram[0x0C] |= 1 << 6;
            }
        }
        let a = self.ram[0x1F];
        (self.adc_prev_go, self.adc_prev_on, self.adc_prev_chs) = (a & 2 != 0, a & 1 != 0, a >> 2 & 0xF);
    }

    fn log(&mut self, flag: &'static str, now: u64) {
        if self.timer_events.len() < 4096 {
            self.timer_events.push((flag, now));
        }
    }

    fn add_flags(&mut self, a: u8, b: u8) {
        let r = a as u16 + b as u16;
        self.set_flag(C, r > 0xFF);
        self.set_flag(DC, (a & 0xF) + (b & 0xF) > 0xF);
        self.zflag(r as u8);
    }

    /// `a - b`: C = 1 si no hay prestamo (a >= b), DC analogo en el nibble bajo.
    fn sub_flags(&mut self, a: u8, b: u8) {
        self.set_flag(C, a >= b);
        self.set_flag(DC, (a & 0xF) >= (b & 0xF));
        self.zflag(a.wrapping_sub(b));
    }

    /// Ejecuta hasta que `stop` sea verdadero o se agoten `max_cycles`. Devuelve true si se cumplio `stop`.
    pub fn run_until(&mut self, max_cycles: u64, mut stop: impl FnMut(&Sim) -> bool) -> bool {
        let limit = self.cycles + max_cycles;
        while self.cycles < limit {
            if stop(self) {
                return true;
            }
            if !self.step() {
                return stop(self);
            }
        }
        stop(self)
    }
}
