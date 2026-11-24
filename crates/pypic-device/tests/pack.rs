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

// ---- registers.json ------------------------------------------------------------------------------------

fn reg<'a>(p: &'a DevicePack, name: &str) -> &'a pypic_device::Register {
    p.registers.get(name).unwrap_or_else(|| panic!("falta {name}"))
}

fn addrs(r: &pypic_device::Register) -> Vec<u32> {
    r.addresses.iter().map(|a| a.0).collect()
}

/// Direcciones, bits y valores de reset copiados de las Tablas 2-1 a 2-4 del DS40001291H (p.27-29).
#[test]
fn registers_match_datasheet_tables() {
    let p = pack();
    assert_eq!(p.registers.registers.len(), 73);
    assert_eq!(addrs(reg(&p, "STATUS")), [0x003, 0x083, 0x103, 0x183]);
    assert_eq!(reg(&p, "STATUS").bits.iter().map(|b| b.as_deref().unwrap()).collect::<Vec<_>>(), ["C", "DC", "Z", "PD", "TO", "RP0", "RP1", "IRP"]);
    assert_eq!(reg(&p, "STATUS").por, "0001 1xxx");
    assert_eq!(addrs(reg(&p, "PORTD")), [0x008]);
    assert_eq!(addrs(reg(&p, "TRISD")), [0x088]);
    assert_eq!(addrs(reg(&p, "PORTB")), [0x006, 0x106]);
    assert_eq!(addrs(reg(&p, "TRISB")), [0x086, 0x186]);
    assert_eq!(addrs(reg(&p, "ANSEL")), [0x188]);
    assert_eq!(addrs(reg(&p, "ANSELH")), [0x189]);
    assert_eq!(addrs(reg(&p, "OPTION_REG")), [0x081, 0x181]);
    assert_eq!(addrs(reg(&p, "TMR0")), [0x001, 0x101]);
    assert_eq!(addrs(reg(&p, "ADCON0")), [0x01F]);
    assert_eq!(reg(&p, "ADCON0").bits[1].as_deref(), Some("GO_DONE"));
    assert_eq!(addrs(reg(&p, "OSCCON")), [0x08F]);
    assert_eq!(reg(&p, "OSCCON").por, "-110 q000"); // IRCF = 110 -> 4 MHz (p.64)
    assert_eq!(addrs(reg(&p, "EECON1")), [0x18C]);
    assert_eq!(addrs(reg(&p, "SPBRGH")), [0x09A]);
    assert!(!reg(&p, "INDF").physical && !reg(&p, "EECON2").physical);
    // SSPADD y SSPMSK comparten la direccion 93h: el segundo solo se ve con SSPM<3:0> = 1001
    assert_eq!(addrs(reg(&p, "SSPADD")), addrs(reg(&p, "SSPMSK")));
    assert!(reg(&p, "SSPMSK").selected_by.is_some() && reg(&p, "SSPADD").selected_by.is_none());
    // las direcciones 18Eh y 18Fh no existen
    assert_eq!(p.registers.unimplemented.iter().map(|n| n.0).collect::<Vec<_>>(), [0x18E, 0x18F]);
}

fn mutated(f: impl FnOnce(&mut DevicePack)) -> Vec<String> {
    let mut p = pack();
    f(&mut p);
    p.validate()
}

#[test]
fn registers_validation_catches_inconsistencies() {
    // falta un registro: queda una direccion SFR sin cubrir
    let e = mutated(|p| p.registers.registers.retain(|r| r.name != "ADCON0"));
    assert!(has(&e, "0x01f no tiene registro"), "{e:?}");
    // un registro fuera de la zona SFR de su banco
    let e = mutated(|p| p.registers.registers.iter_mut().find(|r| r.name == "PIR1").unwrap().addresses = vec![Num(0x01F0)]);
    assert!(has(&e, "no esta en la zona SFR"), "{e:?}");
    // dos registros en la misma direccion sin condicion de seleccion
    let e = mutated(|p| p.registers.registers.iter_mut().find(|r| r.name == "SSPMSK").unwrap().selected_by = None);
    assert!(has(&e, "sin condicion de seleccion"), "{e:?}");
    // bits mal contados, nombre repetido, bit con nombre de registro, reset invalido
    let e = mutated(|p| p.registers.registers.iter_mut().find(|r| r.name == "INTCON").unwrap().bits.pop().map(|_| ()).unwrap());
    assert!(has(&e, "8 entradas"), "{e:?}");
    let e = mutated(|p| p.registers.registers.iter_mut().find(|r| r.name == "PIR1").unwrap().bits[0] = Some("GIE".into()));
    assert!(has(&e, "'GIE' esta repetido"), "{e:?}");
    let e = mutated(|p| p.registers.registers.iter_mut().find(|r| r.name == "PIR1").unwrap().bits[0] = Some("STATUS".into()));
    assert!(has(&e, "tiene el nombre de un registro"), "{e:?}");
    let e = mutated(|p| p.registers.registers.iter_mut().find(|r| r.name == "PIR1").unwrap().por = "2000 0000".into());
    assert!(has(&e, "valor de reset"), "{e:?}");
    // direcciones de un registro con distinto desplazamiento
    let e = mutated(|p| p.registers.registers.iter_mut().find(|r| r.name == "TMR0").unwrap().addresses = vec![Num(0x001), Num(0x102)]);
    assert!(has(&e, "mismo desplazamiento"), "{e:?}");
}

