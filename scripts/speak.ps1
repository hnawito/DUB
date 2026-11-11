$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Speech
$speaker = New-Object System.Speech.Synthesis.SpeechSynthesizer
try {
    if ($env:DUB_VOICE) { $speaker.SelectVoice($env:DUB_VOICE) }
    $speaker.Rate = [int]$env:DUB_RATE
    $speaker.Volume = [int]$env:DUB_VOLUME
    $speaker.SetOutputToWaveFile($env:DUB_WAV)
    $text = [System.IO.File]::ReadAllText($env:DUB_TEXT, [System.Text.Encoding]::UTF8)
    $speaker.Speak($text)
} finally {
    $speaker.Dispose()
}
