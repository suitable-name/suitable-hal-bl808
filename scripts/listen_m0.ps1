param(
    [string]$Port = "COM8",
    [int]$Baud = 2000000,
    [int]$TimeoutSec = 30,
    [string]$OutFile = "C:\Users\ngussek\Dev\Rust\bl808-hal\m0_captured.txt"
)

if (Test-Path $OutFile) {
    Remove-Item -Force $OutFile
}

$sp = New-Object System.IO.Ports.SerialPort $Port, $Baud, "None", 8, "One"
$sp.ReadTimeout = 1000
try {
    $sp.Open()
    $sp.DiscardInBuffer()
    Write-Output "Listening on $Port at $Baud baud for $TimeoutSec seconds..."
    $sw = [System.Diagnostics.Stopwatch]::StartNew()
    $total = ""
    while ($sw.ElapsedMilliseconds -lt ($TimeoutSec * 1000)) {
        if ($sp.BytesToRead -gt 0) {
            $chunk = $sp.ReadExisting()
            Write-Host -NoNewline $chunk
            $total += $chunk
            [System.IO.File]::AppendAllText($OutFile, $chunk)
        } else {
            Start-Sleep -Milliseconds 20
        }
    }
    Write-Output "`nTotal received: $($total.Length) characters."
} finally {
    if ($sp.IsOpen) {
        $sp.Close()
    }
}
