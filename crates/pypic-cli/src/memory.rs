//! `pypic memory`: cuanta memoria usa el programa, sin grabar nada (docs/memoria.md, #64).
//!
//! El informe sale de lo que el compilador ya produjo (la imagen, los simbolos de RAM y los puntos de entrada) y de las capacidades del Device Pack:
//! no estima nada que no pueda contar. Lo que no se puede analizar (el cuerpo de una funcion en ensamblador) se dice como **parcial**, no como comprobado.

use std::process::ExitCode;

use pypic_device::DevicePack;
use pypic_frontend::Program;
use pypic_pic14::{Compiled, Options};
use serde_json::{json, Value};

/// La version del formato JSON: sube cuando cambia el significado de un campo; agregar campos no la cambia (docs/memoria.md).
pub const JSON_VERSION: u32 = 1;

fn percent(used: u64, total: u64) -> f64 {
    if total == 0 {
        0.0
    } else {
        ((used as f64 / total as f64) * 10_000.0).round() / 100.0
    }
}

/// `12,5` en el texto (con coma), como el resto de la documentacion.
fn pct_text(p: f64) -> String {
    format!("{p:.1}").replace('.', ",")
}

pub struct Inputs<'a> {
    pub file: &'a str,
    pub compiled: &'a Compiled,
    pub program: &'a Program,
    pub pack: &'a DevicePack,
    pub opts: &'a Options,
    pub details: bool,
}

