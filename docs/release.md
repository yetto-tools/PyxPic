# Ramas, CI y publicaciones

## Ramas

| Rama | Para qué | Quién escribe en ella |
|---|---|---|
| `main` | Desarrollo: cada pieza terminada entra aquí (commit y push, ver CONTRIBUTING). Siempre debe compilar y pasar el CI | el trabajo diario |
| `release` | **Lo publicable.** Solo recibe mezclas de `main` cuando el CI de `main` está en verde y la versión ya se subió | mezclas de `main`, nunca commits sueltos |
| `vX.Y.Z` (etiquetas) | Cada publicación: una etiqueta sobre un commit de `release` | la crea quien publica |
| ramas de trabajo (`hardware/...`, `vscode/...`, `ci-...`) | Una línea de trabajo que se mezcla en `main` y se borra | quien la abre |

## Qué hace el CI (`.github/workflows/ci.yml`)

En cada `push` a `main` o `release` y en cada pull request: `cargo fmt --check` (con `rustfmt.toml`), `cargo clippy -D warnings`, `cargo test --workspace` y la validación de cada Device Pack, en **Linux y Windows**; y las pruebas de la extensión de VS Code (que ejecutan la CLI real). La versión de Rust está fijada en el propio archivo (`RUST_VERSION`) para que un aviso nuevo de una versión posterior no ponga el CI en rojo sin que cambie nada.

## Publicar una versión

1. En `main`, sube la versión del espacio de trabajo (`[workspace.package] version` en `Cargo.toml`) y, si cambió la extensión, la de `vscode-pypic/package.json` (con su descripción y README: una prueba lo exige). Espera el CI en verde.
2. Mezcla `main` en `release`:
   ```
   git switch release
   git merge --no-ff main -m "Release 0.1.0"
   git push origin release
   ```
3. Etiqueta y sube la etiqueta:
   ```
   git tag -a v0.1.0 -m "PyPIC 0.1.0"
   git push origin v0.1.0
   ```
4. El workflow `release.yml` (`.github/workflows/release.yml`) hace el resto: comprueba que la etiqueta está en `release` y que su versión coincide con `Cargo.toml`, vuelve a pasar las pruebas, compila la CLI en Linux y Windows, empaqueta la extensión (`.vsix`) y crea la Release de GitHub con los archivos y `SHA256SUMS.txt`. Una etiqueta con sufijo (`v0.1.0-rc1`) sale como *pre-release*.

Cada archivo `pypic-vX.Y.Z-<sistema>` lleva la CLI, los Device Packs (`devices/`), los ejemplos, `LICENSE` y `README.md`. Si algo falla antes de publicar, no se crea la Release: se corrige en `main`, se vuelve a mezclar en `release` y se mueve o se rehace la etiqueta.

## Estado

- **CI:** corrió en GitHub por primera vez en el pull request #76 y pasó en Linux y en Windows, y las pruebas de la extensión. Desde entonces corre en cada push a `main` y `release` y en cada pull request.
- **`release` está protegida:** exige que pasen los tres trabajos del CI (`Rust (ubuntu-latest)`, `Rust (windows-latest)`, `Extension de VS Code`) y no admite `push --force` ni borrarla. Los administradores pueden saltarse la exigencia de los trabajos (es una sola persona); `main` no tiene protección.
- **`release.yml` todavía no se ha ejecutado:** se probará con la primera etiqueta. Conviene la primera vez una etiqueta con sufijo (`v0.1.0-rc1`): sale como pre-release y se puede borrar sin consecuencias si algo falla.
- Las versiones siguen provisionales (`0.0.1` en el espacio de trabajo): decide cuándo es `0.1.0`.
