# Identidades: que no se crucen cuentas, nombres ni correos

Con varias cuentas de GitHub y varios correos en la misma maquina, un commit o un push pueden salir con la identidad equivocada. En este
repositorio ya paso: el historial tiene commits con cuatro autores distintos (dos correos propios, la cuenta `yetto-tools` y otro usuario).
Esta guardia lo detecta **antes** de que salga, y avisa que corregir.

## Que protege

| Gancho | Cuando | Bloquea si |
|---|---|---|
| `pre-commit` | al hacer `git commit` | el nombre o el correo del **autor** o de quien **confirma** no son los esperados |
| `pre-push` | al hacer `git push` | el remoto es de otro propietario, la cuenta de credenciales de la carpeta no es la esperada, o algun commit **nuevo** a subir tiene otro autor |

El `pre-push` solo juzga los commits que se van a subir, no lo que ya estaba en el remoto. Un colaborador legitimo se permite con
`identity.allowedEmail`.

## Activarla (una vez por clon)

Los ganchos viven en `.githooks/`; Git no los activa solo. Lo esperado se guarda en la configuracion **local** del repositorio
(`.git/config`), no en un archivo versionado, para que cada persona ponga la suya:

```
git config --local core.hooksPath .githooks
git config --local identity.expectedName    "Nombre Apellido"
git config --local identity.expectedEmail   "correo@ejemplo.com"
git config --local identity.expectedAccount "cuenta-de-github"        # usuario de credenciales de esta carpeta
git config --local identity.expectedOwner   "propietario-del-remoto"   # usuario u organizacion duena del repositorio
```

Sin `identity.expectedName` / `identity.expectedEmail` la guardia **bloquea** (no adivina). Para quitarla: `git config --local --unset core.hooksPath`.

## La identidad por carpeta (lo que la guardia comprueba)

La guardia no decide la identidad: comprueba la que Git ya iba a usar. Esa sale de `~/.gitconfig` con reglas `includeIf` por carpeta:

```
[includeIf "gitdir:C:/Users/erashon/umg/PyxPic/"]
    path = C:/Users/erashon/.gitconfig-personal
```

y en el archivo incluido:

```
[user]
    name = Nombre Apellido
    email = correo@ejemplo.com
[credential "https://github.com"]
    username = cuenta-de-github
```

`credential.https://github.com.username` hace que git pida las credenciales de **esa** cuenta. El ayudante de `gh` solo entrega el token de la
cuenta **activa** de `gh`; si es otra, el push falla en vez de usar la cuenta equivocada.

## `gh` (issues, PR, API) no sigue la carpeta

`gh` usa la cuenta activa global (`gh auth status`). Para fijar la cuenta por comando, sin `gh auth switch`:

```
GH_TOKEN="$(gh auth token --user cuenta-de-github)" gh issue list
```

El token vive solo en ese proceso. No se escribe en ningun archivo.

## Pruebas

```
sh .githooks/tests.sh
```

Crea un repositorio temporal y comprueba 21 casos: autor, nombre y correo cruzados, quien confirma, remoto de otro propietario (https y ssh),
cuenta de credenciales equivocada o ausente, commits nuevos cruzados, colaborador permitido, rama nueva y existente, y borrado de ramas.

## Limites

- `git commit --no-verify` y `git push --no-verify` se saltan la guardia; es a proposito.
- Una guardia local no impide que otro clon sin activar suba commits cruzados. Para eso haria falta una regla en GitHub o en el CI.
- No cambia commits ya publicados; reescribir historia es otra decision.
