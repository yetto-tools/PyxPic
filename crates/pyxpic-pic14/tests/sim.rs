use std::path::PathBuf;

use pyxpic_device::DevicePack;
use pyxpic_pic14::samples::{self, BLINK_CONFIG1, BLINK_CONFIG2, CONFIG1, CONFIG2};
use pyxpic_pic14::sim::{Sim, STATUS};
use pyxpic_pic14::{
    Dest::{F, W},
    Image, Inst,
    Inst::*,
};

fn sim(prog: &[Inst]) -> Sim {
    let mut img = Image::new();
    for (i, inst) in prog.iter().enumerate() {
        img.set(i as u32, inst.encode().unwrap()).unwrap();
    }
    Sim::new(&img)
}

fn run(prog: &[Inst], steps: usize) -> Sim {
    let mut s = sim(prog);
    for _ in 0..steps {
        s.step();
    }
    s
}

fn flags(s: &Sim) -> (bool, bool, bool) {
    let st = s.read(STATUS);
    (st & 1 != 0, st & 2 != 0, st & 4 != 0) // C, DC, Z
}

// ---- semantica de instrucciones (DS40001291H tabla 15-2) ---------------------------------

#[test]
fn addlw_flags() {
    let s = run(&[Movlw(0xFF), Addlw(1)], 2);
    assert_eq!((s.w, flags(&s)), (0, (true, true, true)));
    let s = run(&[Movlw(0x0F), Addlw(1)], 2);
    assert_eq!((s.w, flags(&s)), (0x10, (false, true, false)));
}

#[test]
fn sublw_and_subwf_borrow_semantics() {
    // SUBLW k: W = k - W; C = 1 si no hay prestamo
    let s = run(&[Movlw(5), Sublw(3)], 2);
    assert_eq!((s.w, flags(&s).0), (0xFE, false));
    let s = run(&[Movlw(3), Sublw(5)], 2);
    assert_eq!((s.w, flags(&s).0), (2, true));
    let s = run(&[Movlw(3), Sublw(3)], 2);
    assert_eq!((s.w, flags(&s)), (0, (true, true, true)));
    // SUBWF f,F: f = f - W
    let s = run(&[Movlw(1), Movwf(0x20), Movlw(2), Subwf(0x20, F)], 4);
    assert_eq!((s.read(0x20), flags(&s).0), (0xFF, false));
}

#[test]
fn logic_and_z_flag() {
    let s = run(&[Movlw(0xF0), Andlw(0x0F)], 2);
    assert_eq!((s.w, flags(&s).2), (0, true));
    let s = run(&[Movlw(0xF0), Iorlw(0x0F), Xorlw(0xFF)], 3);
    assert_eq!((s.w, flags(&s).2), (0, true));
    let s = run(&[Movlw(0x55), Movwf(0x20), Comf(0x20, W)], 3);
    assert_eq!((s.w, s.read(0x20)), (0xAA, 0x55));
}

#[test]
fn rotates_go_through_carry() {
    // 0x81 con C=0: RLF -> 0x02 con C=1; luego RRF con C=1 -> 0x81, C=0
    let s = run(&[Movlw(0x81), Movwf(0x20), Rlf(0x20, F)], 3);
    assert_eq!((s.read(0x20), flags(&s).0), (0x02, true));
    let s = run(&[Movlw(0x81), Movwf(0x20), Rlf(0x20, F), Rrf(0x20, F)], 4);
    assert_eq!((s.read(0x20), flags(&s).0), (0x81, false));
}

#[test]
fn swapf_does_not_touch_status() {
    let mut s = sim(&[Movlw(0xA5), Movwf(0x20), Swapf(0x20, W)]);
    let before = s.read(STATUS);
    for _ in 0..3 {
        s.step();
    }
    assert_eq!(s.w, 0x5A);
    assert_eq!(s.read(STATUS), before);
}

#[test]
fn skip_instructions_cost_two_cycles_when_taken() {
    // DECFSZ llega a 0: salta el MOVLW 0xAA
    let mut s = sim(&[Movlw(1), Movwf(0x20), Decfsz(0x20, F), Movlw(0xAA), Movlw(0xBB)]);
    for _ in 0..4 {
        s.step();
    }
    assert_eq!(s.w, 0xBB);
    assert_eq!(s.cycles, 1 + 1 + 2 + 1); // el salto cuesta un ciclo extra
                                         // no salta si el resultado no es 0
    let s = run(&[Movlw(2), Movwf(0x20), Decfsz(0x20, F), Movlw(0xAA)], 4);
    assert_eq!(s.w, 0xAA);
    // BTFSC / BTFSS
    let s = run(&[Movlw(0x01), Movwf(0x20), Btfsc(0x20, 0), Movlw(0xAA), Btfss(0x20, 1), Movlw(0xCC), Movlw(0xDD)], 6);
    assert_eq!(s.w, 0xCC); // bit0=1: BTFSC no salta (MOVLW AA); bit1=0: BTFSS no salta (MOVLW CC)
    let s = run(&[Movlw(0x01), Movwf(0x20), Btfss(0x20, 0), Movlw(0xAA), Movlw(0xDD)], 4);
    assert_eq!(s.w, 0xDD); // bit0=1 -> BTFSS salta el MOVLW AA
}

