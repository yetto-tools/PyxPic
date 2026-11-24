//! La CLI de punta a punta: el dispositivo lo declara el programa y el pack se resuelve solo.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use pypic_pic14::Image;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().unwrap()
}

fn tmp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("pypic-cli-{}-{name}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn pypic(cwd: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_pypic")).current_dir(cwd).args(args).output().unwrap()
}

fn text(o: &Output) -> String {
    format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr))
}

#[test]
fn asm_warns_when_numeric_config_value_is_truncated() {
    let dir = tmp("config-warning");
    let source = dir.join("config.s");
    let output = dir.join("config.hex");
    std::fs::write(&source, "PROCESSOR 16F887\nCONFIG WRT = 0x100\nPSECT test,class=CODE,delta=2,abs\n nop\nEND\n").unwrap();
    let result = pypic(&root(), &["asm", source.to_str().unwrap(), "-o", output.to_str().unwrap()]);
    assert!(result.status.success(), "{}", text(&result));
    assert!(text(&result).contains("warning[ASMW001]") && text(&result).contains("se usa 0"), "{}", text(&result));
    let image = Image::from_intel_hex(&std::fs::read_to_string(output).unwrap()).unwrap();
    assert_eq!(image.get(0x2008), Some(0x39ff));
}

#[test]
fn pic_as_blink_matches_native_assembler_when_xc8_and_dfp_are_configured() {
    let (Ok(pic_as), Ok(dfp)) = (std::env::var("PYPIC_TEST_PIC_AS"), std::env::var("PYPIC_TEST_XC8_DFP")) else {
        eprintln!("comparación XC8 omitida: configurar PYPIC_TEST_PIC_AS y PYPIC_TEST_XC8_DFP");
        return;
    };
    let dir = tmp("xc8-compat");
    let official = dir.join("official.hex");
    let native = dir.join("native.hex");
    let source = "examples/asm/blink_rd0.s";
    let options = "-Wl,-presetVec=0h,-pintentry=4h";
    let xc8 = Command::new(pic_as).current_dir(root())
        .arg("-mcpu=16F887").arg(format!("-mdfp={dfp}"))
        .args(["-xassembler-with-cpp", source, options, "-o"])
        .arg(&official).output().unwrap();
    assert!(xc8.status.success(), "{}", text(&xc8));
    let own = pypic(&root(), &["asm", source, options, "-o", native.to_str().unwrap()]);
    assert!(own.status.success(), "{}", text(&own));
    let read = |path| Image::from_intel_hex(&std::fs::read_to_string(path).unwrap()).unwrap();
    assert_eq!(read(official), read(native));
}

#[test]
fn pic_as_wrt_256_matches_native_config_when_xc8_is_configured() {
    let (Ok(pic_as), Ok(dfp)) = (std::env::var("PYPIC_TEST_PIC_AS"), std::env::var("PYPIC_TEST_XC8_DFP")) else {
        eprintln!("comparación XC8 omitida: configurar PYPIC_TEST_PIC_AS y PYPIC_TEST_XC8_DFP");
        return;
    };
    let dir = tmp("xc8-wrt-256");
    let source = dir.join("wrt.s");
    let official = dir.join("official.hex");
    let native = dir.join("native.hex");
    std::fs::write(&source, "PROCESSOR 16F887\nCONFIG WRT = 256\nPSECT test,class=CODE,delta=2,abs\n nop\nEND\n").unwrap();
    let xc8 = Command::new(pic_as).current_dir(root()).arg("-mcpu=16F887")
        .arg(format!("-mdfp={dfp}")).arg(&source).arg("-o").arg(&official).output().unwrap();
    assert!(xc8.status.success(), "{}", text(&xc8));
    let own = pypic(&root(), &["asm", source.to_str().unwrap(), "-o", native.to_str().unwrap()]);
    assert!(own.status.success(), "{}", text(&own));
    let read = |path| Image::from_intel_hex(&std::fs::read_to_string(path).unwrap()).unwrap();
    // XC8 añade CONFIG1 por defecto, así que se compara la palabra configurada.
    assert_eq!(read(&official).get(0x2008), read(&native).get(0x2008));
    assert_eq!(read(&native).get(0x2008), Some(0x3dff));
}

