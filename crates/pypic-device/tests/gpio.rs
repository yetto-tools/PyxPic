//! `from pypic.gpio import ...` validado contra los pines reales del Device Pack del PIC16F887.

use std::path::PathBuf;

use pypic_device::DevicePack;
use pypic_frontend::{analyze, TargetProfile};

fn target() -> TargetProfile {
    DevicePack::load(&PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../devices/pic16f887")).unwrap().target_profile()
}

fn codes(src: &str) -> Vec<&'static str> {
    analyze(src, &target()).diagnostics.codes()
}

#[test]
fn profile_carries_the_pack_pins_with_analog_channels() {
    let t = target();
    let pins = t.pins.as_ref().unwrap();
    assert_eq!(pins.len(), 36); // 35 de E/S + RE3 (solo entrada)
    let rb0 = &pins["RB0"];
    assert_eq!((rb0.port, rb0.bit, rb0.input_only, rb0.analog), ('B', 0, false, Some(12))); // AN12
    assert_eq!(pins["RD0"].analog, None); // RD0 es solo digital
    assert_eq!(pins["RA5"].analog, Some(4)); // AN4
    assert_eq!(pins["RE0"].analog, Some(5)); // AN5
    assert!(pins["RE3"].input_only);
    assert!(!pins.contains_key("VDD") && !pins.contains_key("VSS"));
}

#[test]
fn real_pins_are_accepted_and_missing_ones_are_not() {
    assert!(analyze("from pypic.gpio import RD0, RB0, RA0, RE3, RC7\ndef main() -> None:\n    pass\n", &target()).program.is_some());
    assert_eq!(codes("from pypic.gpio import RD9\n"), vec!["PYP2074"]); // el F887 no tiene RD9
    assert_eq!(codes("from pypic.gpio import RF0\n"), vec!["PYP2074"]); // ni puerto F
    assert_eq!(codes("from pypic.gpio import VDD\n"), vec!["PYP2074"]); // alimentacion, no E/S
}

#[test]
fn error_lists_the_available_pins() {
    let src = "from pypic.gpio import RD9\n";
    let out = analyze(src, &target()).diagnostics.render(src, "t.py");
    assert!(out.contains("PIC16F887") && out.contains("RD0") && out.contains("RE3"), "{out}");
}

#[test]
fn input_only_pin_cannot_be_an_output() {
    assert_eq!(codes("from pypic.gpio import RE3\ndef main() -> None:\n    RE3.output()\n"), vec!["PYP2075"]);
    let ok = "from pypic.gpio import RE3\ndef main() -> None:\n    RE3.input()\n    x: u8 = RE3.value()\n";
    assert!(analyze(ok, &target()).program.is_some());
}

#[test]
fn the_ir_keeps_port_bit_and_analog_channel() {
    let src = "from pypic.gpio import RB0, RD0\ndef main() -> None:\n    RB0.output()\n    RD0.output()\n";
    let p = analyze(src, &target()).program.unwrap();
    let pins = &p.pins;
    assert_eq!((pins[0].pin.as_str(), pins[0].port, pins[0].bit, pins[0].analog), ("RB0", 'B', 0, Some(12)));
    assert_eq!((pins[1].pin.as_str(), pins[1].port, pins[1].bit, pins[1].analog), ("RD0", 'D', 0, None));
}
