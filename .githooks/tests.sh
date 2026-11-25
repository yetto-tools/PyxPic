#!/bin/sh
# Pruebas de la guardia de identidad (.githooks/pre-commit y pre-push) en un repositorio temporal. Sale con 0 si todo pasa.
# Uso: sh .githooks/tests.sh

here=$(cd "$(dirname "$0")" && pwd)
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
pass=0
fail=0
ok() { pass=$((pass + 1)); }
no() { fail=$((fail + 1)); echo "FALLA: $1"; }
expect_ok() { desc=$1; shift; if "$@" >/dev/null 2>&1; then ok; else no "$desc (deberia pasar)"; fi; }
expect_blocked() { desc=$1; shift; if "$@" >/dev/null 2>&1; then no "$desc (deberia bloquearse)"; else ok; fi; }

cd "$tmp" && git init -q repo && cd repo || exit 2
git config --local core.hooksPath "$here"
git config --local user.name "Ana Prueba"
git config --local user.email "ana@ejemplo.com"
git config --local identity.expectedName "Ana Prueba"
git config --local identity.expectedEmail "ana@ejemplo.com"
git config --local identity.expectedAccount "cuenta-ana"
git config --local identity.expectedOwner "org-ana"
git config --local credential.https://github.com.username "cuenta-ana"

# ---- pre-commit ---------------------------------------------------------------------------------------------------------------------
echo uno > f && git add f
expect_ok "un commit con la identidad esperada" git commit -q -m uno
good=$(git rev-parse HEAD)
count=$(git rev-list --count HEAD)

expect_blocked "un correo de autor distinto" env GIT_AUTHOR_EMAIL=otro@ejemplo.com git commit -q --allow-empty -m dos
expect_blocked "un nombre de autor distinto" git -c user.name="Otra Persona" commit -q --allow-empty -m dos
expect_blocked "un correo distinto de quien confirma" env GIT_COMMITTER_EMAIL=otro@ejemplo.com git commit -q --allow-empty -m dos
expect_blocked "el nombre y el correo cruzados a la vez" env GIT_AUTHOR_NAME=Cruzado GIT_AUTHOR_EMAIL=cruzado@ejemplo.com git commit -q --allow-empty -m dos
expect_blocked "sin identidad esperada configurada" git -c identity.expectedEmail= commit -q --allow-empty -m dos
[ "$(git rev-list --count HEAD)" = "$count" ] && ok || no "los commits bloqueados no deben crear nada"

# la guardia se puede saltar a proposito (--no-verify); asi se fabrica un commit cruzado para probar el pre-push
env GIT_AUTHOR_NAME=Cruzado GIT_AUTHOR_EMAIL=cruzado@ejemplo.com git commit -q --no-verify --allow-empty -m cruzado
crossed=$(git rev-parse HEAD)

# ---- pre-push (se invoca como lo hace git: remoto y url como argumentos y las referencias por la entrada) -------------------------------------
zero=0000000000000000000000000000000000000000
push() { printf 'refs/heads/main %s refs/heads/main %s\n' "$1" "$2" | "$here/pre-push" origin "$3"; }
mine="https://github.com/org-ana/repo.git"

expect_ok "subir solo commits con la identidad esperada" push "$good" "$zero" "$mine"
expect_ok "una url https con el usuario incluido" push "$good" "$zero" "https://cuenta-ana@github.com/org-ana/repo.git"
expect_ok "una url ssh del propietario esperado" push "$good" "$zero" "git@github.com:org-ana/repo.git"
expect_blocked "un remoto de otro propietario (https)" push "$good" "$zero" "https://github.com/otra-org/repo.git"
expect_blocked "un remoto de otro propietario (ssh)" push "$good" "$zero" "git@github.com:otra-org/repo.git"
expect_blocked "un commit cruzado entre los que se suben (rama nueva)" push "$crossed" "$zero" "$mine"
expect_blocked "un commit cruzado entre los que se suben (rama existente)" push "$crossed" "$good" "$mine"
if push "$crossed" "$zero" "$mine" 2>&1 | grep -q "Cruzado <cruzado@ejemplo.com>"; then ok; else no "el mensaje debe decir cual commit esta cruzado"; fi
expect_ok "un commit que ya estaba en el remoto no se vuelve a juzgar" push "$crossed" "$crossed" "$mine"
expect_ok "borrar una rama no juzga commits" push "$zero" "$good" "$mine"

git config --local --add identity.allowedEmail cruzado@ejemplo.com
expect_ok "un colaborador permitido con identity.allowedEmail" push "$crossed" "$good" "$mine"
git config --local --unset-all identity.allowedEmail

git config --local credential.https://github.com.username otra-cuenta
expect_blocked "credenciales de otra cuenta (https)" push "$good" "$zero" "$mine"
expect_ok "el ssh no usa esa cuenta de credenciales" push "$good" "$zero" "git@github.com:org-ana/repo.git"
git config --local --unset credential.https://github.com.username
expect_blocked "sin cuenta de credenciales fijada (https)" push "$good" "$zero" "$mine"

echo "guardia de identidad: $pass pruebas bien, $fail fallos"
[ "$fail" = 0 ]