#[test]
fn call_return_retlw_and_stack_depth() {
    // 0: CALL 3; 1: MOVLW 0x11; 2: SLEEP; 3: RETLW 0x42
    let mut s = sim(&[Call(3), Movlw(0x11), Sleep, Retlw(0x42)]);
    s.step();
    assert_eq!(s.pc, 3);
    s.step();
    assert_eq!((s.pc, s.w), (1, 0x42));
    // 8 llamadas anidadas (la profundidad de la pila de la F887) se resuelven y vuelven bien:
    // 0: CALL r1; 1: SLEEP; r_i (i=1..7) en 2i: CALL r_{i+1}; RETURN; r8 en 16: RETLW 0x99
    let mut prog = vec![Call(2), Sleep];
    for i in 1..=7u16 {
        prog.push(Call(2 * (i + 1)));
        prog.push(Return);
    }
    prog.push(Retlw(0x99));
    let mut s = sim(&prog);
    assert!(s.run_until(1000, |s| s.halted || false) || s.halted);
    assert_eq!(s.w, 0x99);
    assert!(s.halted);
}

#[test]
fn call_and_goto_take_page_bits_from_pclath() {
    let mut s = sim(&[Movlw(0x08), Movwf(0x0A), Goto(0x005)]);
    for _ in 0..3 {
        s.step();
    }
    assert_eq!(s.pc, 0x805); // PCLATH<4:3> = 01 -> pagina 1
}

#[test]
fn computed_goto_through_pcl() {
    // 0: MOVLW 1; 1: CALL 4; 2: NOP; 4: ADDWF PCL,F; 5..7: RETLW 10h/20h/30h
    let mut img = Image::new();
    let prog: &[(u32, Inst)] = &[(0, Movlw(1)), (1, Call(4)), (2, Nop), (4, Addwf(0x02, F)), (5, Retlw(0x10)), (6, Retlw(0x20)), (7, Retlw(0x30))];
    for &(a, i) in prog {
        img.set(a, i.encode().unwrap()).unwrap();
    }
    let mut s = Sim::new(&img);
    for _ in 0..4 {
        s.step();
    }
    assert_eq!((s.w, s.pc), (0x20, 2)); // indice 1 -> segundo RETLW, y vuelve tras el CALL
}

#[test]
fn indirect_addressing_through_fsr() {
    let s = run(&[Movlw(0x30), Movwf(0x04), Movlw(0x77), Movwf(0x00), Incf(0x00, F), Movf(0x00, W)], 6);
    assert_eq!(s.read(0x30), 0x78);
    assert_eq!(s.w, 0x78);
}

#[test]
fn banks_mirrors_and_common_ram() {
    // banco 1: el desplazamiento 0x20 es 0xA0, no 0x20
    let s = run(&[Bsf(STATUS as u8, 5), Movlw(0xAA), Movwf(0x20)], 3);
    assert_eq!((s.read(0xA0), s.read(0x20)), (0xAA, 0));
    // zona comun 70h-7Fh: visible desde cualquier banco
    let s = run(&[Movlw(0x5A), Movwf(0x70), Bsf(STATUS as u8, 5), Movf(0x70, W)], 4);
    assert_eq!(s.w, 0x5A);
    // STATUS esta en todos los bancos: en el banco 1 BCF STATUS,5 vuelve al banco 0
    let s = run(&[Bsf(STATUS as u8, 5), Bcf(STATUS as u8, 5), Movlw(7), Movwf(0x21)], 4);
    assert_eq!(s.read(0x21), 7);
}

#[test]
fn trisd_resets_to_input_and_pin_follows_latch() {
    let s = sim(&[]);
    assert_eq!(s.pin('D', 0), None); // entrada (alta impedancia)
                                     // 08h en banco 1 es TRISD: BCF lo vuelve salida
    let s = run(&[Bsf(STATUS as u8, 5), Bcf(0x08, 0), Bcf(STATUS as u8, 5), Bsf(0x08, 0)], 4);
    assert_eq!(s.pin('D', 0), Some(true));
}

