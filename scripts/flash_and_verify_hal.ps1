param(
    [string]$Port = "COM8",
    [int]$Baud = 2000000,
    [string]$DevCubeDir = "C:\tmp\BouffaloLabDevCube-v1.9.0",
    [string]$FirmwareBin = "C:\Users\ngussek\Dev\Rust\bl808-hal\m0_hal_test.bin"
)

$ErrorActionPreference = "Stop"

Write-Output "================================================================"
Write-Output "       BL808 HAL BARE-METAL FIRMWARE FLASH & RUN (Sipeed M1s)    "
Write-Output "================================================================"

# 1. Build Boot2 image
Write-Output "Building BouffaloLab Boot2 container..."
$buildOut = & "$DevCubeDir\bflb_iot_tool.exe" --chipname bl808 --build --firmware $FirmwareBin 2>&1
if ($LASTEXITCODE -ne 0 -or $buildOut -notmatch "eflash loader config") {
    Write-Error "Failed to package Boot2 image: $buildOut"
}

$flashData = "$DevCubeDir\chips\bl808\img_create_iot\whole_flash_data.bin"
if (-not (Test-Path $flashData)) {
    Write-Error "Cannot find $flashData"
}

Write-Output "Flashing $flashData to $Port @ $Baud baud..."
$flashOut = & "$DevCubeDir\bflb_iot_tool.exe" --chipname bl808 --interface uart --port $Port --baudrate $Baud --single --addr 0x0 --firmware $flashData 2>&1
Write-Output $flashOut

if ($flashOut -match "\[All Success\]" -or $flashOut -match "Verify success") {
    Write-Output "`n*** FLASH PROGRAMMING SUCCESSFUL ***`n"
    Write-Output "Starting serial monitor on $Port @ $Baud baud for 15 seconds..."
    
    $sp = New-Object System.IO.Ports.SerialPort $Port, $Baud, "None", 8, "One"
    $sp.ReadTimeout = 1000
    try {
        $sp.Open()
        $sp.DiscardInBuffer()
        $sw = [System.Diagnostics.Stopwatch]::StartNew()
        $captured = ""
        while ($sw.ElapsedMilliseconds -lt 15000) {
            if ($sp.BytesToRead -gt 0) {
                $chunk = $sp.ReadExisting()
                Write-Host -NoNewline $chunk
                $captured += $chunk
            } else {
                Start-Sleep -Milliseconds 20
            }
        }
        Write-Output "`n`nTotal captured: $($captured.Length) characters."
        return $captured
    } finally {
        if ($sp.IsOpen) {
            $sp.Close()
        }
    }
} else {
    Write-Warning "BootROM handshake failed. Device is not in BootROM mode."
    return $null
}
