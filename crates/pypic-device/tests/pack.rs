use std::path::PathBuf;

use pypic_device::{DevicePack, Num};
use pypic_frontend::{analyze, TargetProfile};

fn pack_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../devices/pic16f887")
}

fn pack() -> DevicePack {
    DevicePack::load(&pack_dir()).expect("el pack pic16f887 debe cargar")
}

fn has(errs: &[String], needle: &str) -> bool {
    errs.iter().any(|e| e.contains(needle))
}

#[test]
fn shipped_pic16f887_pack_is_valid() {
    let errs = pack().validate();
    assert!(errs.is_empty(), "{errs:#?}");
}

/// Valores copiados del datasheet DS40001291H (p.1-2, p.22-26, p.206-207).
#[test]
fn pic16f887_matches_datasheet_numbers() {
    let p = pack();
    assert_eq!(p.device.cpu.data_bits, 8);
    assert_eq!(p.device.cpu.instruction_bits, 14);
    assert_eq!(p.device.cpu.instruction_count, 35);
    assert_eq!(p.device.cpu.max_clock_hz, 20_000_000);
    assert_eq!(p.device.cpu.clocks_per_instruction, 4); // 20 MHz -> 200 ns (p.1)
    assert_eq!(p.device.cpu.default_clock_hz, 4_000_000); // OSCCON<IRCF> = 110 por defecto (p.64)
    assert_eq!(p.device.io_pins, 35);
    assert_eq!(p.memory.program.words, 8192);
    assert_eq!(p.memory.program.interrupt_vector, Num(4));
    assert_eq!(p.memory.program.stack_levels, 8);
    assert_eq!(p.memory.data.ram_bytes, 368);
    assert_eq!(p.memory.eeprom.bytes, 256);
    assert_eq!(p.memory.data.banks, 4);
    assert_eq!(p.config.words[0].address, Num(0x2007));
    assert_eq!(p.config.words[1].address, Num(0x2008));
    assert_eq!(p.pins.packages[0].pin_count, 40);
}

#[test]
fn pin_map_spot_checks_against_datasheet_diagram() {
    let p = pack();
    let pins = &p.pins.packages[0].pins;
    let at = |n: u32| pins.iter().find(|x| x.number == n).unwrap();
    assert_eq!(at(1).name, "RE3");
    assert!(at(1).input_only && at(1).functions.contains(&"MCLR".to_string()));
    assert_eq!(at(17).name, "RC2");
    assert!(at(17).functions.contains(&"CCP1".to_string()));
    assert_eq!(at(25).name, "RC6");
    assert!(at(25).functions.contains(&"TX".to_string()));
    assert_eq!(at(38).name, "RB5");
    assert!(at(38).functions.contains(&"T1G".to_string()));
    assert_eq!(at(40).name, "RB7");
    assert_eq!((at(11).name.as_str(), at(32).name.as_str()), ("VDD", "VDD"));
    assert_eq!((at(12).name.as_str(), at(31).name.as_str()), ("VSS", "VSS"));
}

#[test]
fn target_profile_comes_from_the_pack() {
    let t = pack().target_profile();
    assert_eq!((t.native_bits, t.max_int_bits), (8, 32));
    assert_eq!(t.name, "PIC16F887");
}

#[test]
fn frontend_uses_pack_profile_wide_types_warn() {
    let t = pack().target_profile();
    let r = analyze("a: u8 = 0\nb: u16 = 0\n", &t);
    assert!(r.program.is_some());
    assert_eq!(r.diagnostics.codes(), vec!["PYP3001"]);
    // u8 no avisa; con el perfil generico tampoco hay aviso
    assert!(analyze("b: u16 = 0\n", &TargetProfile::generic()).diagnostics.items.is_empty());
}

#[test]
fn rejects_inconsistent_memory() {
    let mut p = pack();
    p.memory.data.ram_bytes = 369;
    assert!(has(&p.validate(), "no coincide con ram_bytes"));

    let mut p = pack();
    p.memory.data.gpr[0].start = Num(0x010); // se solapa con SFR
    assert!(has(&p.validate(), "banco 0"));

    let mut p = pack();
    p.memory.data.gpr[2].start = Num(0x120); // deja un hueco en el banco 2
    assert!(has(&p.validate(), "banco 2"));

    let mut p = pack();
    p.memory.data.sfr[3].end = Num(0x19F); // SFR del banco 3 invade el GPR
    assert!(has(&p.validate(), "banco 3"));

    let mut p = pack();
    p.memory.program.interrupt_vector = Num(9000);
    assert!(has(&p.validate(), "fuera de la memoria"));

    let mut p = pack();
    p.memory.data.common.mirrors[0] = Num(0x0F1);
    assert!(has(&p.validate(), "espejo"));
}

