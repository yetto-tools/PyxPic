//! `pypic check --format json` (docs/diagnosticos-json.md): los diagnosticos en un formato estable para el LSP y la extension de VS Code.
//!
//! El esquema esta **versionado** (`"version": 1`): cambiar el significado o quitar un campo sube la version; agregar un campo no. Las
//! posiciones son de **1 en adelante** y la columna cuenta caracteres, igual que la salida de texto.

use pypic_frontend::diag::line_col;
use pypic_frontend::{Diagnostics, Severity};
use serde_json::{json, Value};

pub const SCHEMA_VERSION: u32 = 1;

fn position(src: &str, offset: u32) -> Value {
    let (line, column) = line_col(src, offset as usize);
    json!({ "line": line, "column": column })
}

/// El informe de un archivo: `ok` es `true` si no hay errores (los avisos no impiden compilar).
pub fn diagnostics_json(file: &str, src: &str, diags: &Diagnostics) -> Value {
    let mut items: Vec<_> = diags.items.iter().collect();
    items.sort_by_key(|d| (d.span.start, d.span.end));
    let errors = items.iter().filter(|d| d.severity == Severity::Error).count();
    json!({
        "version": SCHEMA_VERSION,
        "file": file,
        "ok": errors == 0,
        "summary": { "errors": errors, "warnings": items.len() - errors },
        "diagnostics": items.iter().map(|d| json!({
            "code": d.code,
            "severity": match d.severity { Severity::Error => "error", Severity::Warning => "warning" },
            "message": d.message,
            "range": { "start": position(src, d.span.start), "end": position(src, d.span.end) },
        })).collect::<Vec<_>>(),
    })
}
