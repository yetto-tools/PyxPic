//! PIC16F628A: pack verificado contra DS40044G, el blink corre en el simulador y los perifericos sin probar se rechazan.
//! El simulador modela el PIC16F887: aqui solo se usa para el nucleo y PORTB/TRISB, que estan en las mismas direcciones.

use std::path::PathBuf;

use pyxpic_device::DevicePack;
use pyxpic_frontend::analyze;
use pyxpic_ir::{lower, verify};
use pyxpic_pic14::codegen::Compiled;
use pyxpic_pic14::sim::Sim;
use pyxpic_pic14::{compile, CodegenError, Options};

const CONFIG_ADDR: u32 = 0x2007;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn pack() -> DevicePack {
    let p = DevicePack::load(&root().join("devices/pic16f628a")).unwrap();
    assert_eq!(p.validate(), Vec::<String>::new());
    p
}

fn options(p: &DevicePack) -> Options {
    Options {
        device: p.device.name.clone(),
        program_words: p.memory.program.words,
        ram_start: 0x20,
        ram_end: 0x6F,
        config: p.config.build_words(),
        registers: p.registers.clone(),
        analog_pins: Default::default(),
        ..Options::default()
    }
}

fn try_build(src: &str) -> Result<Compiled, CodegenError> {
    let p = pack();
    let r = analyze(src, &p.target_profile());
    let prog = r.program.unwrap_or_else(|| panic!("frontend:\n{}\n{src}", r.diagnostics.render(src, "t.py")));
    let m = lower(&prog);
    verify(&m).unwrap_or_else(|e| panic!("IR invalida: {e:?}"));
    compile(&m, &options(&p))
}

fn blink() -> String {
    std::fs::read_to_string(root().join("examples/python/pic16f628a/blink.py")).unwrap().replace("\r\n", "\n")
}

#[test]
fn the_pack_matches_the_datasheet_numbers() {
    let p = pack();
    assert_eq!((p.memory.program.words, p.memory.data.ram_bytes, p.memory.eeprom.bytes), (2048, 224, 128));
    assert_eq!(p.device.io_pins, 15);
    assert_eq!(p.registers.get("CMCON").unwrap().addresses[0].0, 0x1F);
    assert_eq!(p.config.build_words(), vec![(CONFIG_ADDR, 0x3F18)]);
}

#[test]
fn fosc_is_split_between_bits_1_0_and_bit_4() {
    let p = pack();
    // DS40044G p.98: FOSC<2:0> ocupa los bits 4, 1 y 0
    for (name, bits) in [("LP", 0x00), ("XT", 0x01), ("HS", 0x02), ("EC", 0x03), ("INTRC_NOCLKOUT", 0x10), ("EXTRC_CLKOUT", 0x13)] {
        let mut words = p.config.build_words();
        p.config.apply(&mut words, "FOSC", name).unwrap();
        assert_eq!(words[0].1, 0x3F08 | bits, "{name}");
    }
}

#[test]
fn the_blink_example_toggles_rb0_every_500_ms() {
    let c = try_build(&blink()).unwrap();
    assert_eq!(c.image.iter().find(|(a, _)| *a == CONFIG_ADDR).map(|(_, w)| w), Some(0x3F18));
    let mut sim = Sim::new(&c.image);
    let mut edges = Vec::new();
    let mut last = 0;
    for _ in 0..3 {
        let limit = sim.cycles + 600_000;
        assert!(sim.run_until(limit, |s| s.read(0x06) & 1 != last), "RB0 no cambia");
        last = sim.read(0x06) & 1;
        edges.push(sim.cycles);
    }
    assert_eq!(sim.read(0x86) & 1, 0, "TRISB0 = salida");
    // 500 000 ciclos de espera mas las instrucciones del bucle, a 1 us por ciclo
    for pair in edges.windows(2) {
        assert!((500_000..500_020).contains(&(pair[1] - pair[0])), "{edges:?}");
    }
}

#[test]
fn a_comparator_pin_turns_the_comparators_off() {
    let c = try_build(&blink().replace("RB0", "RA0")).unwrap();
    assert!(c.listing.contains("MOVLW 0x07"), "{}", c.listing);
    let mut sim = Sim::new(&c.image);
    sim.run_until(2_000, |s| s.read(0x85) & 1 == 0);
    assert_eq!(sim.read(0x85) & 1, 0, "TRISA0 = salida");
}

#[test]
fn pins_that_the_chip_does_not_have_are_rejected() {
    let src = blink().replace("RB0", "RD0");
    assert!(analyze(&src, &pack().target_profile()).program.is_none());
}

#[test]
fn untested_peripherals_are_rejected_with_a_clear_error() {
    let src = "from pyxpic.device import PIC16F628A\nfrom pyxpic.periph.timer import TMR0\nCLOCK: Internal\nFREQUENCY: 4MHz\n\n\ndef main() -> None:\n    TMR0.start(prescaler=256)\n";
    let err = try_build(src).unwrap_err().to_string();
    assert!(err.contains("temporizadores en el PIC16F628A"), "{err}");
}
