//! Los ejemplos en ensamblador de examples/asm: cada uno se ensambla (con los psects de reset y de interrupcion de pic-as) y, en el simulador
//! del nucleo, hace lo mismo que su contraparte en Python y en BASIC.

use std::path::{Path, PathBuf};

use pypic_device::DevicePack;
use pypic_pic14::sim::Sim;
use pypic_pic14::{assemble_with, render_errors, AsmEnv, Assembled};

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn assemble_file(path: &Path) -> Assembled {
    let mut e = AsmEnv::from_pack(&DevicePack::load(&root().join("devices/pic16f887")).unwrap());
    e.add_link_options(&["-presetVec=0h".to_string(), "-pintentry=4h".to_string()]).unwrap();
    let src = std::fs::read_to_string(path).unwrap().replace("\r\n", "\n");
    assemble_with(&src, &e).unwrap_or_else(|er| panic!("{}", render_errors(&er, &path.display().to_string())))
}

fn example(rel: &str) -> Assembled {
    assemble_file(&root().join("examples/asm").join(rel))
}

fn sim(rel: &str) -> Sim {
    Sim::new(&example(rel).image)
}

#[test]
fn every_asm_example_assembles() {
    let mut n = 0;
    let mut stack = vec![root().join("examples/asm")];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(d).unwrap().flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().is_some_and(|x| x == "s") {
                assemble_file(&p);
                n += 1;
            }
        }
    }
    assert!(n >= 6, "se esperaban los ejemplos en ensamblador: {n}");
}

#[test]
fn boton_led_follows_the_button() {
    for (button, led) in [(0u8, false), (1, true)] {
        let mut s = sim("gpio/boton_led.s");
        s.write(0x06, button);
        s.run_until(500, |_| false);
        assert_eq!(s.pin('D', 0), Some(led), "RB0 = {button}");
    }
}

#[test]
fn adc_example_lights_the_led_above_half_scale() {
    for (v, led) in [(100u16, false), (511, false), (512, true), (1000, true)] {
        let mut s = sim("adc/adc.s");
        s.adc_inputs[0] = v;
        s.run_until(2_000, |_| false);
        assert_eq!(s.pin('D', 0), Some(led), "AN0 = {v}");
    }
}

#[test]
fn eeprom_arranques_counts_boots_across_resets() {
    let mut s = sim("eeprom/eeprom_arranques.s");
    for boot in 1..=3u8 {
        s.reset();
        s.run_until(60_000, |_| false);
        assert_eq!(s.eeprom[0], boot, "arranque {boot}");
        assert_eq!(s.pin('D', 0), Some(true), "arranque {boot}: el primer parpadeo ya empezo (197 ms encendido)");
    }
}

#[test]
fn eco_example_echoes_every_byte() {
    let mut s = sim("uart/eco.s");
    s.run_until(2_000, |_| false);
    for (i, b) in [0x41u8, 0x00, 0xFF, 0x7E].iter().enumerate() {
        s.uart_feed(*b, s.cycles + 3_000 + i as u64 * 3_000, true);
    }
    s.run_until(s.cycles + 40_000, |_| false);
    let sent: Vec<u8> = s.uart_tx.iter().map(|(b, _)| *b).collect();
    assert_eq!(sent, vec![0x41, 0x00, 0xFF, 0x7E]);
}

#[test]
fn pwm_example_takes_the_duty_from_the_adc() {
    for v in [703u16, 0, 1023, 512] {
        let mut s = sim("pwm/pwm.s");
        s.adc_inputs[0] = v;
        s.run_until(8_000, |_| false);
        assert_eq!(s.read(0x15), (v >> 2) as u8, "CCPR1L con AN0 = {v}");
        assert_eq!(s.read(0x17), 0x0C | (((v & 3) as u8) << 4), "CCP1CON (modo PWM y DC1B) con AN0 = {v}");
    }
}
