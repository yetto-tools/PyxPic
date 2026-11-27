#!/usr/bin/env pwsh
<#
.SYNOPSIS
    Compilador PIC 12F675 con XC8 - MPLAB Edition
    
.DESCRIPTION
    Compila código ASM a HEX usando MPLAB XC8 Compiler
    Ruta por defecto: E:\Microchip\xc8\v4.00\bin
    
.PARAMETER AsmFile
    Archivo ASM a compilar (default: blink_led_12f675.asm)
    
.PARAMETER OutputDir
    Directorio de salida (default: build)
    
.EXAMPLE
    .\compilar_MPLAB.ps1
    .\compilar_MPLAB.ps1 -AsmFile blink_led_12f675.asm
    .\compilar_MPLAB.ps1 -AsmFile blink_led.asm -OutputDir .\output
    
.NOTES
    Requiere: MPLAB XC8 Compiler instalado en E:\Microchip\MPLAB
#>

param(
    [Parameter(Mandatory=$false)]
    [string]$AsmFile = "blink_led_12f675.asm",
    
    [Parameter(Mandatory=$false)]
    [string]$OutputDir = "build",
    
    [Parameter(Mandatory=$false)]
    [string]$MplabPath = "E:\Microchip\MPLAB"
)

# Configuración
$DEVICE = "12f675"
$ErrorActionPreference = "Stop"

# Colores (ignorar en Windows antiguo)
$colors = @{
    "Green"   = "`e[32m"
    "Red"     = "`e[31m"
    "Yellow"  = "`e[33m"
    "Blue"    = "`e[34m"
    "Cyan"    = "`e[36m"
    "Reset"   = "`e[0m"
}

function Write-Header {
    Write-Host "$($colors['Blue'])================================================$($colors['Reset'])"
    Write-Host "$($colors['Blue'])  COMPILADOR PIC 12F675 - XC8$($colors['Reset'])"
    Write-Host "$($colors['Blue'])  MPLAB Edition (E:\Microchip\MPLAB)$($colors['Reset'])"
    Write-Host "$($colors['Blue'])================================================$($colors['Reset'])"
    Write-Host ""
}

function Write-Success {
    param([string]$Message)
    Write-Host "$($colors['Green'])✓ $Message$($colors['Reset'])"
}

function Write-Error-Custom {
    param([string]$Message)
    Write-Host "$($colors['Red'])❌ $Message$($colors['Reset'])"
}

function Write-Warning-Custom {
    param([string]$Message)
    Write-Host "$($colors['Yellow'])⚠ $Message$($colors['Reset'])"
}

function Write-Info {
    param([string]$Message)
    Write-Host "$($colors['Cyan'])  $Message$($colors['Reset'])"
}

function Find-XC8 {
    param([string]$BasePath)
    
    Write-Host "Buscando XC8 en: $BasePath"
    Write-Host ""
    
    if (-not (Test-Path $BasePath)) {
        Write-Error-Custom "Ruta base no encontrada: $BasePath"
        return $null
    }
    
    # Buscar directorios de versiones (v2.46, v2.45, etc.)
    $xc8_dir = Join-Path $BasePath "xc8"
    
    if (-not (Test-Path $xc8_dir)) {
        Write-Error-Custom "No encontrado: $xc8_dir"
        return $null
    }
    
    # Buscar última versión
    $versions = @(Get-ChildItem -Path $xc8_dir -Directory -Filter "v*" | Sort-Object Name -Descending)
    
    if ($versions.Count -eq 0) {
        Write-Error-Custom "No hay versiones de XC8 instaladas en: $xc8_dir"
        return $null
    }
    
    foreach ($version in $versions) {
        $xc8_exe = Join-Path $version.FullName "bin\xc8.exe"
        
        if (Test-Path $xc8_exe) {
            Write-Success "XC8 encontrado: $xc8_exe"
            
            try {
                $version_info = & $xc8_exe --version 2>&1
                Write-Info "Versión: $version_info"
            }
            catch {
                Write-Warning-Custom "No se pudo obtener versión de XC8"
            }
            
            Write-Host ""
            return $xc8_exe
        }
    }
    
    Write-Error-Custom "XC8.exe no encontrado en ninguna versión de $xc8_dir"
    return $null
}

function Compile-ASM {
    param(
        [string]$XC8Path,
        [string]$AsmPath,
        [string]$HexPath,
        [string]$Device
    )
    
    Write-Host "Compilando..."
    Write-Host ""
    
    try {
        $output = & $XC8Path --chip=$Device --format=hex --output=$HexPath $AsmPath 2>&1
        
        if ($LASTEXITCODE -eq 0) {
            Write-Success "Compilación exitosa"
            if ($output) {
                Write-Info "Salida: $output"
            }
            return $true
        }
        else {
            Write-Error-Custom "Error de compilación (código: $LASTEXITCODE)"
            if ($output) {
                Write-Host $output
            }
            return $false
        }
    }
    catch {
        Write-Error-Custom "Error ejecutando XC8: $_"
        return $false
    }
}

