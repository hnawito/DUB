param([string]$Config = "$PSScriptRoot/../config.json")
$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent
$exe = Join-Path $root 'target/release/offline-dub.exe'
$cfgPath = (Resolve-Path $Config).Path
$cfg = Get-Content -Raw $cfgPath | ConvertFrom-Json
$cfgDir = Split-Path $cfgPath -Parent
$ffmpeg = if ([IO.Path]::IsPathRooted($cfg.ffmpeg)) { $cfg.ffmpeg } else { Join-Path $cfgDir $cfg.ffmpeg }
$ffprobe = if ([IO.Path]::IsPathRooted($cfg.ffprobe)) { $cfg.ffprobe } else { Join-Path $cfgDir $cfg.ffprobe }
$scratch = Join-Path ([IO.Path]::GetTempPath()) ('offline-dub-test-' + [guid]::NewGuid())
New-Item -ItemType Directory $scratch | Out-Null
try {
    $env:DUB_TEXT = Join-Path $scratch 'known.txt'
    $env:DUB_WAV = Join-Path $scratch 'known.wav'
    $env:DUB_VOICE = ''
    $env:DUB_RATE = '0'
    $env:DUB_VOLUME = '100'
    [IO.File]::WriteAllText($env:DUB_TEXT, 'Hello. This video demonstrates local speech generation.')
    & "$PSScriptRoot/speak.ps1"
    & $ffmpeg -v error -y -f lavfi -i 'testsrc2=size=640x360:rate=30:duration=10' -i $env:DUB_WAV -c:v libx264 -c:a aac -af apad -t 10 (Join-Path $scratch 'input.mp4')
    if ($LASTEXITCODE -ne 0) { throw 'Fixture encoding failed' }
    & $exe --config $cfgPath prepare (Join-Path $scratch 'input.mp4') (Join-Path $scratch 'project') --language en
    if ($LASTEXITCODE -ne 0) { throw 'Prepare failed' }
    foreach ($name in @('speech.wav', 'transcript.txt', 'transcript.srt', 'source.mp4')) {
        if (!(Test-Path (Join-Path $scratch "project/$name"))) { throw "Missing $name" }
    }
    [IO.File]::WriteAllText((Join-Path $scratch 'project/transcript.srt'), @'
1
00:00:01,000 --> 00:00:03,000
Hello. These edited words must fit inside exactly two seconds.

2
00:00:06,000 --> 00:00:09,000
Hello again.

'@)
    $output = Join-Path $scratch 'result.mp4'
    & $exe --config $cfgPath render (Join-Path $scratch 'project') $output --voice windows
    if ($LASTEXITCODE -ne 0) { throw 'Render failed' }
    $videoSeconds = & $ffprobe -v error -select_streams v:0 -show_entries stream=duration -of csv=p=0 $output
    if ($LASTEXITCODE -ne 0) { throw 'Video probe failed' }
    $audioSeconds = & $ffprobe -v error -select_streams a:0 -show_entries stream=duration -of csv=p=0 $output
    if ($LASTEXITCODE -ne 0) { throw 'Audio probe failed' }
    $culture = [Globalization.CultureInfo]::InvariantCulture
    $v = [double]::Parse($videoSeconds, $culture)
    $a = [double]::Parse($audioSeconds, $culture)
    if ([Math]::Abs($v - 10) -gt 0.04 -or [Math]::Abs($a - 10) -gt 0.04) { throw 'Original duration was not preserved' }
    $sourceHash = & $ffmpeg -v error -i (Join-Path $scratch 'input.mp4') -map 0:v:0 -c:v copy -f hash -
    if ($LASTEXITCODE -ne 0) { throw 'Source video hash failed' }
    $outputHash = & $ffmpeg -v error -i $output -map 0:v:0 -c:v copy -f hash -
    if ($LASTEXITCODE -ne 0 -or $sourceHash -ne $outputHash) { throw 'Video stream changed' }
    # Check silence inside the leading, inter-segment and trailing gaps, away from AAC boundaries.
    foreach ($gap in @(@('0.1','0.7'), @('3.2','2.5'), @('9.2','0.6'))) {
        # Decode only the selected range so volumedetect sees just this gap.
        # Windows PowerShell treats native stderr diagnostics as errors under Stop.
        $savedPreference = $ErrorActionPreference
        try {
            $ErrorActionPreference = 'Continue'
            $level = & $ffmpeg -hide_banner -ss $gap[0] -t $gap[1] -i $output -af volumedetect -vn -f null NUL 2>&1 | Out-String
        } finally { $ErrorActionPreference = $savedPreference }
        if ($LASTEXITCODE -ne 0 -or $level -notmatch 'max_volume:\s+(-?[\d.]+) dB') { throw 'Missing silence measurement' }
        if ([double]::Parse($Matches[1], $culture) -gt -70) { throw 'Narration spilled into a silent gap' }
    }
    Write-Host "Smoke test passed: video=$v audio=$a"
} finally {
    # Only the uniquely named test directory is eligible for removal.
    $resolved = [IO.Path]::GetFullPath($scratch)
    $tempRoot = [IO.Path]::GetFullPath([IO.Path]::GetTempPath())
    if ($resolved.StartsWith($tempRoot, [StringComparison]::OrdinalIgnoreCase) -and (Split-Path $resolved -Leaf).StartsWith('offline-dub-test-')) {
        Remove-Item -LiteralPath $resolved -Recurse -Force
    }
    foreach ($key in @('DUB_TEXT','DUB_WAV','DUB_VOICE','DUB_RATE','DUB_VOLUME')) { [Environment]::SetEnvironmentVariable($key, $null, 'Process') }
}
