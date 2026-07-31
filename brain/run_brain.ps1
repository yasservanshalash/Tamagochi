# Launches the Yasser brain server with .env loaded.
$here = Split-Path -Parent $MyInvocation.MyCommand.Path
Set-Location $here

Get-Content (Join-Path $here ".env") | ForEach-Object {
    if ($_ -match '^\s*([^#=]+)=(.*)$') {
        [Environment]::SetEnvironmentVariable($Matches[1].Trim(), $Matches[2].Trim(), 'Process')
    }
}
$Env:PYTHONIOENCODING = 'utf-8'
$Env:PYTHONUNBUFFERED = '1'
$Env:ONNX_PROVIDER = 'CPUExecutionProvider'   # kokoro TTS stays on CPU (DirectML breaks it)

python -m uvicorn think_server:app --host 0.0.0.0 --port 8087