/// El informe como JSON versionado; el texto se arma a partir de el.
pub fn report(i: &Inputs) -> Value {
    let mem = &i.pack.memory;
    let capacity = mem.program.words as u64;
    let (cfg_start, cfg_end) = (i.pack.config.space.start.0, i.pack.config.space.end.0);
    // palabras de programa: las que estan dentro de la flash; la zona de configuracion y los ID no cuentan como programa
    let mut words: Vec<u32> = i.compiled.image.iter().map(|(a, _)| a).filter(|a| (*a as u64) < capacity).collect();
    words.sort_unstable();
    let used = words.len() as u64;
    let config_words = i.compiled.image.iter().filter(|(a, _)| *a >= cfg_start && *a <= cfg_end).count();
    let highest = words.last().map(|a| *a as u64 + 1).unwrap_or(0);

    // por funcion: las palabras ocupadas entre su punto de entrada y el de la siguiente (lo anterior a la primera es el arranque y los vectores)
    let mut funcs: Vec<(String, u32)> = i.compiled.functions.clone();
    funcs.sort_by_key(|(_, a)| *a);
    let count_in = |from: u32, to: u32| words.iter().filter(|a| **a >= from && **a < to).count() as u64;
    let first_entry = funcs.first().map(|(_, a)| *a).unwrap_or(capacity as u32);
    let startup = count_in(0, first_entry);
    let per_function: Vec<Value> = funcs
        .iter()
        .enumerate()
        .map(|(n, (name, entry))| {
            let end = funcs.get(n + 1).map(|(_, a)| *a).unwrap_or(capacity as u32);
            json!({ "name": name, "address": entry, "words": count_in(*entry, end) })
        })
        .collect();

    // RAM: el compilador solo usa el banco 0 (docs/sdk-modulos.md y #26); el resto del chip se dice tal cual
    let data = &mem.data;
    let usable = (i.opts.ram_end - i.opts.ram_start + 1) as u64;
    let ram_used = i.compiled.ram_bytes as u64;
    let common_bytes = (data.common.end.0 - data.common.start.0 + 1) as u64;
    let banks: Vec<Value> = (0..data.banks)
        .map(|b| {
            let capacity: u32 = data.gpr.iter().filter(|r| r.start.0 / data.bank_size == b).map(|r| r.len()).sum();
            let (compiler_capacity, compiler_used) = if b == 0 { (usable, ram_used) } else { (0, 0) };
            json!({ "bank": b, "gpr_bytes": capacity, "compiler_capacity": compiler_capacity, "used": compiler_used })
        })
        .collect();
    let global_bytes: u64 =
        i.compiled.globals.iter().filter(|g| !g.is_const).map(|g| (g.ty.bits() as u64).div_ceil(8).max(1)).sum::<u64>() + i.compiled.arrays.iter().map(|a| a.len as u64).sum::<u64>();
    let mut variables: Vec<Value> = i.compiled.globals.iter().filter(|g| !g.is_const).map(|g| json!({ "name": g.name, "address": g.addr, "bits": g.ty.bits() })).collect();
    variables.extend(i.compiled.arrays.iter().map(|a| json!({ "name": a.name, "address": a.addr, "bytes": a.len })));

    // pila de hardware
    let stack = match (i.program.stack_use, mem.program.stack_levels) {
        (Some((main, isr)), levels) => json!({ "levels": levels, "worst_case": main + isr, "main": main, "interrupt": isr }),
        (None, levels) => json!({ "levels": levels, "worst_case": null }),
    };

    // periféricos y pines que el programa usa
    let mut peripherals: Vec<&str> = Vec::new();
    let p = i.program;
    for (used, name) in [
        (!p.adc_calls.is_empty(), "ADC"),
        (!p.uart_calls.is_empty(), "UART"),
        (!p.eeprom_calls.is_empty(), "EEPROM"),
        (!p.i2c_calls.is_empty(), "I2C"),
        (!p.pwm_calls.is_empty(), "PWM"),
        (!p.comp_calls.is_empty(), "comparadores/CVREF"),
        (!p.timer_calls.is_empty(), "temporizadores"),
    ] {
        if used {
            peripherals.push(name);
        }
    }
    let pins: Vec<&str> = p.pins.iter().map(|d| d.pin.as_str()).collect();

    // lo que no se analiza: las funciones en ensamblador (su uso de pila, de bancos y de RAM por nombre no se conoce)
    let asm_functions: Vec<&str> = p
        .module
        .items
        .iter()
        .filter_map(|it| match it {
            pypic_frontend::ast::Item::Func(f) if f.asm.is_some() => Some(f.name.name.as_str()),
            _ => None,
        })
        .collect();
    let mut notes: Vec<String> = Vec::new();
    if !asm_functions.is_empty() {
        notes.push(format!(
            "{} funcion(es) en ensamblador ({}): sus palabras de programa se cuentan, pero su uso de la pila de hardware y de la RAM no se analiza",
            asm_functions.len(),
            asm_functions.join(", ")
        ));
    }
    if i.program.stack_use.is_none() {
        notes.push("el destino no declara los niveles de la pila de hardware: no se calcula el peor caso".into());
    }
    let complete = notes.is_empty();

    let mut v = json!({
        "version": JSON_VERSION,
        "tool": "pypic memory",
        "file": i.file,
        "device": i.pack.device.name,
        "complete": complete,
        "notes": notes,
        "program": {
            "unit": "words",
            "word_bits": mem.program.word_bits,
            "capacity": capacity,
            "used": used,
            "free": capacity - used,
            "used_percent": percent(used, capacity),
            "highest_address_used": highest,
            "startup_and_vectors": startup,
            "config_words": config_words,
        },
        "ram": {
            "unit": "bytes",
            "chip_total": data.ram_bytes,
            "compiler_usable": usable,
            "used": ram_used,
            "globals": global_bytes.min(ram_used),
            "locals_and_workspace": ram_used.saturating_sub(global_bytes),
            "free": usable - ram_used.min(usable),
            "used_percent": percent(ram_used, usable),
            "reserved_common": common_bytes,
            "banks": banks,
        },
        "stack": stack,
        "peripherals": peripherals,
        "pins": pins,
    });
    if i.details {
        v["details"] = json!({ "functions": per_function, "variables": variables });
    }
    v
}

