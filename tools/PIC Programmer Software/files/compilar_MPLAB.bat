@echo off
REM ================================================================
REM Compilador PIC 12F675 con XC8 - Windows Batch
REM Ruta: E:\Microchip\xc8\v4.00\bin
REM ================================================================

setlocal enabledelayedexpansion

REM Colores
color 0A
cls

echo ================================================
echo   COMPILADOR PIC 12F675 - XC8
echo   ASM a HEX - Windows
echo ================================================
echo.

REM Configuracion
set ASM_FILE=%1
if "%ASM_FILE%"=="" (
    set ASM_FILE=blink_led_12f675.asm
)

set OUTPUT_DIR=build
set DEVICE=12f675

REM Ruta de XC8 (actualizar si es diferente)
set XC8_PATH=E:\Microchip\xc8\v4.00\bin\xc8.exe

REM Verificar si la ruta es correcta, sino buscar automaticamente
if not exist "%XC8_PATH%" (
    echo [INFO] Buscando XC8 en E:\Microchip\MPLAB...
    
    REM Buscar versiones disponibles
    for /d %%D in (E:\Microchip\xc8\v4.00\bin\v*) do (
        if exist "%%D\bin\xc8.exe" (
            set XC8_PATH=%%D\bin\xc8.exe
            echo [OK] Encontrado: !XC8_PATH!
            goto found_xc8
        )
    )
    
    echo [ERROR] XC8 no encontrado en E:\Microchip\xc8\v4.00\bin
    echo.
    echo Verificar que XC8 esté instalado en:
    echo E:\Microchip\xc8\v[version]\bin\xc8.exe
    echo.
    pause
    exit /b 1
)

:found_xc8

REM Verificar que el archivo existe
if not exist "%ASM_FILE%" (
    echo [ERROR] No encontrado: %ASM_FILE%
    echo.
    echo Uso: %0 [archivo.asm]
    echo Ejemplo: %0 blink_led_12f675.asm
    pause
    exit /b 1
)

echo [INFO] Archivo fuente: %ASM_FILE%
echo [INFO] Compilador: %XC8_PATH%
echo.

REM Crear directorio de salida
if not exist "%OUTPUT_DIR%" (
    mkdir "%OUTPUT_DIR%"
    echo [INFO] Directorio creado: %OUTPUT_DIR%
)

REM Obtener nombre base del archivo
for %%F in ("%ASM_FILE%") do (
    set BASE_NAME=%%~nF
    set BASE_NAME=!BASE_NAME:.asm=!
)

set HEX_FILE=%OUTPUT_DIR%\%BASE_NAME%.hex
set LST_FILE=%OUTPUT_DIR%\%BASE_NAME%.lst
set OBJ_FILE=%OUTPUT_DIR%\%BASE_NAME%.o

echo [INFO] Compilando...
echo.

REM Compilar con XC8
REM XC8 puede compilar ASM directamente
"%XC8_PATH%" --chip=%DEVICE% --format=hex --output=%HEX_FILE% "%ASM_FILE%"

if errorlevel 1 (
    echo.
    echo [ERROR] Error durante la compilacion
    echo.
    pause
    exit /b 1
)

echo.
echo [OK] Compilacion exitosa
echo.

REM Verificar que se genero el HEX
if not exist "%HEX_FILE%" (
    echo [ERROR] El archivo HEX no se genero
    pause
    exit /b 1
)

echo ================================================
echo [OK] COMPILACION COMPLETADA
echo ================================================
echo.

REM Mostrar informacion del archivo
for %%F in ("%HEX_FILE%") do (
    set HEX_SIZE=%%~zF
)

echo [INFO] Archivo generado: %HEX_FILE%
echo [INFO] Tamaño: %HEX_SIZE% bytes
echo.

REM Mostrar contenido del HEX
echo [INFO] Contenido del HEX:
echo.
type "%HEX_FILE%"
echo.

echo ================================================
echo Proximos pasos:
echo ================================================
echo.
echo 1. Abrir el archivo HEX en tu programador:
echo    - PICkit 2 IDE
echo    - MinPro (TL866)
echo    - MPLAB IPE
echo.
echo 2. Archivo a usar: %HEX_FILE%
echo.
echo 3. Configurar fuses si es necesario
echo.

pause