#[test]
fn pic_as_numeric_config_values_match_when_xc8_is_configured() {
    let (Ok(pic_as), Ok(dfp)) = (std::env::var("PYPIC_TEST_PIC_AS"), std::env::var("PYPIC_TEST_XC8_DFP")) else {
        eprintln!("comparación XC8 omitida: configurar PYPIC_TEST_PIC_AS y PYPIC_TEST_XC8_DFP");
        return;
    };
    let dir = tmp("xc8-config-numeric");
    let cases = [("WRT", "-1", 0x2008), ("WRT", "0", 0x2008), ("WRT", "1", 0x2008), ("WRT", "2", 0x2008), ("WRT", "3", 0x2008), ("WRT", "4", 0x2008), ("WRT", "0x100", 0x2008), ("WDTE", "2", 0x2007), ("FOSC", "0x10", 0x2007)];
    for (index, (field, value, address)) in cases.iter().enumerate() {
        let source = dir.join(format!("wrt{index}.s"));
        let official = dir.join(format!("official{index}.hex"));
        let native = dir.join(format!("native{index}.hex"));
        std::fs::write(&source, format!("PROCESSOR 16F887\nCONFIG {field} = {value}\nPSECT test,class=CODE,delta=2,abs\n nop\nEND\n")).unwrap();
        let xc8 = Command::new(&pic_as).current_dir(root()).arg("-mcpu=16F887")
            .arg(format!("-mdfp={dfp}")).arg(&source).arg("-o").arg(&official).output().unwrap();
        let own = pypic(&root(), &["asm", source.to_str().unwrap(), "-o", native.to_str().unwrap()]);
        assert_eq!(own.status.success(), xc8.status.success(), "{field}={value}: XC8: {} PyPIC: {}", text(&xc8), text(&own));
        if xc8.status.success() {
            let read = |path| Image::from_intel_hex(&std::fs::read_to_string(path).unwrap()).unwrap();
            assert_eq!(read(&official).get(*address), read(&native).get(*address), "{field}={value}");
        }
    }
}

#[test]
fn disasm_pic_as_preserves_program_gaps_and_configuration_words() {
    let dir = tmp("disasm-pic-as");
    let src = dir.join("source.hex");
    let asm = dir.join("disassembled.s");
    let out = dir.join("reassembled.hex");
    let mut image = Image::new();
    for (address, word) in [(0, 0x3001), (1, 0x0061), (0x10, 0x3fff), (0x2007, 0x20d4)] {
        image.set(address, word).unwrap();
    }
    std::fs::write(&src, image.to_intel_hex(16)).unwrap();
    let dis = pypic(&root(), &["disasm", src.to_str().unwrap(), "--pic-as"]);
    assert!(dis.status.success(), "{}", text(&dis));
    std::fs::write(&asm, dis.stdout).unwrap();
    let assembled = pypic(&root(), &["asm", asm.to_str().unwrap(), "-o", out.to_str().unwrap()]);
    assert!(assembled.status.success(), "{}", text(&assembled));
    let roundtrip = Image::from_intel_hex(&std::fs::read_to_string(out).unwrap()).unwrap();
    assert_eq!(roundtrip, image);
}

