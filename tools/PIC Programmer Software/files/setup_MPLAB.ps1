#!/usr/bin/env pwsh
<#
.SYNOPSIS
    Verificador y configurador automático de MPLAB XC8

.DESCRIPTION
    Verifica instalación de XC8 en E:\Microchip\xc8\v4.00\bin
    y configura PATH automáticamente si es necesario
    
.EXAMPLE
    .\setup_MPLAB.ps1
    
.NOTES
    Ejecutar como ADMINISTRADOR para cambios globales
#>

param(
    [Parameter(Mandatory=$false)]
    [string]$MplabPath = "E:\Microchip\xc8\v4.00\bin"
)

# Colores
$colors = @{
    "Green"   = "`e[32m"
    "Red"     = "`e[31m"
    "Yellow"  = "`e[33m"
    "Blue"    = "`e[34m"
    "Cyan"    = "`e[36m"
    "Reset"   = "`e[0m"
    "Bold"    = "`e[1m"
}

function Write-Header {
    Write-Host "$($colors['Blue'])$($colors['Bold'])================================================$($colors['Reset'])"
    Write-Host "$($colors['Blue'])$($colors['Bold'])  VERIFICADOR MPLAB XC8$($colors['Reset'])"
    Write-Host "$($colors['Blue'])$($colors['Bold'])  Setup Automático$($colors['Reset'])"
    Write-Host "$($colors['Blue'])$($colors['Bold'])================================================$($colors['Reset'])"
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

function Check-MplabInstallation {
    param([string]$BasePath)
    
    Write-Host "$($colors['Yellow'])Verificando instalación de MPLAB...$($colors['Reset'])"
    Write-Host ""
    
    if (-not (Test-Path $BasePath)) {
        Write-Error-Custom "No encontrado: $BasePath"
        Write-Host ""
        return $null
    }
    
    Write-Success "Encontrado: $BasePath"
    
    # Verificar XC8
    $xc8_dir = Join-Path $BasePath "xc8"
    
    if (-not (Test-Path $xc8_dir)) {
        Write-Error-Custom "No encontrado: $xc8_dir"
        Write-Host ""
        return $null
    }
    
    Write-Success "Encontrado: XC8"
    
    # Buscar versiones
    $versions = @(Get-ChildItem -Path $xc8_dir -Directory -Filter "v*" -ErrorAction SilentlyContinue | Sort-Object Name -Descending)
    
    if ($versions.Count -eq 0) {
        Write-Error-Custom "No hay versiones de XC8 instaladas"
        Write-Host ""
        return $null
    }
    
    Write-Info "Versiones encontradas:"
    
    $xc8_exe = $null
    
    foreach ($version in $versions) {
        $bin_path = Join-Path $version.FullName "bin"
        $xc8_bin = Join-Path $bin_path "xc8.exe"
        
        if (Test-Path $xc8_bin) {
            Write-Info "  • $($version.Name) → $bin_path"
            
            if ($null -eq $xc8_exe) {
                $xc8_exe = $xc8_bin
            }
        }
    }
    
    if ($null -eq $xc8_exe) {
        Write-Error-Custom "XC8.exe no encontrado en ninguna versión"
        Write-Host ""
        return $null
    }
    
    Write-Host ""
    return $xc8_exe
}

function Test-XC8 {
    param([string]$XC8Path)
    
    Write-Host "$($colors['Yellow'])Testeando XC8...$($colors['Reset'])"
    Write-Host ""
    
    try {
        $version_info = & $XC8Path --version 2>&1
        Write-Success "XC8 funciona correctamente"
        Write-Info "Salida: $version_info"
        Write-Host ""
        return $true
    }
    catch {
        Write-Error-Custom "Error al ejecutar XC8: $_"
        Write-Host ""
        return $false
    }
}

function Check-PATH {
    param([string]$XC8Path)
    
    $xc8_dir = Split-Path $XC8Path -Parent
    $path_entries = $env:Path -split ";"
    
    Write-Host "$($colors['Yellow'])Verificando PATH...$($colors['Reset'])"
    Write-Host ""
    
    if ($path_entries -contains $xc8_dir) {
        Write-Success "XC8 ya está en PATH"
        Write-Info $xc8_dir
        Write-Host ""
        return $true
    }
    else {
        Write-Warning-Custom "XC8 NO está en PATH"
        Write-Info "XC8 bin: $xc8_dir"
        Write-Host ""
        return $false
    }
}

function Add-ToPathPermanently {
    param([string]$XC8Dir)
    
    Write-Host "$($colors['Yellow'])Intentando agregar XC8 a PATH permanentemente...$($colors['Reset'])"
    Write-Host ""
    
    # Verificar si es admin
    $currentPrincipal = New-Object Security.Principal.WindowsPrincipal([Security.Principal.WindowsIdentity]::GetCurrent())
    $isAdmin = $currentPrincipal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
    
    if (-not $isAdmin) {
        Write-Warning-Custom "Se requieren permisos de ADMINISTRADOR"
        Write-Host ""
        Write-Host "Soluciones:"
        Write-Host "  1. Ejecutar PowerShell como ADMINISTRADOR"
        Write-Host "  2. Luego ejecutar:"
        Write-Host "     .\setup_MPLAB.ps1"
        Write-Host ""
        Write-Host "O agregarlo manualmente:"
        Write-Host "  1. Presionar Win+Pause (Propiedades del sistema)"
        Write-Host "  2. Click: Variables de entorno"
        Write-Host "  3. Agregar a PATH:"
        Write-Host "     $XC8Dir"
        Write-Host ""
        return $false
    }
    
    try {
        $current_path = [Environment]::GetEnvironmentVariable("Path", [EnvironmentVariableTarget]::User)
        
        if ($current_path -notlike "*$XC8Dir*") {
            $new_path = "$current_path;$XC8Dir"
            [Environment]::SetEnvironmentVariable("Path", $new_path, [EnvironmentVariableTarget]::User)
            
            Write-Success "XC8 agregado a PATH (usuario)"
            Write-Info $XC8Dir
            Write-Host ""
            Write-Warning-Custom "Necesita reiniciar PowerShell para aplicar cambios"
            Write-Host ""
            return $true
        }
        else {
            Write-Success "XC8 ya está en PATH"
            Write-Host ""
            return $true
        }
    }
    catch {
        Write-Error-Custom "Error al modificar PATH: $_"
        Write-Host ""
        return $false
    }
}

function Show-CompilationExample {
    param([string]$XC8Dir)
    
    Write-Host "$($colors['Blue'])Ejemplo de compilación:$($colors['Reset'])"
    Write-Host ""
    Write-Info "Comando directo:"
    Write-Host '  "$XC8Dir\xc8.exe" --chip=12f675 --format=hex --output=blink.hex blink_led_12f675.asm'
    Write-Host ""
    Write-Info "O usar los scripts:"
    Write-Host "  .\compilar_MPLAB.bat"
    Write-Host "  .\compilar_MPLAB.ps1"
    Write-Host ""
}

# ===== INICIO PRINCIPAL =====
try {
    Write-Header
    
    # Verificar instalación
    $xc8_exe = Check-MplabInstallation -BasePath $MplabPath
    
    if ($null -eq $xc8_exe) {
        Write-Error-Custom "No se pudo encontrar XC8 en $MplabPath"
        Write-Host ""
        Write-Host "Soluciones:"
        Write-Host "  1. Descargar MPLAB XC8 desde:"
        Write-Host "     https://www.microchip.com/en-us/tools-resources/develop/mplab-xc-compilers"
        Write-Host ""
        Write-Host "  2. Durante instalación seleccionar:"
        Write-Host "     ✓ PIC 12F Family"
        Write-Host "     ✓ Instalar en: $MplabPath"
        Write-Host ""
        Write-Host "  3. Ejecutar este script de nuevo"
        Write-Host ""
        exit 1
    }
    
    # Test XC8
    $xc8_works = Test-XC8 -XC8Path $xc8_exe
    
    if (-not $xc8_works) {
        Write-Error-Custom "XC8 no responde correctamente"
        Write-Host ""
        exit 1
    }
    
    # Verificar PATH
    $xc8_dir = Split-Path $xc8_exe -Parent
    $in_path = Check-PATH -XC8Path $xc8_exe
    
    # Agregar a PATH si falta
    if (-not $in_path) {
        Write-Host "$($colors['Yellow'])Intentando agregar XC8 a PATH...$($colors['Reset'])"
        Write-Host ""
        
        $added = Add-ToPathPermanently -XC8Dir $xc8_dir
        
        if ($added) {
            Write-Success "XC8 configurado correctamente"
        }
    }
    else {
        Write-Success "Instalación verificada correctamente"
    }
    
    Write-Host ""
    
    # Mostrar resumen
    Write-Host "$($colors['Blue'])================================================$($colors['Reset'])"
    Write-Host "$($colors['Green'])RESUMEN$($colors['Reset'])"
    Write-Host "$($colors['Blue'])================================================$($colors['Reset'])"
    Write-Host ""
    Write-Host "  MPLAB Path:   $MplabPath"
    Write-Host "  XC8 Exe:      $xc8_exe"
    Write-Host "  En PATH:      $($in_path ? 'Sí' : 'No')"
    Write-Host ""
    
    # Mostrar ejemplo
    Show-CompilationExample -XC8Dir $xc8_dir
    
    Write-Host "$($colors['Green'])¡Configuración completada! 🎉$($colors['Reset'])"
    Write-Host ""
    
}
catch {
    Write-Error-Custom "Error inesperado: $_"
    exit 1
}