pub fn text(v: &Value, details: bool) -> String {
    let n = |p: &str| v.pointer(p).and_then(Value::as_u64).unwrap_or(0);
    let f = |p: &str| v.pointer(p).and_then(Value::as_f64).unwrap_or(0.0);
    let mut s = String::new();
    s += &format!("Memoria de {} ({})\n", v["device"].as_str().unwrap_or("?"), v["file"].as_str().unwrap_or("?"));
    s += &format!(
        "  Programa (flash):  {} de {} palabras de {} bits ({} %), {} libres; dirección más alta usada: {:#06X}\n",
        n("/program/used"),
        n("/program/capacity"),
        n("/program/word_bits"),
        pct_text(f("/program/used_percent")),
        n("/program/free"),
        n("/program/highest_address_used").saturating_sub(1)
    );
    s += &format!("                     arranque y vectores: {} palabras; configuración: {} palabras (no cuentan como programa)\n", n("/program/startup_and_vectors"), n("/program/config_words"));
    s += &format!(
        "  RAM de datos:      {} de {} bytes ({} %) en el banco 0, el único que usa el compilador por ahora; el chip tiene {} bytes y la zona común reserva {}\n",
        n("/ram/used"),
        n("/ram/compiler_usable"),
        pct_text(f("/ram/used_percent")),
        n("/ram/chip_total"),
        n("/ram/reserved_common")
    );
    match v.pointer("/stack/worst_case").and_then(Value::as_u64) {
        Some(w) => s += &format!("  Pila de hardware:  {} de {} niveles en el peor caso (principal {}, interrupción {})\n", w, n("/stack/levels"), n("/stack/main"), n("/stack/interrupt")),
        None => s += &format!("  Pila de hardware:  {} niveles; el peor caso no se calcula\n", n("/stack/levels")),
    }
    let list = |p: &str| v.pointer(p).and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_str).collect::<Vec<_>>().join(", ")).unwrap_or_default();
    let (periph, pins) = (list("/peripherals"), list("/pins"));
    s += &format!("  Periféricos:       {}\n", if periph.is_empty() { "ninguno" } else { &periph });
    s += &format!("  Pines usados:      {}\n", if pins.is_empty() { "ninguno" } else { &pins });
    if details {
        s += "  Funciones (palabras hasta la siguiente; las tablas y rutinas de apoyo que van al final se suman a la última):\n";
        for fu in v.pointer("/details/functions").and_then(Value::as_array).into_iter().flatten() {
            s += &format!("    {:<28} {:#06X}  {:>5} palabras\n", fu["name"].as_str().unwrap_or("?"), fu["address"].as_u64().unwrap_or(0), fu["words"].as_u64().unwrap_or(0));
        }
        s += &format!(
            "  RAM: {} bytes de variables globales y arreglos, {} de locales, temporales y espacio de trabajo del compilador (esperas, rutinas)\n  Variables:\n",
            n("/ram/globals"),
            n("/ram/locals_and_workspace")
        );
        for va in v.pointer("/details/variables").and_then(Value::as_array).into_iter().flatten() {
            let size = match (va["bits"].as_u64(), va["bytes"].as_u64()) {
                (Some(b), _) => format!("{} bits", b),
                (_, Some(b)) => format!("{b} bytes (arreglo)"),
                _ => String::new(),
            };
            s += &format!("    {:<28} {:#06X}  {}\n", va["name"].as_str().unwrap_or("?"), va["address"].as_u64().unwrap_or(0), size);
        }
    }
    if v["complete"].as_bool() == Some(false) {
        s += "  RESULTADO PARCIAL:\n";
        for note in v["notes"].as_array().into_iter().flatten().filter_map(Value::as_str) {
            s += &format!("    - {note}\n");
        }
    }
    s
}

pub fn run(i: &Inputs, json: bool) -> ExitCode {
    let v = report(i);
    if json {
        println!("{}", serde_json::to_string_pretty(&v).expect("el informe es JSON valido"));
    } else {
        print!("{}", text(&v, i.details));
    }
    ExitCode::SUCCESS
}
