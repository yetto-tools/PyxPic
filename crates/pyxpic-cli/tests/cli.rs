//! La CLI de punta a punta: el dispositivo lo declara el programa y el pack se resuelve solo.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use pyxpic_pic14::Image;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().unwrap()
}

fn tmp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("pyxpic-cli-{}-{name}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn pyxpic(cwd: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_pyxpic")).current_dir(cwd).args(args).output().unwrap()
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
    let result = pyxpic(&root(), &["asm", source.to_str().unwrap(), "-o", output.to_str().unwrap()]);
    assert!(result.status.success(), "{}", text(&result));
    assert!(text(&result).contains("warning[ASMW001]") && text(&result).contains("se usa 0"), "{}", text(&result));
    let image = Image::from_intel_hex(&std::fs::read_to_string(output).unwrap()).unwrap();
    assert_eq!(image.get(0x2008), Some(0x39ff));
}

#[test]
fn pic_as_blink_matches_native_assembler_when_xc8_and_dfp_are_configured() {
    let (Ok(pic_as), Ok(dfp)) = (std::env::var("PYXPIC_TEST_PIC_AS"), std::env::var("PYXPIC_TEST_XC8_DFP")) else {
        eprintln!("comparación XC8 omitida: configurar PYXPIC_TEST_PIC_AS y PYXPIC_TEST_XC8_DFP");
        return;
    };
    let dir = tmp("xc8-compat");
    let official = dir.join("official.hex");
    let native = dir.join("native.hex");
    let source = "examples/asm/gpio/blink_rd0.s";
    let options = "-Wl,-presetVec=0h,-pintentry=4h";
    let xc8 = Command::new(pic_as).current_dir(root()).arg("-mcpu=16F887").arg(format!("-mdfp={dfp}")).args(["-xassembler-with-cpp", source, options, "-o"]).arg(&official).output().unwrap();
    assert!(xc8.status.success(), "{}", text(&xc8));
    let own = pyxpic(&root(), &["asm", source, options, "-o", native.to_str().unwrap()]);
    assert!(own.status.success(), "{}", text(&own));
    let read = |path| Image::from_intel_hex(&std::fs::read_to_string(path).unwrap()).unwrap();
    assert_eq!(read(official), read(native));
}

#[test]
fn pic_as_wrt_256_matches_native_config_when_xc8_is_configured() {
    let (Ok(pic_as), Ok(dfp)) = (std::env::var("PYXPIC_TEST_PIC_AS"), std::env::var("PYXPIC_TEST_XC8_DFP")) else {
        eprintln!("comparación XC8 omitida: configurar PYXPIC_TEST_PIC_AS y PYXPIC_TEST_XC8_DFP");
        return;
    };
    let dir = tmp("xc8-wrt-256");
    let source = dir.join("wrt.s");
    let official = dir.join("official.hex");
    let native = dir.join("native.hex");
    std::fs::write(&source, "PROCESSOR 16F887\nCONFIG WRT = 256\nPSECT test,class=CODE,delta=2,abs\n nop\nEND\n").unwrap();
    let xc8 = Command::new(pic_as).current_dir(root()).arg("-mcpu=16F887").arg(format!("-mdfp={dfp}")).arg(&source).arg("-o").arg(&official).output().unwrap();
    assert!(xc8.status.success(), "{}", text(&xc8));
    let own = pyxpic(&root(), &["asm", source.to_str().unwrap(), "-o", native.to_str().unwrap()]);
    assert!(own.status.success(), "{}", text(&own));
    let read = |path| Image::from_intel_hex(&std::fs::read_to_string(path).unwrap()).unwrap();
    // XC8 añade CONFIG1 por defecto, así que se compara la palabra configurada.
    assert_eq!(read(&official).get(0x2008), read(&native).get(0x2008));
    assert_eq!(read(&native).get(0x2008), Some(0x3dff));
}