# ===== INICIO PRINCIPAL =====
try {
    Write-Header
    
    # Verificar archivo ASM
    if (-not (Test-Path $AsmFile)) {
        Write-Error-Custom "Archivo no encontrado: $AsmFile"
        Write-Host ""
        Write-Host "Uso: .\compilar_MPLAB.ps1 [archivo.asm]"
        Write-Host "Ejemplo: .\compilar_MPLAB.ps1 blink_led_12f675.asm"
        Write-Host ""
        exit 1
    }
    
    $asm_path = (Resolve-Path $AsmFile).Path
    Write-Host "📂 Archivo fuente: $asm_path"
    Write-Host ""
    
    # Crear directorio de salida
    if (-not (Test-Path $OutputDir)) {
        New-Item -ItemType Directory -Path $OutputDir -Force | Out-Null
    }
    
    # Obtener nombre base
    $base_name = [System.IO.Path]::GetFileNameWithoutExtension($AsmFile)
    $hex_file = Join-Path $OutputDir "$base_name.hex"
    $hex_full_path = (Resolve-Path -LiteralPath $OutputDir).Path
    $hex_file_full = Join-Path $hex_full_path "$base_name.hex"
    
    # Buscar XC8
    Write-Host "$($colors['Yellow'])Localizando compilador...$($colors['Reset'])"
    Write-Host ""
    
    $xc8_exe = Find-XC8 -BasePath $MplabPath
    
    if ($null -eq $xc8_exe) {
        Write-Error-Custom "No se pudo encontrar XC8"
        Write-Host ""
        Write-Host "Soluciones:"
        Write-Host "  1. Verificar que MPLAB XC8 está instalado en:"
        Write-Host "     $MplabPath\xc8\v[version]\bin\xc8.exe"
        Write-Host ""
        Write-Host "  2. Descargar desde:"
        Write-Host "     https://www.microchip.com/en-us/tools-resources/develop/mplab-xc-compilers"
        Write-Host ""
        Write-Host "  3. Reinstalar seleccionando 'PIC 12F Family'"
        Write-Host ""
        exit 1
    }
    
    # Compilar
    $success = Compile-ASM -XC8Path $xc8_exe -AsmPath $asm_path -HexPath $hex_file_full -Device $DEVICE
    
    if (-not $success) {
        Write-Host ""
        exit 1
    }
    
    Write-Host ""
    
    # Verificar resultado
    if (-not (Test-Path $hex_file_full)) {
        Write-Error-Custom "El archivo HEX no se generó"
        exit 1
    }
    
    # Mostrar información
    Write-Host "$($colors['Blue'])================================================$($colors['Reset'])"
    Write-Success "COMPILACIÓN COMPLETADA"
    Write-Host "$($colors['Blue'])================================================$($colors['Reset'])"
    Write-Host ""
    
    $hex_size = (Get-Item $hex_file_full).Length
    $hex_lines = @(Get-Content $hex_file_full).Count
    
    Write-Host "$($colors['Green'])📄 Archivo generado:$($colors['Reset'])"
    Write-Info "$hex_file"
    Write-Host ""
    
    Write-Host "$($colors['Yellow'])Información del HEX:$($colors['Reset'])"
    Write-Info "Líneas:  $hex_lines"
    Write-Info "Tamaño:  $hex_size bytes"
    Write-Host ""
    
    # Mostrar contenido
    Write-Host "$($colors['Yellow'])Contenido del HEX:$($colors['Reset'])"
    Write-Host ""
    Get-Content $hex_file_full | ForEach-Object { Write-Info $_ }
    Write-Host ""
    
    # Próximos pasos
    Write-Host "$($colors['Blue'])Próximos pasos:$($colors['Reset'])"
    Write-Host "  1. Abrir el programador:"
    Write-Host "     • PICkit 2/3 IDE"
    Write-Host "     • MinPro (TL866)"
    Write-Host "     • MPLAB IPE"
    Write-Host ""
    Write-Host "  2. Cargar archivo: $($colors['Green'])$hex_file$($colors['Reset'])"
    Write-Host ""
    Write-Host "  3. Programar el PIC12F675"
    Write-Host ""
    Write-Host "  4. Conectar alimentación 5V"
    Write-Host "     → LED parpadea 1 Hz (500ms ON, 500ms OFF)"
    Write-Host ""
    Write-Host "$($colors['Green'])¡Listo! 🎉$($colors['Reset'])"
    Write-Host ""
}
catch {
    Write-Error-Custom "Error inesperado: $_"
    exit 1
}