#[test]
fn disasm_pic_as_uses_device_pack_configuration_and_vectors() {
    let dir = tmp("disasm-pic-as-device");
    let input = dir.join("input.hex");
    let source = dir.join("output.s");
    let output = dir.join("output.hex");
    let mut image = Image::new();
    for (addr, word) in [(0, 0x3001), (4, 0x0008), (0x2007, 0x20d4), (0x2008, 0x3fff)] {
        image.set(addr, word).unwrap();
    }
    std::fs::write(&input, image.to_intel_hex(16)).unwrap();
    let result = pypic(&root(), &["disasm", input.to_str().unwrap(), "--pic-as", "-mcpu=16F887"]);
    assert!(result.status.success(), "{}", text(&result));
    let asm = String::from_utf8(result.stdout).unwrap();
    assert!(asm.contains("PROCESSOR 16F887") && asm.contains("CONFIG CONFIG1") && asm.contains("pypic_interrupt:"), "{asm}");
    std::fs::write(&source, asm).unwrap();
    let result = pypic(&root(), &["asm", source.to_str().unwrap(), "-o", output.to_str().unwrap()]);
    assert!(result.status.success(), "{}", text(&result));
    assert_eq!(Image::from_intel_hex(&std::fs::read_to_string(output).unwrap()).unwrap(), image);
}

#[test]
fn build_finds_the_device_pack_from_the_programs_declaration() {
    let out = tmp("build").join("button_led.hex");
    let o = pypic(&root(), &["build", "examples/button_led.py", "-o", out.to_str().unwrap()]);
    assert!(o.status.success(), "{}", text(&o));
    assert!(text(&o).contains("PIC16F887") && text(&o).contains("4000000 Hz"), "{}", text(&o));
    let img = Image::from_intel_hex(&std::fs::read_to_string(&out).unwrap()).unwrap();
    assert_eq!(img.get(0x2007), Some(0x20D4)); // palabra de configuracion en la direccion del pack
    assert!(img.get(0).is_some());
}