#[test]
fn sleep_halts_and_erased_flash_is_addlw_ff() {
    let mut s = sim(&[Sleep]);
    assert!(s.step());
    assert!(!s.step());
    assert_eq!(s.read(STATUS) & 0x08, 0); // PD = 0
    let mut s = sim(&[]);
    s.w = 1;
    s.step(); // flash borrada: 0x3FFF = ADDLW 0xFF
    assert_eq!(s.w, 0);
}

// ---- verificacion del blink de muestra ---------------------------------------------------------

/// Eventos del pin RD0: (ciclo, nivel) cada vez que cambia su estado excitado.
fn rd0_events(img: &Image, want: usize) -> Vec<(u64, bool)> {
    let mut s = Sim::new(img);
    let mut events: Vec<(u64, bool)> = Vec::new();
    let mut last = s.pin('D', 0);
    while events.len() < want && s.cycles < 20_000_000 {
        s.step();
        let now = s.pin('D', 0);
        if now != last {
            if let Some(level) = now {
                events.push((s.cycles, level));
            }
            last = now;
        }
    }
    events
}

#[test]
fn blink_toggles_rd0_with_cycle_exact_timing() {
    let ev = rd0_events(&samples::blink_rd0(), 5);
    let levels: Vec<bool> = ev.iter().map(|e| e.1).collect();
    assert_eq!(levels, [false, true, false, true, false]); // RD0 pasa a salida en bajo, luego parpadea
                                                           // Alto -> bajo: CALL..RETURN (592 142 ciclos, calculados a mano) + BCF (1)
    assert_eq!(ev[2].0 - ev[1].0, 592_143);
    // Bajo -> alto: delay (592 142) + GOTO (2) + BSF (1)
    assert_eq!(ev[3].0 - ev[2].0, 592_145);
    // Periodo = 1 184 288 ciclos = 1.184 s con 1 MHz de ciclo de instruccion (Fosc = 4 MHz)
    assert_eq!(ev[4].0 - ev[2].0, 1_184_288);
}

#[test]
fn blink_survives_the_hex_file_roundtrip() {
    let img = samples::blink_rd0();
    let hex = img.to_intel_hex(16);
    let parsed = Image::from_intel_hex(&hex).unwrap();
    assert_eq!(parsed, img);
    assert_eq!(rd0_events(&parsed, 3).iter().map(|e| e.1).collect::<Vec<_>>(), [false, true, false]);
}

#[test]
fn blink_only_writes_expected_addresses_and_fits_the_device() {
    let img = samples::blink_rd0();
    assert!(img.check_bounds(8192, (0x2000, 0x3FFF)).is_ok());
    assert_eq!(img.get(CONFIG1), Some(BLINK_CONFIG1));
    assert_eq!(img.get(CONFIG2), Some(BLINK_CONFIG2));
    // vector de reset: GOTO 0x0005
    assert_eq!(img.get(0), Some(Goto(5).encode().unwrap()));
}

/// Los valores de CONFIG se contrastan campo a campo con el Device Pack del PIC16F887.
#[test]
fn blink_config_words_match_the_device_pack_fields() {
    let pack = DevicePack::load(&PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../devices/pic16f887")).unwrap();
    let field = |word: u16, name: &str| -> u32 {
        let f = pack.config.words.iter().flat_map(|w| &w.fields).find(|f| f.name == name).unwrap_or_else(|| panic!("{name}"));
        let v = (word as u32 >> f.lsb) & ((1 << f.width) - 1);
        assert!(f.values.contains_key(&v.to_string()), "{name}={v} no existe en el pack");
        v
    };
    let c1 = [("DEBUG", 1), ("LVP", 0), ("FCMEN", 0), ("IESO", 0), ("BOREN", 0), ("CPD", 1), ("CP", 1), ("MCLRE", 0), ("PWRTE", 1), ("WDTE", 0), ("FOSC", 4)];
    for (name, want) in c1 {
        assert_eq!(field(BLINK_CONFIG1, name), want, "CONFIG1.{name}");
    }
    assert_eq!(field(BLINK_CONFIG2, "WRT"), 3);
    assert_eq!(field(BLINK_CONFIG2, "BOR4V"), 1);
    // FOSC=100 es INTOSCIO y WDTE=0 es "WDT deshabilitado" segun el pack
    let fosc = pack.config.words[0].fields.iter().find(|f| f.name == "FOSC").unwrap();
    assert!(fosc.values["4"].starts_with("INTOSCIO"));
    let wdte = pack.config.words[0].fields.iter().find(|f| f.name == "WDTE").unwrap();
    assert!(wdte.values["0"].contains("deshabilitado"));
}