#[test]
fn rejects_inconsistent_pins() {
    let mut p = pack();
    p.pins.packages[0].pins[1].number = 3; // numero repetido
    assert!(has(&p.validate(), "repetido"));

    let mut p = pack();
    p.pins.packages[0].pins[1].name = "RB7".into(); // nombre no coincide con port/bit
    assert!(has(&p.validate(), "inconsistente"));

    let mut p = pack();
    p.device.io_pins = 36;
    assert!(has(&p.validate(), "pines de E/S"));

    let mut p = pack();
    p.pins.packages[0].pins.pop();
    assert!(has(&p.validate(), "pin_count"));
}

#[test]
fn rejects_inconsistent_config() {
    let mut p = pack();
    p.config.words[0].fields[0].lsb = 14; // fuera de la palabra de 14 bits
    assert!(has(&p.validate(), "fuera de la palabra"));

    let mut p = pack();
    p.config.words[0].fields[1].lsb = 13; // se solapa con DEBUG
    assert!(has(&p.validate(), "se solapa"));

    let mut p = pack();
    p.config.words[1].address = Num(0x1FFF);
    assert!(has(&p.validate(), "fuera del espacio"));

    let mut p = pack();
    p.config.words[0].fields[4].values.remove("2"); // BOREN incompleto
    assert!(has(&p.validate(), "faltan valores"));
}

#[test]
fn rejects_bad_device_info_and_schema() {
    let mut p = pack();
    p.device.cpu.data_bits = 12;
    assert!(has(&p.validate(), "data_bits"));

    let mut p = pack();
    p.device.pypic.max_int_bits = 4;
    assert!(has(&p.validate(), "max_int_bits"));

    let mut p = pack();
    p.device.pypic.support_level = "mágico".into();
    assert!(has(&p.validate(), "support_level"));

    let mut p = pack();
    p.memory.schema_version = 2;
    assert!(has(&p.validate(), "schema_version"));

    let mut p = pack();
    p.device.sources.clear();
    assert!(has(&p.validate(), "fuente"));
}

#[test]
fn config_defaults_are_all_ones_and_fields_cover_known_bits() {
    let p = pack();
    for w in &p.config.words {
        assert_eq!(w.default, Num(0x3FFF), "{}", w.name);
    }
    // CONFIG1 usa los 14 bits; CONFIG2 solo WRT (10-9) y BOR4V (8)
    let bits = |i: usize| p.config.words[i].fields.iter().map(|f| f.width).sum::<u32>();
    assert_eq!(bits(0), 14);
    assert_eq!(bits(1), 3);
}

#[test]
fn unknown_fields_and_bad_numbers_fail_to_load() {
    let dir = std::env::temp_dir().join(format!("pypic-pack-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    for f in ["device.json", "memory.json", "pins.json", "config.json"] {
        std::fs::copy(pack_dir().join(f), dir.join(f)).unwrap();
    }
    let dev = std::fs::read_to_string(dir.join("device.json")).unwrap();
    std::fs::write(dir.join("device.json"), dev.replacen("\"io_pins\": 35,", "\"io_pins\": 35, \"campo_raro\": 1,", 1)).unwrap();
    assert!(DevicePack::load(&dir).unwrap_err().0.contains("campo_raro"));

    std::fs::copy(pack_dir().join("device.json"), dir.join("device.json")).unwrap();
    let mem = std::fs::read_to_string(dir.join("memory.json")).unwrap();
    std::fs::write(dir.join("memory.json"), mem.replacen("0x0004", "0xZZ", 1)).unwrap();
    assert!(DevicePack::load(&dir).unwrap_err().0.contains("numero invalido"));
    std::fs::remove_dir_all(&dir).ok();

    assert!(DevicePack::load(&dir).is_err());
}