#[test]
fn build_works_from_any_directory_with_pypic_devices() {
    let out = tmp("elsewhere").join("c.hex");
    let o = Command::new(env!("CARGO_BIN_EXE_pypic"))
        .current_dir(tmp("elsewhere"))
        .env("PYPIC_DEVICES", root().join("devices"))
        .args(["build", root().join("examples/counter.py").to_str().unwrap(), "-o", out.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(o.status.success(), "{}", text(&o));
    assert!(out.exists());
}

#[test]
fn build_requires_the_declaration() {
    let dir = tmp("nodecl");
    let src = dir.join("p.py");
    std::fs::write(&src, "def main() -> None:\n    pass\n").unwrap();
    let o = pypic(&root(), &["build", src.to_str().unwrap(), "-o", dir.join("p.hex").to_str().unwrap()]);
    assert_eq!(o.status.code(), Some(1));
    assert!(text(&o).contains("from pypic.device import"), "{}", text(&o));
    assert!(!dir.join("p.hex").exists());
    // `check` si funciona sin dispositivo (analisis generico)
    assert!(pypic(&root(), &["check", src.to_str().unwrap()]).status.success());
}

#[test]
fn unknown_device_lists_the_available_ones() {
    let dir = tmp("unknown");
    let src = dir.join("p.py");
    std::fs::write(&src, "from pypic.device import PIC99F999\ndef main() -> None:\n    pass\n").unwrap();
    let o = pypic(&root(), &["build", src.to_str().unwrap()]);
    assert_eq!(o.status.code(), Some(1));
    assert!(text(&o).contains("PIC99F999") && text(&o).contains("PIC16F887"), "{}", text(&o));
}

#[test]
fn a_device_without_a_backend_is_refused_with_a_clear_error() {
    // un pack valido de otro dispositivo (copia con otro nombre): el backend no lo conoce todavia
    let dir = tmp("otherdev");
    let pack = dir.join("devices/pic16f886");
    std::fs::create_dir_all(&pack).unwrap();
    for f in ["device.json", "memory.json", "pins.json", "config.json", "registers.json", "peripherals.json"] {
        let s = std::fs::read_to_string(root().join("devices/pic16f887").join(f)).unwrap();
        std::fs::write(pack.join(f), s.replace("\"PIC16F887\"", "\"PIC16F886\"")).unwrap();
    }
    let src = dir.join("p.py");
    std::fs::write(&src, "from pypic.device import PIC16F886\ndef main() -> None:\n    pass\n").unwrap();
    let o = pypic(&dir, &["build", src.to_str().unwrap(), "-o", dir.join("p.hex").to_str().unwrap()]);
    assert_eq!(o.status.code(), Some(1), "{}", text(&o));
    assert!(text(&o).contains("no soporta") && text(&o).contains("PIC16F886"), "{}", text(&o));
}

#[test]
fn declaring_one_device_and_forcing_another_pack_is_an_error() {
    let dir = tmp("mismatch");
    let src = dir.join("p.py");
    std::fs::write(&src, "from pypic.device import PIC12F625\ndef main() -> None:\n    pass\n").unwrap();
    let o = pypic(&root(), &["check", src.to_str().unwrap(), "--device", "devices/pic16f887"]);
    assert_eq!(o.status.code(), Some(1));
    assert!(text(&o).contains("PYP2090"), "{}", text(&o));
}

#[test]
fn clock_above_the_device_maximum_is_reported_with_the_source_location() {
    let dir = tmp("clock");
    let src = dir.join("p.py");
    std::fs::write(&src, "from pypic.device import PIC16F887\nCLOCK_HZ: Const[u32] = 25_000_000\ndef main() -> None:\n    pass\n").unwrap();
    let o = pypic(&root(), &["check", src.to_str().unwrap()]);
    assert_eq!(o.status.code(), Some(1));
    assert!(text(&o).contains(":2:1: error[PYP2094]") && text(&o).contains("20000000"), "{}", text(&o));
}

#[test]
fn pins_are_validated_against_the_declared_device() {
    let dir = tmp("pins");
    let src = dir.join("p.py");
    std::fs::write(&src, "from pypic.device import PIC16F887\nfrom pypic.periph.gpio import RD9\ndef main() -> None:\n    pass\n").unwrap();
    let o = pypic(&root(), &["check", src.to_str().unwrap()]);
    assert_eq!(o.status.code(), Some(1));
    assert!(text(&o).contains("PYP2074") && text(&o).contains("RD0"), "{}", text(&o));
}

#[test]
fn the_shipped_examples_all_build() {
    for ex in ["examples/counter.py", "examples/button_led.py"] {
        let out = tmp("examples").join(format!("{}.hex", Path::new(ex).file_stem().unwrap().to_string_lossy()));
        let o = pypic(&root(), &["build", ex, "-o", out.to_str().unwrap()]);
        assert!(o.status.success(), "{ex}: {}", text(&o));
    }
}

// ---- esperas: reloj por defecto, una sola forma, informe de tiempos ----------------------------------

#[test]
fn blink_example_builds_and_the_timing_report_shows_exact_cycles() {
    let out = tmp("blink").join("blink.hex");
    let o = pypic(&root(), &["build", "examples/blink.py", "--timing", "-o", out.to_str().unwrap()]);
    assert!(o.status.success(), "{}", text(&o));
    let t = text(&o);
    assert!(t.contains("500 ms -> 500000 ciclos") && t.contains("+0.0 ns"), "{t}");
    assert!(out.exists());
}

#[test]
fn a_missing_clock_hz_uses_the_device_default_and_says_so() {
    let dir = tmp("noclock");
    let src = dir.join("p.py");
    std::fs::write(&src, "from pypic.device import PIC16F887\nfrom pypic.system.time import pause\ndef main() -> None:\n    pause(1)\n").unwrap();
    let o = pypic(&root(), &["build", src.to_str().unwrap(), "-o", dir.join("p.hex").to_str().unwrap()]);
    assert!(o.status.success(), "{}", text(&o));
    let t = text(&o);
    assert!(t.contains("PYP3004") && t.contains("(por defecto)") && t.contains("4000000"), "{t}");
}

#[test]
fn removed_aliases_point_to_the_single_name() {
    let dir = tmp("alias");
    let src = dir.join("p.py");
    std::fs::write(&src, "from pypic.device import PIC16F887\nfrom pypic.system.time import delay_ms\ndef main() -> None:\n    pass\n").unwrap();
    let o = pypic(&root(), &["check", src.to_str().unwrap()]);
    assert_eq!(o.status.code(), Some(1));
    assert!(text(&o).contains("PYP2104") && text(&o).contains("pause"), "{}", text(&o));
}

// ---- ensamblador ---------------------------------------------------------------------------------------

#[test]
fn asm_reproduces_the_proteus_verified_hex_byte_for_byte() {
    // el dispositivo sale de la directiva PROCESSOR; los vectores se fijan con las opciones del enlazador de pic-as
    let out = tmp("asm").join("blink_rd0.hex");
    let o = pypic(&root(), &["asm", "examples/asm/blink_rd0.s", "-Wl,-presetVec=0h,-pintentry=4h", "-o", out.to_str().unwrap()]);
    assert!(o.status.success(), "{}", text(&o));
    let made = std::fs::read_to_string(&out).unwrap().replace("
", "
");
    let verified = std::fs::read_to_string(root().join("examples/hex/blink_rd0.hex")).unwrap().replace("
", "
");
    assert_eq!(made, verified);
}

#[test]
fn asm_lists_and_reports_errors_with_location() {
    let dir = tmp("asmerr");
    let good = dir.join("g.s");
    std::fs::write(&good, "PSECT t,class=CODE,delta=2,abs
    movlw 0x2A
").unwrap();
    let o = pypic(&root(), &["asm", good.to_str().unwrap(), "-o", dir.join("g.hex").to_str().unwrap(), "--listing"]);
    assert!(o.status.success() && text(&o).contains("0000 302A      movlw 0x2A"), "{}", text(&o));
    let bad = dir.join("b.s");
    std::fs::write(&bad, "PSECT t,class=CODE,delta=2,abs
    MOVLB 1
    MOVLW 999
").unwrap();
    let o = pypic(&root(), &["asm", bad.to_str().unwrap(), "-o", dir.join("b.hex").to_str().unwrap()]);
    assert_eq!(o.status.code(), Some(1));
    let t = text(&o);
    assert!(t.contains(":2:5: error[ASM002]") && t.contains(":3:11: error[ASM004]"), "{t}");
    assert!(!dir.join("b.hex").exists());
}

#[test]
fn asm_takes_the_device_from_mcpu_and_rejects_a_wrong_processor() {
    let dir = tmp("asmcpu");
    let f = dir.join("c.s");
    std::fs::write(&f, "#include <xc.inc>
CONFIG WDTE = OFF
PSECT code,abs
    bsf STATUS,STATUS_RP0_POSN
").unwrap();
    let hex = dir.join("c.hex");
    let o = pypic(&root(), &["asm", f.to_str().unwrap(), "-mcpu=16F887", "-o", hex.to_str().unwrap()]);
    assert!(o.status.success(), "{}", text(&o));
    // sin dispositivo, xc.inc no se puede incluir
    let o = pypic(&root(), &["asm", f.to_str().unwrap(), "-o", hex.to_str().unwrap()]);
    assert_eq!(o.status.code(), Some(1));
    assert!(text(&o).contains("dispositivo"), "{}", text(&o));
    // un dispositivo sin Device Pack se dice con claridad
    let o = pypic(&root(), &["asm", f.to_str().unwrap(), "-mcpu=16F999", "-o", hex.to_str().unwrap()]);
    assert!(!o.status.success() && text(&o).contains("desconocido"), "{}", text(&o));
}

// ---- configuracion por nombre (#33) -----------------------------------------------------------------

const BLINK: &str = "from pypic.device import PIC16F887\nfrom pypic.periph.gpio import RD0\nCLOCK_HZ: Const[u32] = 4_000_000\ndef main() -> None:\n    RD0.output()\n    RD0.on()\n    while True:\n        pass\n";

fn hex_word(hex: &str, addr: u32) -> Option<u16> {
    let img = pypic_pic14::Image::from_intel_hex(hex).unwrap();
    img.get(addr)
}

#[test]
fn build_programs_the_packs_default_configuration_when_none_is_requested() {
    let dir = tmp("cfg0");
    let src = dir.join("p.py");
    std::fs::write(&src, BLINK).unwrap();
    let out = dir.join("p.hex");
    let o = pypic(&root(), &["build", src.to_str().unwrap(), "-o", out.to_str().unwrap()]);
    assert!(o.status.success() && text(&o).contains("CONFIG1=0x20D4"), "{}", text(&o));
    let hex = std::fs::read_to_string(&out).unwrap();
    assert_eq!((hex_word(&hex, 0x2007), hex_word(&hex, 0x2008)), (Some(0x20D4), Some(0x3FFF)));
}

#[test]
fn config_by_name_changes_only_the_requested_fields() {
    let dir = tmp("cfg1");
    let src = dir.join("p.py");
    std::fs::write(&src, BLINK).unwrap();
    let out = dir.join("p.hex");
    let o = pypic(&root(), &["build", src.to_str().unwrap(), "--config", "WDTE=ON,FOSC=XT", "--config", "BOR4V=BOR21V", "-o", out.to_str().unwrap()]);
    assert!(o.status.success(), "{}", text(&o));
    assert!(text(&o).contains("3 ajuste(s) pedidos"), "{}", text(&o));
    let hex = std::fs::read_to_string(&out).unwrap();
    assert_eq!(hex_word(&hex, 0x2007), Some((0x20D4 & !0b1111) | 0b1000 | 0b001));
    assert_eq!(hex_word(&hex, 0x2008), Some(0x3FFF & !(1 << 8)));
}

#[test]
fn config_errors_say_what_is_valid_and_write_nothing() {
    let dir = tmp("cfg2");
    let src = dir.join("p.py");
    std::fs::write(&src, BLINK).unwrap();
    let out = dir.join("p.hex");
    let o = pypic(&root(), &["build", src.to_str().unwrap(), "--config", "WDTE=MAYBE", "-o", out.to_str().unwrap()]);
    assert_eq!(o.status.code(), Some(1));
    assert!(text(&o).contains("ON (") && text(&o).contains("OFF ("), "{}", text(&o));
    assert!(!out.exists());
    let o = pypic(&root(), &["build", src.to_str().unwrap(), "--config", "NOPE=1", "-o", out.to_str().unwrap()]);
    assert!(!o.status.success() && text(&o).contains("no es un ajuste"), "{}", text(&o));
    let o = pypic(&root(), &["build", src.to_str().unwrap(), "--config", "WDTE=ON,WDTE=OFF", "-o", out.to_str().unwrap()]);
    assert!(!o.status.success() && text(&o).contains("dos veces"), "{}", text(&o));
    let o = pypic(&root(), &["build", src.to_str().unwrap(), "--config", "WDTE", "-o", out.to_str().unwrap()]);
    assert_eq!(o.status.code(), Some(2));
}

// ---- Intel HEX mal formado (#61) -----------------------------------------------------------------------

#[test]
fn disasm_rejects_malformed_control_records_with_a_failing_exit_status() {
    let dir = tmp("hexbad");
    // los dos archivos del issue: checksum correcto, pero un EOF con datos y un tipo 04 con direccion
    for (name, body, needle) in [
        ("eof_con_datos.hex", ":0100000100FE\n", "registro 01"),
        ("ext_con_direccion.hex", ":020002040000F8\n:00000001FF\n", "registro 04"),
    ] {
        let f = dir.join(name);
        std::fs::write(&f, body).unwrap();
        let o = pypic(&root(), &["disasm", f.to_str().unwrap()]);
        assert_eq!(o.status.code(), Some(1), "{name}: {}", text(&o));
        assert!(text(&o).contains(needle) && text(&o).contains(":1") || text(&o).contains(needle), "{name}: {}", text(&o));
        assert!(!text(&o).contains("0 palabras"), "{name}: no debe informar una imagen vacia como exito");
    }
    // un archivo valido y vacio sigue siendo correcto
    let ok = dir.join("vacio.hex");
    std::fs::write(&ok, ":00000001FF\n").unwrap();
    assert!(pypic(&root(), &["disasm", ok.to_str().unwrap()]).status.success());
}