#[test]
fn registers_must_support_every_pin_and_the_bank_select_bits() {
    let e = mutated(|p| p.registers.registers.retain(|r| r.name != "TRISD"));
    assert!(has(&e, "falta TRISD"), "{e:?}");
    let e = mutated(|p| p.registers.registers.iter_mut().find(|r| r.name == "PORTD").unwrap().bits[0] = Some("PD0".into()));
    assert!(has(&e, "el pin RD0 no tiene su bit"), "{e:?}");
    let e = mutated(|p| p.registers.registers.iter_mut().find(|r| r.name == "ANSEL").unwrap().bits[3] = None);
    assert!(has(&e, "ANSEL no tiene el bit ANS3"), "{e:?}");
    let e = mutated(|p| p.registers.registers.iter_mut().find(|r| r.name == "STATUS").unwrap().bits[5] = None);
    assert!(has(&e, "no tiene el bit de seleccion de banco RP0"), "{e:?}");
}

#[test]
fn unknown_fields_in_registers_json_are_rejected() {
    let r = pypic_device::Registers::parse(r#"{"schema_version":1,"source":"x","unimplemented":[],"registers":[],"extra":1}"#);
    assert!(r.is_err());
}

// ---- peripherals.json ----------------------------------------------------------------------------------

#[test]
fn peripherals_match_datasheet_chapters() {
    let p = pack();
    let pr = &p.peripherals;
    assert_eq!(pr.peripherals.len(), 17);
    let t1 = pr.get("TIMER1").unwrap();
    assert_eq!((t1.bits, t1.page), (Some(16), 78));
    assert_eq!(t1.enable, ["TMR1ON"]);
    assert_eq!((t1.interrupts[0].flag.as_str(), t1.interrupts[0].enable.as_str()), ("TMR1IF", "TMR1IE"));
    assert_eq!(pr.get("TIMER0").unwrap().bits, Some(8));
    assert_eq!(pr.get("ADC").unwrap().bits, Some(10));
    assert_eq!(pr.get("ADC").unwrap().pins.iter().filter(|x| x.function.starts_with("AN")).count(), 14);
    assert_eq!(pr.get("COMPARATORS").unwrap().enable, ["C1ON", "C2ON"]);
    assert_eq!(pr.get("MSSP").unwrap().modes, ["SPI", "I2C"]);
    assert_eq!(pr.get("ECCP").unwrap().uses, ["TIMER1", "TIMER2"]);
    // cada funcion alterna la posee un solo periferico
    assert_eq!(pr.owner_of("SCL").unwrap().name, "MSSP");
    assert_eq!(pr.owner_of("TX").unwrap().name, "EUSART");
    assert_eq!(pr.owner_of("P1B").unwrap().name, "ECCP");
    assert_eq!(pr.owner_of("CCP2").unwrap().name, "CCP2");
    assert_eq!(pr.owner_of("INT").unwrap().name, "EXT_INT");
    assert!(pr.owner_of("NOPE").is_none());
}

#[test]
fn pin_uses_report_what_a_pin_would_give_up() {
    let p = pack();
    let uses = |pin: &str| p.pin_uses(pin).into_iter().map(|(f, o)| format!("{f}:{o}")).collect::<Vec<_>>();
    assert!(uses("RD0").is_empty()); // RD0 solo es E/S digital
    assert_eq!(uses("RC3"), ["SCK:MSSP", "SCL:MSSP"]);
    assert_eq!(uses("RA4"), ["T0CKI:TIMER0", "C1OUT:COMPARATORS"]); // un pin, dos perifericos: conflicto posible
    assert_eq!(uses("RC2"), ["P1A:ECCP", "CCP1:ECCP"]);
    assert_eq!(uses("RB0"), ["AN12:ADC", "INT:EXT_INT"]);
    assert!(uses("RZ9").is_empty());
}

#[test]
fn peripherals_validation_catches_inconsistencies() {
    let e = mutated(|p| p.peripherals.peripherals.iter_mut().find(|x| x.name == "TIMER1").unwrap().registers.push("T9CON".into()));
    assert!(has(&e, "el registro T9CON no existe"), "{e:?}");
    let e = mutated(|p| p.peripherals.peripherals.iter_mut().find(|x| x.name == "TIMER1").unwrap().enable = vec!["TMR9ON".into()]);
    assert!(has(&e, "TMR9ON no existe"), "{e:?}");
    let e = mutated(|p| p.peripherals.peripherals.iter_mut().find(|x| x.name == "TIMER2").unwrap().interrupts[0].flag = "NOPEIF".into());
    assert!(has(&e, "NOPEIF no existe"), "{e:?}");
    // un periferico que no existe en pins.json, y una funcion de pin sin dueno
    let e = mutated(|p| p.peripherals.peripherals.iter_mut().find(|x| x.name == "TIMER0").unwrap().pins[0].function = "T0XYZ".into());
    assert!(has(&e, "T0XYZ no esta en ningun pin") && has(&e, "la funcion T0CKI del pin RA4 no la posee"), "{e:?}");
    // dos perifericos con la misma funcion
    let e = mutated(|p| {
        let f = p.peripherals.peripherals.iter().find(|x| x.name == "TIMER1").unwrap().pins[0].clone();
        p.peripherals.peripherals.iter_mut().find(|x| x.name == "TIMER0").unwrap().pins.push(f);
    });
    assert!(has(&e, "ya la posee"), "{e:?}");
    // modo no declarado, tipo y direccion invalidos, dependencia inexistente
    let e = mutated(|p| p.peripherals.peripherals.iter_mut().find(|x| x.name == "MSSP").unwrap().modes.clear());
    assert!(has(&e, "el modo 'SPI'"), "{e:?}");
    let e = mutated(|p| p.peripherals.peripherals.iter_mut().find(|x| x.name == "WDT").unwrap().kind = "raro".into());
    assert!(has(&e, "tipo 'raro'"), "{e:?}");
    let e = mutated(|p| p.peripherals.peripherals.iter_mut().find(|x| x.name == "EUSART").unwrap().pins[0].direction = "sideways".into());
    assert!(has(&e, "direccion 'sideways'"), "{e:?}");
    let e = mutated(|p| p.peripherals.peripherals.iter_mut().find(|x| x.name == "CCP2").unwrap().uses.push("TIMER9".into()));
    assert!(has(&e, "usa TIMER9, que no existe"), "{e:?}");
}

#[test]
fn peripherals_must_agree_with_device_json_counts() {
    let e = mutated(|p| p.peripherals.peripherals.retain(|x| x.name != "TIMER2"));
    assert!(has(&e, "cantidad de temporizadores"), "{e:?}");
    let e = mutated(|p| {
        p.peripherals.peripherals.iter_mut().find(|x| x.name == "ADC").unwrap().pins.pop();
    });
    assert!(has(&e, "canales o bits del ADC") || has(&e, "VREF-"), "{e:?}");
    let e = mutated(|p| p.peripherals.peripherals.iter_mut().find(|x| x.name == "COMPARATORS").unwrap().enable.pop().map(|_| ()).unwrap());
    assert!(has(&e, "cantidad de comparadores"), "{e:?}");
    let e = mutated(|p| p.peripherals.peripherals.retain(|x| x.name != "MSSP"));
    assert!(has(&e, "CCP, ECCP, EUSART o MSSP"), "{e:?}");
}

// ---- configuracion por nombre (#33) -----------------------------------------------------------------

#[test]
fn the_build_words_are_the_packs_defaults_for_pypic() {
    // CONFIG1 = INTOSCIO, WDT apagado, MCLR interno (0x20D4); CONFIG2 sin programar
    assert_eq!(pack().config.build_words(), vec![(0x2007, 0x20D4), (0x2008, 0x3FFF)]);
}

#[test]
fn settings_apply_by_name_on_top_of_the_build_words() {
    let p = pack();
    let mut w = p.config.build_words();
    p.config.apply(&mut w, "WDTE", "ON").unwrap();
    p.config.apply(&mut w, "FOSC", "XT").unwrap();
    // 0x20D4 con WDTE=1 (bit 3) y FOSC=001
    assert_eq!(w[0].1, (0x20D4 & !0b1111) | 0b1000 | 0b001);
    p.config.apply(&mut w, "BOR4V", "BOR21V").unwrap();
    assert_eq!(w[1].1, 0x3FFF & !(1 << 8));
    // un numero tambien vale, y una palabra entera
    p.config.apply(&mut w, "FOSC", "2").unwrap();
    assert_eq!(w[0].1 & 7, 2);
    p.config.apply(&mut w, "CONFIG1", "0x1234").unwrap();
    assert_eq!(w[0].1, 0x1234);
}

#[test]
fn settings_that_do_not_exist_list_the_valid_choices() {
    let p = pack();
    let mut w = p.config.build_words();
    let e = p.config.apply(&mut w, "NOPE", "ON").unwrap_err();
    assert!(e.contains("WDTE") && e.contains("FOSC") && e.contains("CONFIG1"), "{e}");
    let e = p.config.apply(&mut w, "WDTE", "MAYBE").unwrap_err();
    assert!(e.contains("ON (") && e.contains("OFF (") && e.contains("0..=1"), "{e}");
    let e = p.config.apply(&mut w, "FOSC", "9").unwrap_err();
    assert!(e.contains("INTRC_NOCLKOUT"), "{e}");
    assert!(p.config.apply(&mut w, "CONFIG1", "0x4000").is_err());
}

#[test]
fn a_build_default_that_does_not_fit_is_a_pack_error() {
    let e = mutated(|p| p.config.words[0].build_default = Some(pypic_device::Num(0x4000)));
    assert!(e.iter().any(|m| m.contains("build_default")), "{e:?}");
}