#[test]
fn pic_as_numeric_config_values_match_when_xc8_is_configured() {
    let (Ok(pic_as), Ok(dfp)) = (std::env::var("PYXPIC_TEST_PIC_AS"), std::env::var("PYXPIC_TEST_XC8_DFP")) else {
        eprintln!("comparación XC8 omitida: configurar PYXPIC_TEST_PIC_AS y PYXPIC_TEST_XC8_DFP");
        return;
    };
    let dir = tmp("xc8-config-numeric");
    let cases = [
        ("WRT", "-1", 0x2008),
        ("WRT", "0", 0x2008),
        ("WRT", "1", 0x2008),
        ("WRT", "2", 0x2008),
        ("WRT", "3", 0x2008),
        ("WRT", "4", 0x2008),
        ("WRT", "0x100", 0x2008),
        ("WDTE", "2", 0x2007),
        ("FOSC", "0x10", 0x2007),
        ("CONFIG1", "0", 0x2007),
        ("CONFIG1", "0x3fff", 0x2007),
        ("CONFIG1", "0x4000", 0x2007),
        ("CONFIG2", "-1", 0x2008),
        ("IDLOC0", "0x80", 0x2000),
    ];
    for (index, (field, value, address)) in cases.iter().enumerate() {
        let source = dir.join(format!("wrt{index}.s"));
        let official = dir.join(format!("official{index}.hex"));
        let native = dir.join(format!("native{index}.hex"));
        std::fs::write(&source, format!("PROCESSOR 16F887\nCONFIG {field} = {value}\nPSECT test,class=CODE,delta=2,abs\n nop\nEND\n")).unwrap();
        let xc8 = Command::new(&pic_as).current_dir(root()).arg("-mcpu=16F887").arg(format!("-mdfp={dfp}")).arg(&source).arg("-o").arg(&official).output().unwrap();
        let own = pyxpic(&root(), &["asm", source.to_str().unwrap(), "-o", native.to_str().unwrap()]);
        assert_eq!(own.status.success(), xc8.status.success(), "{field}={value}: XC8: {} PyxPic: {}", text(&xc8), text(&own));
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
    let dis = pyxpic(&root(), &["disasm", src.to_str().unwrap(), "--pic-as"]);
    assert!(dis.status.success(), "{}", text(&dis));
    std::fs::write(&asm, dis.stdout).unwrap();
    let assembled = pyxpic(&root(), &["asm", asm.to_str().unwrap(), "-o", out.to_str().unwrap()]);
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
    for (addr, word) in [(0, 0x3001), (4, 0x0008), (0x2000, 0x41), (0x2001, 0), (0x2002, 0x20), (0x2003, 0x7f), (0x2007, 0x20d4), (0x2008, 0x3fff)] {
        image.set(addr, word).unwrap();
    }
    std::fs::write(&input, image.to_intel_hex(16)).unwrap();
    let result = pyxpic(&root(), &["disasm", input.to_str().unwrap(), "--pic-as", "-mcpu=16F887"]);
    assert!(result.status.success(), "{}", text(&result));
    let asm = String::from_utf8(result.stdout).unwrap();
    assert!(asm.contains("PROCESSOR 16F887") && asm.contains("CONFIG CONFIG1") && asm.contains("CONFIG IDLOC0 = 0x0041") && asm.contains("pyxpic_interrupt:"), "{asm}");
    std::fs::write(&source, asm).unwrap();
    let result = pyxpic(&root(), &["asm", source.to_str().unwrap(), "-o", output.to_str().unwrap()]);
    assert!(result.status.success(), "{}", text(&result));
    assert_eq!(Image::from_intel_hex(&std::fs::read_to_string(output).unwrap()).unwrap(), image);
    if let (Ok(pic_as), Ok(dfp)) = (std::env::var("PYXPIC_TEST_PIC_AS"), std::env::var("PYXPIC_TEST_XC8_DFP")) {
        let official = dir.join("official.hex");
        let result = Command::new(pic_as).current_dir(root()).arg("-mcpu=16F887").arg(format!("-mdfp={dfp}")).arg(&source).arg("-o").arg(&official).output().unwrap();
        assert!(result.status.success(), "{}", text(&result));
        assert_eq!(Image::from_intel_hex(&std::fs::read_to_string(official).unwrap()).unwrap(), image);
    }
}

#[test]
fn build_finds_the_device_pack_from_the_programs_declaration() {
    let out = tmp("build").join("button_led.hex");
    let o = pyxpic(&root(), &["build", "examples/python/gpio/button_led.py", "-o", out.to_str().unwrap()]);
    assert!(o.status.success(), "{}", text(&o));
    assert!(text(&o).contains("PIC16F887") && text(&o).contains("4000000 Hz"), "{}", text(&o));
    let summary = String::from_utf8_lossy(&o.stdout);
    for label in ["Archivo:", "Dispositivo:", "Programa:", "RAM:", "Reloj:", "Configuracion:"] {
        assert!(summary.lines().any(|line| line.trim_start().starts_with(label)), "{summary}");
    }
    let img = Image::from_intel_hex(&std::fs::read_to_string(&out).unwrap()).unwrap();
    assert_eq!(img.get(0x2007), Some(0x20D4)); // palabra de configuracion en la direccion del pack
    assert!(img.get(0).is_some());
}

#[test]
fn build_works_from_any_directory_with_pyxpic_devices() {
    let out = tmp("elsewhere").join("c.hex");
    let o = Command::new(env!("CARGO_BIN_EXE_pyxpic"))
        .current_dir(tmp("elsewhere"))
        .env("PYXPIC_DEVICES", root().join("devices"))
        .args(["build", root().join("examples/python/gpio/counter.py").to_str().unwrap(), "-o", out.to_str().unwrap()])
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
    let o = pyxpic(&root(), &["build", src.to_str().unwrap(), "-o", dir.join("p.hex").to_str().unwrap()]);
    assert_eq!(o.status.code(), Some(1));
    assert!(text(&o).contains("from pyxpic.device import"), "{}", text(&o));
    assert!(!dir.join("p.hex").exists());
    // `check` si funciona sin dispositivo (analisis generico)
    assert!(pyxpic(&root(), &["check", src.to_str().unwrap()]).status.success());
}

#[test]
fn unknown_device_lists_the_available_ones() {
    let dir = tmp("unknown");
    let src = dir.join("p.py");
    std::fs::write(&src, "from pyxpic.device import PIC99F999\ndef main() -> None:\n    pass\n").unwrap();
    let o = pyxpic(&root(), &["build", src.to_str().unwrap()]);
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
    std::fs::write(&src, "from pyxpic.device import PIC16F886\ndef main() -> None:\n    pass\n").unwrap();
    let o = pyxpic(&dir, &["build", src.to_str().unwrap(), "-o", dir.join("p.hex").to_str().unwrap()]);
    assert_eq!(o.status.code(), Some(1), "{}", text(&o));
    assert!(text(&o).contains("no soporta") && text(&o).contains("PIC16F886"), "{}", text(&o));
}

#[test]
fn declaring_one_device_and_forcing_another_pack_is_an_error() {
    let dir = tmp("mismatch");
    let src = dir.join("p.py");
    std::fs::write(&src, "from pyxpic.device import PIC12F625\ndef main() -> None:\n    pass\n").unwrap();
    let o = pyxpic(&root(), &["check", src.to_str().unwrap(), "--device", "devices/pic16f887"]);
    assert_eq!(o.status.code(), Some(1));
    assert!(text(&o).contains("PYP2090"), "{}", text(&o));
}

#[test]
fn clock_above_the_device_maximum_is_reported_with_the_source_location() {
    let dir = tmp("clock");
    let src = dir.join("p.py");
    std::fs::write(&src, "from pyxpic.device import PIC16F887\nCLOCK_HZ: Const[u32] = 25_000_000\ndef main() -> None:\n    pass\n").unwrap();
    let o = pyxpic(&root(), &["check", src.to_str().unwrap()]);
    assert_eq!(o.status.code(), Some(1));
    assert!(text(&o).contains(":2:1: error[PYP2094]") && text(&o).contains("20000000"), "{}", text(&o));
}

#[test]
fn pins_are_validated_against_the_declared_device() {
    let dir = tmp("pins");
    let src = dir.join("p.py");
    std::fs::write(&src, "from pyxpic.device import PIC16F887\nfrom pyxpic.periph.gpio import RD9\ndef main() -> None:\n    pass\n").unwrap();
    let o = pyxpic(&root(), &["check", src.to_str().unwrap()]);
    assert_eq!(o.status.code(), Some(1));
    assert!(text(&o).contains("PYP2074") && text(&o).contains("RD0"), "{}", text(&o));
}

#[test]
fn the_shipped_examples_all_build() {
    for ex in ["examples/python/gpio/counter.py", "examples/python/gpio/button_led.py"] {
        let out = tmp("examples").join(format!("{}.hex", Path::new(ex).file_stem().unwrap().to_string_lossy()));
        let o = pyxpic(&root(), &["build", ex, "-o", out.to_str().unwrap()]);
        assert!(o.status.success(), "{ex}: {}", text(&o));
    }
}

// ---- esperas: reloj por defecto, una sola forma, informe de tiempos ----------------------------------

#[test]
fn blink_example_builds_and_the_timing_report_shows_exact_cycles() {
    let out = tmp("blink").join("blink.hex");
    let o = pyxpic(&root(), &["build", "examples/python/gpio/blink.py", "--timing", "-o", out.to_str().unwrap()]);
    assert!(o.status.success(), "{}", text(&o));
    let t = text(&o);
    assert!(t.contains("500 ms -> 500000 ciclos") && t.contains("+0.0 ns"), "{t}");
    assert!(out.exists());
}

#[test]
fn a_missing_clock_hz_uses_the_device_default_and_says_so() {
    let dir = tmp("noclock");
    let src = dir.join("p.py");
    std::fs::write(&src, "from pyxpic.device import PIC16F887\nfrom pyxpic.system.time import pause\ndef main() -> None:\n    pause(1)\n").unwrap();
    let o = pyxpic(&root(), &["build", src.to_str().unwrap(), "-o", dir.join("p.hex").to_str().unwrap()]);
    assert!(o.status.success(), "{}", text(&o));
    let t = text(&o);
    assert!(t.contains("PYP3004") && t.contains("(por defecto)") && t.contains("4000000"), "{t}");
}

#[test]
fn removed_aliases_point_to_the_single_name() {
    let dir = tmp("alias");
    let src = dir.join("p.py");
    std::fs::write(&src, "from pyxpic.device import PIC16F887\nfrom pyxpic.system.time import delay_ms\ndef main() -> None:\n    pass\n").unwrap();
    let o = pyxpic(&root(), &["check", src.to_str().unwrap()]);
    assert_eq!(o.status.code(), Some(1));
    assert!(text(&o).contains("PYP2104") && text(&o).contains("pause"), "{}", text(&o));
}

// ---- ensamblador ---------------------------------------------------------------------------------------

#[test]
fn asm_reproduces_the_proteus_verified_hex_byte_for_byte() {
    // el dispositivo sale de la directiva PROCESSOR; los vectores se fijan con las opciones del enlazador de pic-as
    let out = tmp("asm").join("blink_rd0.hex");
    let o = pyxpic(&root(), &["asm", "examples/asm/gpio/blink_rd0.s", "-Wl,-presetVec=0h,-pintentry=4h", "-o", out.to_str().unwrap()]);
    assert!(o.status.success(), "{}", text(&o));
    // el repositorio guarda los .hex con los saltos del sistema (Git los convierte con * text=auto): se compara sin ellos
    let made = std::fs::read_to_string(&out).unwrap().replace("\r\n", "\n");
    let verified = std::fs::read_to_string(root().join("examples/hex/blink_rd0.hex")).unwrap().replace("\r\n", "\n");
    assert_eq!(made, verified);
}

#[test]
fn asm_lists_and_reports_errors_with_location() {
    let dir = tmp("asmerr");
    let good = dir.join("g.s");
    std::fs::write(
        &good,
        "PSECT t,class=CODE,delta=2,abs
    movlw 0x2A
",
    )
    .unwrap();
    let o = pyxpic(&root(), &["asm", good.to_str().unwrap(), "-o", dir.join("g.hex").to_str().unwrap(), "--listing"]);
    assert!(o.status.success() && text(&o).contains("0000 302A      movlw 0x2A"), "{}", text(&o));
    let bad = dir.join("b.s");
    std::fs::write(
        &bad,
        "PSECT t,class=CODE,delta=2,abs
    MOVLB 1
    MOVLW 999
",
    )
    .unwrap();
    let o = pyxpic(&root(), &["asm", bad.to_str().unwrap(), "-o", dir.join("b.hex").to_str().unwrap()]);
    assert_eq!(o.status.code(), Some(1));
    let t = text(&o);
    assert!(t.contains(":2:5: error[ASM002]") && t.contains(":3:11: error[ASM004]"), "{t}");
    assert!(!dir.join("b.hex").exists());
}

#[test]
fn asm_takes_the_device_from_mcpu_and_rejects_a_wrong_processor() {
    let dir = tmp("asmcpu");
    let f = dir.join("c.s");
    std::fs::write(
        &f,
        "#include <xc.inc>
CONFIG WDTE = OFF
PSECT code,abs
    bsf STATUS,STATUS_RP0_POSN
",
    )
    .unwrap();
    let hex = dir.join("c.hex");
    let o = pyxpic(&root(), &["asm", f.to_str().unwrap(), "-mcpu=16F887", "-o", hex.to_str().unwrap()]);
    assert!(o.status.success(), "{}", text(&o));
    // sin dispositivo, xc.inc no se puede incluir
    let o = pyxpic(&root(), &["asm", f.to_str().unwrap(), "-o", hex.to_str().unwrap()]);
    assert_eq!(o.status.code(), Some(1));
    assert!(text(&o).contains("dispositivo"), "{}", text(&o));
    // un dispositivo sin Device Pack se dice con claridad
    let o = pyxpic(&root(), &["asm", f.to_str().unwrap(), "-mcpu=16F999", "-o", hex.to_str().unwrap()]);
    assert!(!o.status.success() && text(&o).contains("desconocido"), "{}", text(&o));
}

// ---- configuracion por nombre (#33) -----------------------------------------------------------------

const BLINK: &str =
    "from pyxpic.device import PIC16F887\nfrom pyxpic.periph.gpio import RD0\nCLOCK_HZ: Const[u32] = 4_000_000\ndef main() -> None:\n    RD0.output()\n    RD0.on()\n    while True:\n        pass\n";

fn hex_word(hex: &str, addr: u32) -> Option<u16> {
    let img = pyxpic_pic14::Image::from_intel_hex(hex).unwrap();
    img.get(addr)
}

#[test]
fn build_programs_the_packs_default_configuration_when_none_is_requested() {
    let dir = tmp("cfg0");
    let src = dir.join("p.py");
    std::fs::write(&src, BLINK).unwrap();
    let out = dir.join("p.hex");
    let o = pyxpic(&root(), &["build", src.to_str().unwrap(), "-o", out.to_str().unwrap()]);
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
    let o = pyxpic(&root(), &["build", src.to_str().unwrap(), "--config", "WDTE=ON,FOSC=XT", "--config", "BOR4V=BOR21V", "-o", out.to_str().unwrap()]);
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
    let o = pyxpic(&root(), &["build", src.to_str().unwrap(), "--config", "WDTE=MAYBE", "-o", out.to_str().unwrap()]);
    assert_eq!(o.status.code(), Some(1));
    assert!(text(&o).contains("ON (") && text(&o).contains("OFF ("), "{}", text(&o));
    assert!(!out.exists());
    let o = pyxpic(&root(), &["build", src.to_str().unwrap(), "--config", "NOPE=1", "-o", out.to_str().unwrap()]);
    assert!(!o.status.success() && text(&o).contains("no es un ajuste"), "{}", text(&o));
    let o = pyxpic(&root(), &["build", src.to_str().unwrap(), "--config", "WDTE=ON,WDTE=OFF", "-o", out.to_str().unwrap()]);
    assert!(!o.status.success() && text(&o).contains("dos veces"), "{}", text(&o));
    let o = pyxpic(&root(), &["build", src.to_str().unwrap(), "--config", "WDTE", "-o", out.to_str().unwrap()]);
    assert_eq!(o.status.code(), Some(2));
}

// ---- Intel HEX mal formado (#61) -----------------------------------------------------------------------

#[test]
fn disasm_rejects_malformed_control_records_with_a_failing_exit_status() {
    let dir = tmp("hexbad");
    // los dos archivos del issue: checksum correcto, pero un EOF con datos y un tipo 04 con direccion
    for (name, body, needle) in [("eof_con_datos.hex", ":0100000100FE\n", "registro 01"), ("ext_con_direccion.hex", ":020002040000F8\n:00000001FF\n", "registro 04")] {
        let f = dir.join(name);
        std::fs::write(&f, body).unwrap();
        let o = pyxpic(&root(), &["disasm", f.to_str().unwrap()]);
        assert_eq!(o.status.code(), Some(1), "{name}: {}", text(&o));
        assert!(text(&o).contains(needle) && text(&o).contains(":1") || text(&o).contains(needle), "{name}: {}", text(&o));
        assert!(!text(&o).contains("0 palabras"), "{name}: no debe informar una imagen vacia como exito");
    }
    // un archivo valido y vacio sigue siendo correcto
    let ok = dir.join("vacio.hex");
    std::fs::write(&ok, ":00000001FF\n").unwrap();
    assert!(pyxpic(&root(), &["disasm", ok.to_str().unwrap()]).status.success());
}

#[test]
fn clock_declaration_sets_fosc_and_rejects_a_conflicting_config() {
    let dir = tmp("clock-source");
    let src = dir.join("reloj.py");
    std::fs::write(&src, "from pyxpic.device import PIC16F887\nCLOCK: Crystal
FREQUENCY: 20MHz\n\n\ndef main() -> None:\n    pass\n").unwrap();
    let out = dir.join("reloj.hex");
    let o = pyxpic(&root(), &["build", src.to_str().unwrap(), "-o", out.to_str().unwrap()]);
    assert!(o.status.success(), "{}", text(&o));
    assert!(text(&o).contains("Crystal 20 MHz (FOSC = HS)"), "{}", text(&o));
    let img = Image::from_intel_hex(&std::fs::read_to_string(&out).unwrap()).unwrap();
    assert_eq!(img.get(0x2007), Some(0x20D2)); // FOSC = HS (010) en lugar del INTOSCIO (100) por defecto: 0x20D4
                                               // pedir otro oscilador a la vez es una contradiccion
    let o = pyxpic(&root(), &["build", src.to_str().unwrap(), "-o", out.to_str().unwrap(), "--config", "FOSC=XT"]);
    assert!(!o.status.success() && text(&o).contains("fija FOSC = HS"), "{}", text(&o));
}

// ---- pyxpic.toml (#37) ----------------------------------------------------------------------------------------------

fn devices() -> String {
    root().join("devices").to_string_lossy().replace('\\', "/")
}

fn project_dir(name: &str, toml: &str, main: &str) -> PathBuf {
    let dir = tmp(name);
    std::fs::write(dir.join("pyxpic.toml"), toml.replace("@DEVICES@", &devices())).unwrap();
    std::fs::write(dir.join("main.py"), main).unwrap();
    dir
}

const BARE_MAIN: &str =
    "from pyxpic.periph.gpio import RD0\nfrom pyxpic.system.time import pause\n\n\ndef main() -> None:\n    RD0.output()\n    while True:\n        RD0.toggle()\n        pause(100)\n";

#[test]
fn a_project_builds_with_no_arguments_taking_device_clock_config_and_output_from_the_profile() {
    let dir = project_dir(
        "proj-basic",
        "[project]\nmain = \"main.py\"\ndevices_dir = \"@DEVICES@\"\n\n[target.placa]\ndevice = \"PIC16F887\"\nclock = \"Crystal 20MHz\"\nconfig = { WDTE = \"ON\" }\noutput = \"out/placa.hex\"\n",
        BARE_MAIN,
    );
    std::fs::create_dir_all(dir.join("out")).unwrap();
    let o = pyxpic(&dir, &["build"]); // sin archivo: el 'main' del proyecto; sin 'from pyxpic.device import ...' en el fuente
    assert!(o.status.success(), "{}", text(&o));
    let t = text(&o);
    assert!(t.contains("PIC16F887") && t.contains("Perfil:") && t.contains("placa") && t.contains("Crystal 20 MHz (FOSC = HS)"), "{t}");
    let img = Image::from_intel_hex(&std::fs::read_to_string(dir.join("out/placa.hex")).unwrap()).unwrap();
    let c1 = img.get(0x2007).unwrap();
    assert_eq!(c1 & 7, 2, "FOSC = HS"); // 010
    assert_ne!(c1 & 0x08, 0, "WDTE = ON desde el perfil");
    // el tiempo se calcula con los 20 MHz del perfil: 100 ms son 500 000 ciclos
    let o = pyxpic(&dir, &["build", "--timing"]);
    assert!(text(&o).contains("500000 ciclos"), "{}", text(&o));
}

#[test]
fn the_program_wins_over_the_profile_and_a_device_mismatch_is_a_clear_error() {
    let toml = "[project]\ndevices_dir = \"@DEVICES@\"\n\n[target.otra]\ndevice = \"PIC16F886\"\n\n[target.ok]\ndevice = \"PIC16F887\"\nclock = \"Crystal 20MHz\"\n";
    let dir = project_dir("proj-mismatch", toml, &format!("from pyxpic.device import PIC16F887\n{BARE_MAIN}"));
    let file = dir.join("main.py");
    let o = pyxpic(&dir, &["build", file.to_str().unwrap(), "--target", "otra"]);
    let t = text(&o);
    assert!(!o.status.success() && t.contains("declara 'from pyxpic.device import PIC16F887'") && t.contains("perfil 'otra'") && t.contains("pide PIC16F886"), "{t}");
    // mismo dispositivo: bien
    let o = pyxpic(&dir, &["build", file.to_str().unwrap(), "--target", "ok"]);
    assert!(o.status.success(), "{}", text(&o));
    // el programa que declara su reloj manda sobre el del perfil, y se avisa
    std::fs::write(&file, format!("from pyxpic.device import PIC16F887\nCLOCK: Internal
FREQUENCY: 8MHz\n{}", BARE_MAIN)).unwrap();
    let o = pyxpic(&dir, &["build", file.to_str().unwrap(), "--target", "ok"]);
    let t = text(&o);
    assert!(o.status.success() && t.contains("aviso") && t.contains("manda el programa") && t.contains("Internal 8 MHz"), "{t}");
    // si el reloj es el mismo, no hay aviso
    std::fs::write(&file, format!("from pyxpic.device import PIC16F887\nCLOCK: Crystal
FREQUENCY: 20MHz\n{}", BARE_MAIN)).unwrap();
    let o = pyxpic(&dir, &["build", file.to_str().unwrap(), "--target", "ok"]);
    assert!(o.status.success() && !text(&o).contains("aviso"), "{}", text(&o));
}

#[test]
fn the_target_is_chosen_with_target_or_default_target_and_the_command_line_wins_over_the_profile() {
    let toml = "[project]\nmain = \"main.py\"\ndevices_dir = \"@DEVICES@\"\n\n[target.a]\ndevice = \"PIC16F887\"\nconfig = { WDTE = \"ON\" }\n\n[target.b]\ndevice = \"PIC16F887\"\nclock = \"Internal 8MHz\"\n";
    let dir = project_dir("proj-targets", toml, BARE_MAIN);
    let o = pyxpic(&dir, &["build"]);
    assert!(!o.status.success() && text(&o).contains("hay varios perfiles (a, b)") && text(&o).contains("--target"), "{}", text(&o));
    let o = pyxpic(&dir, &["build", "--target", "zzz"]);
    assert!(!o.status.success() && text(&o).contains("no hay un perfil 'zzz'"), "{}", text(&o));
    let o = pyxpic(&dir, &["build", "--target", "b"]);
    assert!(o.status.success() && text(&o).contains("Internal 8 MHz"), "{}", text(&o));
    // --config manda sobre el perfil (WDTE = ON en el perfil, OFF en la linea de comandos) y no es un "se indica dos veces"
    let on = pyxpic(&dir, &["build", "--target", "a", "-o", dir.join("on.hex").to_str().unwrap()]);
    let off = pyxpic(&dir, &["build", "--target", "a", "--config", "WDTE=OFF", "-o", dir.join("off.hex").to_str().unwrap()]);
    assert!(on.status.success() && off.status.success(), "{}{}", text(&on), text(&off));
    let word = |f: &str| Image::from_intel_hex(&std::fs::read_to_string(dir.join(f)).unwrap()).unwrap().get(0x2007).unwrap();
    assert_ne!(word("on.hex") & 0x08, 0);
    assert_eq!(word("off.hex") & 0x08, 0);
    // un default_target elige solo
    std::fs::write(
        dir.join("pyxpic.toml"),
        format!(
            "[project]\nmain = \"main.py\"\ndevices_dir = \"{}\"\ndefault_target = \"b\"\n\n[target.a]\ndevice = \"PIC16F887\"\n[target.b]\ndevice = \"PIC16F887\"\nclock = \"Internal 8MHz\"\n",
            devices()
        ),
    )
    .unwrap();
    let o = pyxpic(&dir, &["build"]);
    assert!(o.status.success() && text(&o).contains("Internal 8 MHz"), "{}", text(&o));
}

#[test]
fn project_file_errors_are_located_and_explain_the_valid_keys() {
    let dir = project_dir("proj-errors", "[target.a]\ndevce = \"PIC16F887\"\n", BARE_MAIN);
    let o = pyxpic(&dir, &["check", dir.join("main.py").to_str().unwrap()]);
    assert_eq!(o.status.code(), Some(2), "{}", text(&o));
    assert!(text(&o).contains("[target.a] la clave 'devce' no existe (admite: device, clock, config, output)"), "{}", text(&o));
    let o = pyxpic(&root(), &["check", "examples/python/gpio/blink.py", "--target", "x"]);
    assert!(!o.status.success() && text(&o).contains("--target necesita un pyxpic.toml"), "{}", text(&o));
    // un reloj invalido en el perfil se explica con las reglas del reloj
    let dir = project_dir("proj-badclock", "[project]\ndevices_dir = \"@DEVICES@\"\n[target.a]\ndevice = \"PIC16F887\"\nclock = \"Internal 20MHz\"\n", BARE_MAIN);
    let o = pyxpic(&dir, &["build", dir.join("main.py").to_str().unwrap()]);
    let t = text(&o);
    assert!(!o.status.success() && t.contains("PYP2194") && t.contains("[target.a] clock = \"Internal 20MHz\" no es valido"), "{t}");
    // sin dispositivo en ningun lado: se dice donde ponerlo
    let dir = project_dir("proj-nodevice", "[project]\ndevices_dir = \"@DEVICES@\"\n", "def main() -> None:\n    pass\n");
    let o = pyxpic(&dir, &["build", dir.join("main.py").to_str().unwrap()]);
    assert!(!o.status.success() && text(&o).contains("falta el dispositivo") && text(&o).contains("[target.NOMBRE] device"), "{}", text(&o));
}

#[test]
fn the_shipped_project_example_builds_both_targets_and_creates_the_output_folder() {
    let dir = root().join("examples/python/proyecto");
    let _ = std::fs::remove_dir_all(dir.join("build"));
    let o = pyxpic(&dir, &["build"]);
    assert!(o.status.success(), "{}", text(&o));
    assert!(text(&o).contains("Internal 4 MHz") && dir.join("build/parpadeo_interna.hex").is_file(), "{}", text(&o));
    let o = pyxpic(&dir, &["build", "--target", "cristal"]);
    assert!(o.status.success() && text(&o).contains("Crystal 20 MHz (FOSC = HS)") && dir.join("build/parpadeo_cristal.hex").is_file(), "{}", text(&o));
    let _ = std::fs::remove_dir_all(dir.join("build"));
}

// ---- pyxpic init y pyxpic project check ---------------------------------------------------------------------------------------

fn fresh(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("pyxpic-cli-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    d
}

#[test]
fn init_creates_a_valid_project_that_checks_and_builds_with_no_further_edits() {
    let dir = fresh("init-ok");
    let o = pyxpic(&root(), &["init", dir.to_str().unwrap(), "--name", "termometro", "--devices-dir", &devices()]);
    assert!(o.status.success(), "{}", text(&o));
    assert!(text(&o).contains("Proyecto creado") && text(&o).contains("PIC16F887") && text(&o).contains("Internal 4MHz"), "{}", text(&o));
    let toml = std::fs::read_to_string(dir.join("pyxpic.toml")).unwrap();
    assert!(
        toml.contains("name = \"termometro\"") && toml.contains("[target.pic16f887]") && toml.contains("clock = \"Internal 4MHz\"") && toml.contains("output = \"build/termometro.hex\""),
        "{toml}"
    );
    assert!(std::fs::read_to_string(dir.join("main.py")).unwrap().contains("termometro"));
    // el proyecto recien creado se revisa y se compila sin tocar nada
    let o = pyxpic(&dir, &["project", "check"]);
    assert!(o.status.success() && text(&o).contains("perfil pic16f887: ok") && text(&o).contains("Internal 4 MHz"), "{}", text(&o));
    let o = pyxpic(&dir, &["build"]);
    assert!(o.status.success() && dir.join("build/termometro.hex").is_file(), "{}", text(&o));
}

#[test]
fn init_takes_a_device_alias_and_a_clock_and_validates_both_before_writing() {
    let dir = fresh("init-opts");
    let o = pyxpic(&root(), &["init", dir.to_str().unwrap(), "--device", "16f887", "--clock", "Crystal 20MHz", "--devices-dir", &devices()]);
    assert!(o.status.success(), "{}", text(&o));
    let toml = std::fs::read_to_string(dir.join("pyxpic.toml")).unwrap();
    assert!(toml.contains("device = \"PIC16F887\"") && toml.contains("clock = \"Crystal 20MHz\""), "{toml}");
    let o = pyxpic(&dir, &["project", "check"]);
    assert!(o.status.success() && text(&o).contains("Crystal 20 MHz (FOSC = HS)"), "{}", text(&o));
    // un dispositivo desconocido o un reloj imposible no crean nada
    for (extra, frag) in [(&["--device", "PIC99X999"][..], "desconocido"), (&["--clock", "Internal 20MHz"][..], "PYP2194")] {
        let d = fresh("init-bad");
        let mut args = vec!["init", d.to_str().unwrap(), "--devices-dir"];
        let dev = devices();
        args.push(&dev);
        args.extend_from_slice(extra);
        let o = pyxpic(&root(), &args);
        assert!(!o.status.success() && text(&o).contains(frag), "{}", text(&o));
        assert!(!d.join("pyxpic.toml").exists() && !d.join("main.py").exists(), "no se crea nada si algo no es valido");
    }
}

#[test]
fn init_never_overwrites_an_existing_file() {
    let dir = tmp("init-exists");
    std::fs::write(dir.join("main.py"), "mi programa\n").unwrap();
    let o = pyxpic(&root(), &["init", dir.to_str().unwrap(), "--devices-dir", &devices()]);
    assert_eq!(o.status.code(), Some(1), "{}", text(&o));
    assert!(text(&o).contains("ya existe") && text(&o).contains("main.py") && text(&o).contains("no se sobrescribe nada"), "{}", text(&o));
    assert_eq!(std::fs::read_to_string(dir.join("main.py")).unwrap(), "mi programa\n");
    assert!(!dir.join("pyxpic.toml").exists(), "ni siquiera se crea el otro archivo");
}

#[test]
fn project_check_reports_every_problem_per_target_and_modifies_nothing() {
    let toml = "[project]\nmain = \"main.py\"\ndevices_dir = \"@DEVICES@\"\n\n[target.bien]\ndevice = \"PIC16F887\"\nclock = \"Crystal 20MHz\"\nconfig = { WDTE = \"ON\" }\n\n[target.mal]\ndevice = \"PIC16F886\"\nclock = \"Internal 20MHz\"\nconfig = { WDTE = \"QUIZAS\", NOEXISTE = 1 }\n";
    let dir = project_dir("check-problems", toml, &format!("from pyxpic.device import PIC16F887\n{BARE_MAIN}"));
    let before = (std::fs::read_to_string(dir.join("pyxpic.toml")).unwrap(), std::fs::read_to_string(dir.join("main.py")).unwrap());
    let o = pyxpic(&dir, &["project", "check"]);
    let t = text(&o);
    assert_eq!(o.status.code(), Some(1), "{t}");
    assert!(t.contains("perfil bien: ok") && t.contains("perfil mal: CON ERRORES"), "{t}");
    assert!(t.contains("el perfil 'mal' pide PIC16F886") && t.contains("PYP2194") && t.contains("WDTE") && t.contains("NOEXISTE"), "{t}");
    // un solo perfil
    let o = pyxpic(&dir, &["project", "check", "--target", "bien"]);
    assert!(o.status.success() && !text(&o).contains("perfil mal"), "{}", text(&o));
    // no se escribio nada: ni el proyecto, ni el programa, ni un .hex
    assert_eq!((std::fs::read_to_string(dir.join("pyxpic.toml")).unwrap(), std::fs::read_to_string(dir.join("main.py")).unwrap()), before);
    assert!(!dir.join("main.hex").exists() && !dir.join("build").exists());
}

#[test]
fn project_check_sees_missing_files_source_errors_and_warnings() {
    let toml = "[project]\nmain = \"nada.py\"\ndevices_dir = \"@DEVICES@\"\n[target.a]\ndevice = \"PIC16F887\"\n";
    let dir = project_dir("check-nofile", toml, "def main() -> None:\n    pass\n");
    let o = pyxpic(&dir, &["project", "check"]);
    assert!(!o.status.success() && text(&o).contains("no se pudo leer el programa") && text(&o).contains("nada.py"), "{}", text(&o));
    // un error del propio programa sale con archivo, linea y codigo
    let toml = "[project]\nmain = \"main.py\"\ndevices_dir = \"@DEVICES@\"\n[target.a]\ndevice = \"PIC16F887\"\n";
    let dir = project_dir("check-src", toml, "def main() -> None:\n    x: uint8 = 300\n");
    let o = pyxpic(&dir, &["project", "check"]);
    assert!(!o.status.success() && text(&o).contains("main.py:2:") && text(&o).contains("PYP2"), "{}", text(&o));
    // sin dispositivo en ningun lado
    let dir = project_dir("check-nodev", "[project]\nmain = \"main.py\"\ndevices_dir = \"@DEVICES@\"\n[target.a]\nclock = \"Internal 4MHz\"\n", "def main() -> None:\n    pass\n");
    let o = pyxpic(&dir, &["project", "check"]);
    assert!(!o.status.success() && text(&o).contains("falta el dispositivo"), "{}", text(&o));
}

#[test]
fn project_check_json_is_versioned_and_machine_readable() {
    let toml = "[project]\nname = \"p\"\nmain = \"main.py\"\ndevices_dir = \"@DEVICES@\"\n[target.ok]\ndevice = \"PIC16F887\"\nclock = \"Crystal 8MHz\"\nconfig = { WDTE = \"ON\" }\n[target.mal]\ndevice = \"PIC16F887\"\nclock = \"Crystal 24MHz\"\n";
    let dir = project_dir("check-json", toml, BARE_MAIN);
    let o = pyxpic(&dir, &["project", "check", "--format", "json"]);
    assert_eq!(o.status.code(), Some(1));
    let v: serde_json::Value = serde_json::from_slice(&o.stdout).unwrap_or_else(|e| panic!("{e}: {}", text(&o)));
    assert_eq!(v["version"], 1);
    assert_eq!(v["name"], "p");
    assert_eq!(v["ok"], false);
    let targets = v["targets"].as_array().unwrap();
    assert_eq!(targets.len(), 2);
    let ok = targets.iter().find(|t| t["name"] == "ok").unwrap();
    assert_eq!(
        (ok["status"].as_str(), ok["device"].as_str(), ok["clock"]["declaration"].as_str(), ok["config"]["WDTE"].as_str()),
        (Some("ok"), Some("PIC16F887"), Some("Crystal 8MHz"), Some("ON"))
    );
    assert!(ok["clock"]["description"].as_str().unwrap().contains("FOSC = HS") && ok["errors"].as_array().unwrap().is_empty());
    let mal = targets.iter().find(|t| t["name"] == "mal").unwrap();
    assert_eq!(mal["status"], "error");
    assert!(mal["errors"][0].as_str().unwrap().contains("PYP2194"), "{mal}");
    // todo ok -> salida 0 y ok = true
    let toml = "[project]\nmain = \"main.py\"\ndevices_dir = \"@DEVICES@\"\n[target.a]\ndevice = \"PIC16F887\"\n";
    let dir = project_dir("check-json-ok", toml, BARE_MAIN);
    let o = pyxpic(&dir, &["project", "check", "--format", "json"]);
    let v: serde_json::Value = serde_json::from_slice(&o.stdout).unwrap();
    assert!(o.status.success() && v["ok"] == true && v["targets"][0]["clock"].is_null(), "{}", text(&o));
}

#[test]
fn project_command_errors_are_clear() {
    let o = pyxpic(&root(), &["project", "borrar"]);
    assert_eq!(o.status.code(), Some(2));
    assert!(text(&o).contains("no existe") && text(&o).contains("check"), "{}", text(&o));
    let dir = fresh("check-none");
    std::fs::create_dir_all(&dir).unwrap();
    let o = pyxpic(&dir, &["project", "check"]);
    assert_eq!(o.status.code(), Some(2));
    assert!(text(&o).contains("no hay un pyxpic.toml") && text(&o).contains("pyxpic init"), "{}", text(&o));
    let o = pyxpic(&root(), &["project", "check", "--format", "yaml"]);
    assert_eq!(o.status.code(), Some(2));
    assert!(text(&o).contains("'text' o 'json'"), "{}", text(&o));
}

// ---- pyxpic project set ------------------------------------------------------------------------------------------------------

const COMMENTED: &str = "# Mi proyecto (este comentario debe sobrevivir)\n[project]\nname = \"demo\"            # nombre\nmain = \"main.py\"\ndevices_dir = \"@DEVICES@\"\n\n[target.placa]            # mi placa\ndevice = \"PIC16F887\"\nclock = \"Internal 4MHz\"   # reloj actual\noutput = \"build/demo.hex\"\n";

fn set_project(name: &str) -> PathBuf {
    project_dir(name, COMMENTED, BARE_MAIN)
}

fn toml_of(dir: &Path) -> String {
    std::fs::read_to_string(dir.join("pyxpic.toml")).unwrap()
}

#[test]
fn set_changes_only_the_asked_keys_keeps_comments_and_leaves_a_backup() {
    let dir = set_project("set-ok");
    let before = toml_of(&dir);
    let o = pyxpic(&dir, &["project", "set", "clock=Crystal 20MHz", "config.WDTE=ON", "config.BOREN=1"]);
    assert!(o.status.success(), "{}", text(&o));
    let t = text(&o);
    assert!(t.contains("placa.clock: Internal 4MHz -> Crystal 20MHz") && t.contains("placa.config.WDTE: (nuevo) ON") && t.contains("pyxpic.toml.bak"), "{t}");
    let after = toml_of(&dir);
    // los comentarios (los de linea y los del final de linea) y el resto del archivo no se tocan
    for keep in [
        "# Mi proyecto (este comentario debe sobrevivir)",
        "name = \"demo\"            # nombre",
        "[target.placa]            # mi placa",
        "clock = \"Crystal 20MHz\"   # reloj actual",
        "output = \"build/demo.hex\"",
    ] {
        assert!(after.contains(keep), "falta {keep:?} en:\n{after}");
    }
    assert!(after.contains("config = { WDTE = \"ON\", BOREN = 1 }"), "{after}");
    assert_eq!(std::fs::read_to_string(dir.join("pyxpic.toml.bak")).unwrap(), before, "la copia es la version anterior");
    // el archivo resultante se revisa y se compila con el reloj nuevo
    assert!(pyxpic(&dir, &["project", "check"]).status.success());
    let o = pyxpic(&dir, &["build"]);
    assert!(o.status.success() && text(&o).contains("Crystal 20 MHz (FOSC = HS)"), "{}", text(&o));
}

#[test]
fn set_unset_removes_keys_and_an_empty_config_table_and_same_value_changes_nothing() {
    let dir = set_project("set-unset");
    assert!(pyxpic(&dir, &["project", "set", "config.WDTE=ON"]).status.success());
    let o = pyxpic(&dir, &["project", "set", "--unset", "config.WDTE", "--unset", "output"]);
    assert!(o.status.success() && text(&o).contains("config.WDTE: ON -> (quitado)") && text(&o).contains("output: build/demo.hex -> (quitado)"), "{}", text(&o));
    let t = toml_of(&dir);
    assert!(!t.contains("config") && !t.contains("output"), "{t}");
    // el mismo valor: sin cambios y sin copia nueva
    let _ = std::fs::remove_file(dir.join("pyxpic.toml.bak"));
    let o = pyxpic(&dir, &["project", "set", "clock=Internal 4MHz"]);
    assert!(o.status.success() && text(&o).contains("sin cambios"), "{}", text(&o));
    assert!(!dir.join("pyxpic.toml.bak").exists());
}

#[test]
fn set_dry_run_shows_the_changes_and_writes_nothing() {
    let dir = set_project("set-dry");
    let before = toml_of(&dir);
    let o = pyxpic(&dir, &["project", "set", "--dry-run", "clock=Crystal 8MHz"]);
    assert!(o.status.success() && text(&o).contains("Crystal 8MHz") && text(&o).contains("--dry-run: no se escribio nada"), "{}", text(&o));
    assert_eq!(toml_of(&dir), before);
    assert!(!dir.join("pyxpic.toml.bak").exists());
}

#[test]
fn set_refuses_a_change_that_would_leave_the_profile_invalid_and_writes_nothing() {
    let dir = set_project("set-invalid");
    let before = toml_of(&dir);
    for (arg, frag) in
        [("clock=Internal 20MHz", "PYP2194"), ("clock=Crystal 24MHz", "PYP2194"), ("config.WDTE=QUIZAS", "WDTE"), ("config.NOEXISTE=1", "NOEXISTE"), ("device=PIC99X999", "desconocido")]
    {
        let o = pyxpic(&dir, &["project", "set", arg]);
        assert_eq!(o.status.code(), Some(1), "{arg}: {}", text(&o));
        assert!(text(&o).contains("no se escribe nada") && text(&o).contains(frag), "{arg}: {}", text(&o));
        assert_eq!(toml_of(&dir), before, "{arg}: el archivo no cambia");
        assert!(!dir.join("pyxpic.toml.bak").exists(), "{arg}: ni se crea la copia");
    }
    // el programa declara su dispositivo: el perfil no puede pedir otro
    let dir = project_dir("set-mismatch", COMMENTED, &format!("from pyxpic.device import PIC16F887\n{BARE_MAIN}"));
    let o = pyxpic(&dir, &["project", "set", "device=PIC16F886"]);
    assert_eq!(o.status.code(), Some(1), "{}", text(&o));
    assert!(text(&o).contains("el programa declara 'from pyxpic.device import PIC16F887'") && text(&o).contains("pide PIC16F886"), "{}", text(&o));
}

#[test]
fn set_picks_the_profile_creates_new_ones_and_edits_project_keys() {
    let toml = format!("{COMMENTED}\n[target.otra]\ndevice = \"PIC16F887\"\n");
    let dir = project_dir("set-targets", &toml, BARE_MAIN);
    // dos perfiles y ninguno por defecto: hay que elegir
    let o = pyxpic(&dir, &["project", "set", "clock=Crystal 8MHz"]);
    assert_eq!(o.status.code(), Some(2), "{}", text(&o));
    assert!(text(&o).contains("hay varios perfiles (otra, placa)"), "{}", text(&o));
    let o = pyxpic(&dir, &["project", "set", "--target", "otra", "clock=Crystal 8MHz"]);
    assert!(o.status.success(), "{}", text(&o));
    assert!(toml_of(&dir).contains("[target.otra]") && toml_of(&dir).contains("clock = \"Crystal 8MHz\""), "{}", toml_of(&dir));
    // un perfil que no existe: error, o se crea con --create
    let o = pyxpic(&dir, &["project", "set", "--target", "nueva", "device=PIC16F887"]);
    assert_eq!(o.status.code(), Some(2), "{}", text(&o));
    assert!(text(&o).contains("no hay un perfil 'nueva'") && text(&o).contains("--create"), "{}", text(&o));
    let o = pyxpic(&dir, &["project", "set", "--target", "nueva", "--create", "device=PIC16F887", "clock=Internal 8MHz"]);
    assert!(o.status.success() && text(&o).contains("perfil nueva: (nuevo)"), "{}", text(&o));
    let o = pyxpic(&dir, &["project", "check", "--target", "nueva"]);
    assert!(o.status.success() && text(&o).contains("Internal 8 MHz"), "{}", text(&o));
    // claves del proyecto: no necesitan perfil
    let o = pyxpic(&dir, &["project", "set", "project.default_target=placa", "project.name=otro"]);
    assert!(o.status.success(), "{}", text(&o));
    assert!(toml_of(&dir).contains("default_target = \"placa\"") && toml_of(&dir).contains("name = \"otro\"            # nombre"), "{}", toml_of(&dir));
    // un default_target que no existe deja el archivo invalido: se rechaza
    let before = toml_of(&dir);
    let o = pyxpic(&dir, &["project", "set", "project.default_target=fantasma"]);
    assert_eq!(o.status.code(), Some(1), "{}", text(&o));
    assert!(text(&o).contains("invalido") && text(&o).contains("fantasma"), "{}", text(&o));
    assert_eq!(toml_of(&dir), before);
}

#[test]
fn set_usage_errors_say_the_valid_keys() {
    let dir = set_project("set-usage");
    let o = pyxpic(&dir, &["project", "set"]);
    assert_eq!(o.status.code(), Some(2));
    assert!(text(&o).contains("no hay nada que cambiar") && text(&o).contains("config.AJUSTE"), "{}", text(&o));
    let o = pyxpic(&dir, &["project", "set", "color=rojo"]);
    assert_eq!(o.status.code(), Some(2));
    assert!(text(&o).contains("la clave 'color' no existe") && text(&o).contains("device, clock, output"), "{}", text(&o));
    let o = pyxpic(&dir, &["project", "set", "clock"]);
    assert_eq!(o.status.code(), Some(2));
    assert!(text(&o).contains("se escribe CLAVE=VALOR"), "{}", text(&o));
    let o = pyxpic(&dir, &["project", "set", "project.fantasma=1"]);
    assert_eq!(o.status.code(), Some(2));
    assert!(text(&o).contains("la clave 'project.fantasma' no existe"), "{}", text(&o));
}

#[test]
fn project_check_devices_dir_overrides_the_projects_like_build_does() {
    // el proyecto apunta a una carpeta de Device Packs que no existe; --devices-dir indica la buena
    let dir = project_dir("check-devdir", "[project]\nmain = \"main.py\"\ndevices_dir = \"carpeta-que-no-existe\"\n[target.a]\ndevice = \"PIC16F887\"\n", BARE_MAIN);
    let o = pyxpic(&dir, &["project", "check"]);
    assert_eq!(o.status.code(), Some(1), "{}", text(&o));
    let o = pyxpic(&dir, &["project", "check", "--devices-dir", &devices()]);
    assert!(o.status.success() && text(&o).contains("perfil a: ok"), "{}", text(&o));
}

#[test]
fn check_prints_the_new_warnings_and_still_succeeds() {
    let dir = tmp("warnings");
    let src = dir.join("a.py");
    std::fs::write(&src, "from pyxpic.device import PIC16F887\n\n\ndef f(a: uint8, b: uint8) -> uint8:\n    return a\n    a = 1\n\n\ndef main() -> None:\n    x: uint8 = 5\n    f(1, 2)\n").unwrap();
    let o = pyxpic(&root(), &["check", src.to_str().unwrap()]);
    let t = text(&o);
    assert!(o.status.success(), "las advertencias no hacen fallar: {t}");
    for expected in ["warning[PYP3020]", "warning[PYP3021]", "warning[PYP3022]", "a.py: ok"] {
        assert!(t.contains(expected), "falta {expected}: {t}");
    }
}

// ---- pyxpic check --format json (#10) ------------------------------------------------------------------------------------------

/// Corre `pyxpic check prog.py --format json` en una carpeta nueva (asi el campo "file" es estable) y devuelve la salida.
fn json_check(source: &str, extra: &[&str]) -> (Output, String) {
    // una carpeta propia por llamada: las pruebas corren en paralelo y no deben pisarse el prog.py
    static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let dir = tmp(&format!("json-{}", NEXT.fetch_add(1, std::sync::atomic::Ordering::SeqCst)));
    std::fs::write(dir.join("prog.py"), source).unwrap();
    let mut args = vec!["check", "prog.py", "--format", "json", "--devices-dir"];
    let dev = devices();
    args.push(&dev);
    args.extend_from_slice(extra);
    let o = pyxpic(&dir, &args);
    let out = String::from_utf8_lossy(&o.stdout).replace("\r\n", "\n");
    (o, out)
}

const WITH_ERRORS: &str = "from pyxpic.device import PIC16F887\n\n\ndef f(a: uint8) -> uint8:\n    return a\n    a = 1\n\n\ndef main() -> None:\n    x: uint8 = 300\n    y = = 5\n";
const WITH_WARNINGS: &str = "from pyxpic.device import PIC16F887\n\n\ndef f(a: uint8, b: uint8) -> uint8:\n    return a\n    a = 1\n\n\ndef main() -> None:\n    x: uint8 = 5\n    f(1, 2)\n";

/// Las instantaneas fijan el formato: cambiarlo a proposito exige actualizar el archivo y subir `version` si rompe a quien lo lee.
fn snapshot(name: &str) -> String {
    std::fs::read_to_string(root().join("crates/pyxpic-cli/tests/snapshots").join(name)).unwrap().replace("\r\n", "\n")
}

#[test]
fn check_json_matches_the_committed_snapshots() {
    let (o, out) = json_check(WITH_ERRORS, &[]);
    assert_eq!(o.status.code(), Some(1), "con errores sale con 1");
    assert_eq!(out, snapshot("check_errors.json"), "el JSON con errores cambio");
    let (o, out) = json_check(WITH_WARNINGS, &[]);
    assert_eq!(o.status.code(), Some(0), "solo avisos: sale con 0");
    assert_eq!(out, snapshot("check_warnings.json"), "el JSON con avisos cambio");
}

#[test]
fn check_json_is_one_document_with_versioned_ranges_and_no_other_output() {
    let (o, out) = json_check(WITH_ERRORS, &[]);
    assert!(o.stderr.is_empty(), "nada en stderr: {}", String::from_utf8_lossy(&o.stderr));
    let v: serde_json::Value = serde_json::from_str(&out).unwrap_or_else(|e| panic!("{e}: {out}"));
    assert_eq!(v["version"], 1);
    assert_eq!(v["file"], "prog.py");
    assert_eq!(v["ok"], false);
    assert_eq!((v["summary"]["errors"].as_u64(), v["summary"]["warnings"].as_u64()), (Some(2), Some(0)));
    let d = v["diagnostics"].as_array().unwrap();
    assert_eq!(d.len(), 2);
    // el error de tipos (linea 10) y el de sintaxis (linea 11) salen juntos y ordenados por posicion
    assert_eq!((d[0]["code"].as_str(), d[0]["severity"].as_str()), (Some("PYP2040"), Some("error")));
    assert_eq!(d[1]["code"], "PYP1100");
    for item in d {
        let (s, e) = (&item["range"]["start"], &item["range"]["end"]);
        let (sl, sc, el, ec) = (s["line"].as_u64().unwrap(), s["column"].as_u64().unwrap(), e["line"].as_u64().unwrap(), e["column"].as_u64().unwrap());
        assert!(sl >= 1 && sc >= 1, "las posiciones son de 1 en adelante");
        assert!((el, ec) >= (sl, sc), "el fin no esta antes del inicio");
        assert!(item["message"].as_str().is_some_and(|m| !m.is_empty()));
    }
    assert_eq!(d[0]["range"]["start"]["line"], 10);
    assert_eq!((d[0]["range"]["start"]["column"].as_u64(), d[0]["range"]["end"]["column"].as_u64()), (Some(16), Some(19)), "el rango cubre el 300");
}

#[test]
fn check_json_of_a_clean_program_is_ok_with_no_diagnostics() {
    let (o, out) = json_check("from pyxpic.device import PIC16F887\n\n\ndef main() -> None:\n    pass\n", &[]);
    assert!(o.status.success());
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!((v["ok"].clone(), v["summary"]["errors"].clone(), v["diagnostics"].as_array().map(Vec::len)), (serde_json::json!(true), serde_json::json!(0), Some(0)));
}

#[test]
fn the_text_output_is_unchanged_and_json_is_only_for_check() {
    let dir = tmp("json-text");
    std::fs::write(dir.join("prog.py"), WITH_WARNINGS).unwrap();
    let o = pyxpic(&dir, &["check", "prog.py", "--devices-dir", &devices()]);
    let t = text(&o);
    assert!(t.contains("prog.py:6:5: warning[PYP3020]") && t.contains("prog.py: ok"), "{t}");
    assert!(!t.contains("\"version\""));
    for args in [&["build", "prog.py", "--format", "json"][..], &["ir", "prog.py", "--format", "json"][..]] {
        let o = pyxpic(&dir, &[args, &["--devices-dir", &devices()]].concat());
        assert_eq!(o.status.code(), Some(2), "{}", text(&o));
        assert!(text(&o).contains("solo vale con check"), "{}", text(&o));
    }
    let o = pyxpic(&dir, &["check", "prog.py", "--format", "yaml"]);
    assert_eq!(o.status.code(), Some(2));
    assert!(text(&o).contains("'text' o 'json'"), "{}", text(&o));
}
