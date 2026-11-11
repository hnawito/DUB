# Offline Dub (Rust, Windows 11, CPU)

A command-line source project for extracting video speech, transcribing it into editable text, synthesizing replacement speech, and rebuilding the video to match the new speech duration. Application logic is Rust; FFmpeg, whisper.cpp, Piper/sherpa-onnx and Windows speech are local native engines. There are no application HTTP clients, cloud AI calls, automatic model downloads, Python requirements, or GPU requirements.

## Build

Install Rust stable with the MSVC toolchain and Visual Studio C++ Build Tools / Windows SDK on a connected provisioning machine. Then:

```powershell
cargo build --release
cargo test
Copy-Item config.example.json config.json
```

Cargo needs access to the crate registry on the first build. For an air-gapped build, run `cargo vendor vendor` on a connected machine, retain Cargo.lock, configure Cargo source replacement as printed by that command, and transfer the toolchain and vendored source. The runtime itself needs no internet. This delivery has not been compiled here: Rust and FFmpeg are absent from the generation environment. Run the build and smoke checks before production use.

## Provision local engines and models

Paths in config.json are relative to that configuration file's directory. Use absolute paths if preferred. Forward slashes are accepted on Windows; backslashes in JSON must be escaped. CLI input/output paths are relative to your current terminal directory. Copy the engine DLLs and language data alongside each executable as required by its distribution.

1. Install a Windows FFmpeg distribution with `ffmpeg.exe`, `ffprobe.exe`, `libx264` and AAC encoding support under `tools/`.
2. Obtain/build CPU-only [whisper.cpp](https://github.com/ggml-org/whisper.cpp) (`cmake -B build -DGGML_CUDA=OFF -DGGML_VULKAN=OFF`, then `cmake --build build --config Release`). Copy whisper-cli.exe and required DLLs to tools. Place a multilingual GGML model such as ggml-base.bin in models. The application always passes `--no-gpu`. English-only models use an `.en` suffix; multilingual input requires a multilingual model.
3. Windows built-in speech requires no separate model. Only installed **SAPI/System.Speech** voices are supported. `voices` lists the exact selectable names. Set the `windows.name` config field to one of these, or leave it null for the system default. Rate is -10 through 10 and volume 0 through 100. Windows Narrator/online natural voices may not be accessible through SAPI.
4. Optional preset neural voices: supply a compatible local Piper CLI executable and a voice `.onnx` plus matching `.onnx.json`. The CLI must accept `--model`, `--speaker`, `--output_file` and UTF-8 text on stdin. Native legacy Piper distributions support this interface; newer [Piper](https://github.com/OHF-Voice/piper1-gpl) distributions may require their own launcher. This project does not invoke Python. Add more Piper entries to the config for more voices/languages; each model has its own language coverage. Do not enable CUDA in an engine launcher.
5. Optional voice cloning: obtain the Windows CPU distribution of [sherpa-onnx](https://github.com/k2-fsa/sherpa-onnx/releases) with a ZipVoice-capable `sherpa-onnx-offline-tts.exe`. Supply the extracted `sherpa-onnx-zipvoice-distill-int8-zh-en-emilia` model and `vocos_24khz.onnx` using the [official ZipVoice setup](https://k2-fsa.github.io/sherpa/onnx/tts/zipvoice.html). Its model directory must contain encoder.int8.onnx, decoder.int8.onnx, tokens.txt, lexicon.txt and espeak-ng-data. Provide a clean reference WAV and a UTF-8 reference.txt containing exactly what it says. Use a short, single-speaker reference (for example 5–15 seconds); use voices you have permission to clone. CPU provider is explicitly selected. This released cloning model supports Chinese and English, not arbitrary multilingual synthesis. More steps trade runtime for quality.

Model and engine files are intentionally not bundled: they are large and have separate licenses. Download and review their licenses during provisioning, then copy all assets onto the offline machine. Piper model licenses vary, current Piper is GPL, FFmpeg licensing depends on its build, whisper.cpp is MIT, and sherpa-onnx is Apache-2.0; review the specific model cards too. Redistribution of a complete binary bundle needs its corresponding notices and any source obligations.

## Use

From the repository folder:

```powershell
$dub = '.\target\release\offline-dub.exe'
& $dub doctor
& $dub voices
& $dub doctor --voice windows
& $dub prepare 'C:\Videos\input.mp4' '.\work-demo' --language auto
notepad '.\work-demo\transcript.txt'
# Save as UTF-8 and close the editor, then:
& $dub render '.\work-demo' '.\dubbed.mp4' --voice windows
# Or choose a local neural preset / reference voice:
& $dub render '.\work-demo' '.\dubbed-piper.mp4' --voice lessac
& $dub render '.\work-demo' '.\dubbed-clone.mp4' --voice clone
```

Use `--config C:\path\config.json` with any subcommand. `--help` describes the CLI. Doctor can synthesize a short probe for a chosen voice; it does not certify transcription quality or enforce an OS firewall. No engine auto-fetch is requested. For a strictly isolated deployment, use verified local engine builds and disconnect the machine or block outbound traffic using site policy.

Each new project retains `source.mp4` (a byte-for-byte copy of the input, whose actual container is auto-detected), `speech.wav` (16 kHz mono PCM), and `transcript.txt`. Open the text in any editor. The file is plain prose, without timing markup. Retain punctuation and add pauses using punctuation supported by your selected synthesizer. Speech recognition can make mistakes, so review the transcript before rendering. Inputs must contain a video stream and an audio stream.

Rendering synthesizes the edited transcript and computes `speech_duration / original_video_duration`. FFmpeg applies that multiplier to video timestamps, writes 30 fps H.264/AAC MP4, and sets the output duration to the generated audio duration. Shorter speech speeds up video; longer speech slows it down. Original soundtrack is replaced entirely, including music and effects; source separation, speaker diarization, subtitle preservation, word alignment and lip sync are not implemented. One voice is used for the entire script. Video re-encoding is CPU-only. Frame rounding may produce a difference of roughly one frame in duration.

Output paths must be new `.mp4` files. A failed render leaves existing outputs untouched. Temporary WAV/video artifacts are removed through Rust RAII. A failed prepare may leave a new empty project directory: inspect and remove that directory or choose a fresh name. Completed source/WAV/text files are retained for editing. Cloning passes text to the local native CLI, so long transcripts that would exceed the Windows command-line limit are rejected with an explanation; use shorter projects for that backend.

## Hardware planning

Start evaluation with a modern x64 CPU with 8 physical cores, 16–32 GB RAM and an SSD. This is a planning baseline, not a measured minimum or real-time guarantee. Begin with Whisper base and the int8 cloning model, then measure representative files using your selected voice and language. Budget storage for the source copy, extracted WAV, models, temporary output and final output. A 16 kHz mono PCM WAV takes about 115 MB per hour. Cloning and H.264 encoding can take substantial CPU time; increase or reduce `threads` for Whisper/sherpa as appropriate. Piper and FFmpeg may manage their own worker counts.

# Offline Dub — current instructions

This guide describes the current Windows CPU-only program. Rendering now follows the start and end times in `transcript.srt`; it does not stretch the entire video. 

## Start here

Open PowerShell in the project directory:

```powershell
cd C:\D\mywork\Video_RUST
$dub = '.\target\release\offline-dub.exe'
& $dub --help
```

This is a command-line program, with no graphical window. Paths containing spaces must be quoted. Run commands from this directory unless you supply an absolute `--config` path.

## Installed components

| Component | Location | Status |
|---|---|---|
| Release executable | `target/release/offline-dub.exe` | Compiled locally |
| FFmpeg / FFprobe | `tools/ffmpeg.exe`, `tools/ffprobe.exe` | H.264 and AAC encoding tested |
| whisper.cpp CPU | `tools/whisper-cli.exe` and DLLs | Transcription tested |
| Multilingual Whisper base | `models/ggml-base.bin` | Installed; language detection supported |
| Windows SAPI speech | Windows-installed voices | David and Zira available |
| Native Piper CLI | `tools/piper/piper.exe` and accompanying files | Installed; no Python needed |
| Kareem medium | `models/ar_JO-kareem-medium.onnx` and `.onnx.json` | Arabic synthesis tested |
| sherpa-onnx / ZipVoice CPU | `tools/sherpa/` | English synthesis tested |
| ZipVoice English/Chinese model | `models/sherpa-onnx-zipvoice-distill-int8-zh-en-emilia/` | Installed, including English pronunciation dictionary |
| ZipVoice vocoder | `models/vocos_24khz.onnx` | Installed |

Keep executables, DLLs, model configuration files, and engine data directories together. The program does not download missing engines or models. Installed engines operate locally; obtaining additional files initially requires an internet connection.

## Voice presets in config.json

| `--voice` value | Description | Ready? |
|---|---|---|
| `windows` | System default SAPI voice, currently Microsoft David Desktop (English male) | Yes |
| `zira` | Microsoft Zira Desktop (English female) | Yes |
| `Ava Multilingual` | ZipVoice reference preset made from the first 12 seconds of your video's extracted speech | Yes |
| `kareem` | Piper Kareem medium, Arabic (`ar_JO`) | Yes |
| `lessac` | Piper English preset | No: its `.onnx` and `.onnx.json` files are not installed |
| `clone` | Generic ZipVoice preset | No: `models/reference.wav` and `models/reference.txt` are missing |

`Ava Multilingual` is a user-chosen preset label. It reproduces the speaker in your reference recording approximately; it is not Microsoft's Ava model and does not add multilingual support to ZipVoice. The installed ZipVoice model supports English and Chinese, not Arabic. Kareem is the installed Arabic option. Voice selection does not translate text: use an Arabic transcript with Kareem.

Other previously detected SAPI voices: Microsoft Haruka Desktop (Japanese), Microsoft Heami Desktop (Korean), Microsoft Huihui Desktop (Chinese), and Microsoft Hanhan Desktop (Chinese). To use one, add a SAPI preset with its exact name. To inspect voices:

```powershell
& $dub voices
& $dub doctor --voice zira
& $dub doctor --voice kareem
& $dub doctor --voice 'Ava Multilingual'
```

`doctor` checks the core transcription tools and optionally synthesizes a short voice probe. It does not certify pronunciation or overall speech quality.

## 1. Extract audio and a timed transcript

The destination project folder must not already exist:

```powershell
& $dub prepare 'C:\Videos\input.mp4' '.\work-new' --language auto
```

The project contains:

- `source.mp4`: a byte-for-byte copy of the input; actual container detected by FFmpeg.
- `speech.wav`: extracted 16 kHz mono speech audio.
- `transcript.srt`: editable text with segment start/end timestamps; **this controls rendering**.
- `transcript.txt`: plain-text reference copy; changes here do not affect timed rendering.

`--language auto` detects the spoken language. Use `--language en` or `--language ar` when known. The installed multilingual model supports these languages; an English-only `.en` model is not suitable for Arabic transcription.

## 2. Edit the timed transcript

```powershell
notepad '.\work-new\transcript.srt'
```

Example:

```srt
1
00:00:01,000 --> 00:00:04,500
Welcome to this demonstration.

2
00:00:06,000 --> 00:00:09,000
This sentence starts at six seconds.
```

Save as UTF-8. Keep cue numbers, the `-->` separator, timestamps in `HH:MM:SS,mmm` format, and a blank line between cues. Multiline text within a cue is supported. Use ordinary text; subtitle styling tags are not interpreted.

For exact original timing, edit only the text. If translating, place each translated segment inside the corresponding original cue. Cues must be ordered, must not overlap, must end after they start, and must stay within the video duration. Invalid timing is rejected before speech generation. An empty cue produces silence in its time slot.

### Existing Sample project

`work-Sample/transcript.srt` has been recovered from the original extraction timestamps and the corresponding 107 lines of the edited plain transcript. Continue editing this SRT file:

```powershell
notepad '.\work-Sample\transcript.srt'
```

Older projects without a timed transcript must be prepared again into a new folder, or supplied with a valid SRT that matches the source. A plain transcript alone cannot reconstruct exact original timing.

## 3. Render an MP4

The output must be a new `.mp4` file; existing files are not overwritten:

```powershell
& $dub render '.\work-Sample' '.\dubbed-david-timed.mp4' --voice windows
& $dub render '.\work-Sample' '.\dubbed-zira-timed.mp4' --voice zira
& $dub render '.\work-Sample' '.\dubbed-ava-timed.mp4' --voice 'Ava Multilingual'
```

For an Arabic project with Arabic text in its SRT:

```powershell
& $dub render '.\work-arabic' '.\dubbed-kareem-timed.mp4' --voice kareem
```

### How timing works

1. Each cue is synthesized separately in the selected voice.
2. Its speech is sped up or slowed down, preserving pitch, to fit the cue's duration.
3. The segment is placed at its original start time in a 48 kHz audio timeline. Silence fills gaps before, between, and after cues.
4. The original video stream is copied into the output without changing its playback speed. Narration replaces the entire original soundtrack.

Placement uses the SRT's millisecond timestamps, with sample-length padding/trimming to prevent spillover into the next cue. Final AAC encoding has normal codec/frame precision; the feature is segment alignment, not word-level lip synchronization. Whisper timestamps are estimates of the original speech boundaries: review and adjust them if needed. Very large speech-speed changes can sound unnatural, so shorten or rewrite text when a time slot is too short.

The video stream must be compatible with an MP4 container because rendering now copies it without re-encoding. H.264/AAC source videos such as the current telecom demo are suitable. Unsupported source video codecs may require conversion first. Subtitles, extra audio streams, background music, speaker diarization, and lip synchronization are not preserved or generated.

The earlier files `renders-20261109-012450/david.mp4` and `zira.mp4` were generated with the old whole-video speed adjustment. They do not demonstrate the new timed mode. The old Ava render was stopped before completion when timed rendering was requested.

## Command reference

| Command | Options / arguments |
|---|---|
| `prepare <VIDEO> <PROJECT>` | `--language <LANG>`; default `auto`; project must be new |
| `render <PROJECT> <OUTPUT>` | `--voice <PRESET>`; default `windows`; requires `transcript.srt`; output must be new `.mp4` |
| `voices` | List preset keys and installed SAPI names |
| `doctor` | Optional `--voice <PRESET>` to test synthesis |
| `help [COMMAND]` | Show command help |
| `--config <FILE>` | Global option; default `config.json` |
| `--help`, `-h` | Show help |
| `--version`, `-V` | Show version |

Example with a configuration elsewhere:

```powershell
& $dub --config 'C:\D\mywork\Video_RUST\config.json' render '.\work-new' '.\result.mp4' --voice zira
```

Configuration file paths to engines/models are resolved relative to the configuration file. Command-line project/input/output paths are resolved relative to your current terminal directory.

## Configuration options

- `ffmpeg`, `ffprobe`, `whisper`, `whisper_model`: executable/model paths.
- `threads`: positive CPU thread count for Whisper and ZipVoice. It does not govern every engine.
- `voices`: map of selectable preset names.
- SAPI: `engine: "sapi"`, `name` (exact installed name or `null` for default), `rate` (-10 to 10), `volume` (0 to 100).
- Piper: `engine: "piper"`, `executable`, `model`, `speaker` (usually 0). The matching `MODEL.onnx.json` must exist.
- ZipVoice: `engine: "zipvoice"`, `executable`, `model_dir`, `vocoder`, `reference_audio`, `reference_text`, `steps` (positive; currently 4).

ZipVoice reference text must match its reference recording exactly. Use a clean, short, single-speaker recording. The saved `Ava Multilingual` reference lives in `models/voices/ava-multilingual/`. Original extracted `speech.wav` remains untouched. Check rights for any reference recordings and model distribution licenses before reuse.

## Compile and validate

Rust stable MSVC, Visual Studio C++ Build Tools, and the Windows SDK are required for compilation. Initial dependency installation requires registry access; later offline builds require cached/vendored dependencies.

```powershell
cd C:\D\mywork\Video_RUST
cargo fmt --check
cargo test
cargo clippy --all-targets -- -D warnings
cargo build --release
powershell.exe -NoProfile -ExecutionPolicy Bypass -File .\scripts\smoke-test.ps1
```

The release program is `target/release/offline-dub.exe`. Run it with this project's configuration, speech script (compiled into the executable), installed engines, and models available at the configured paths. The smoke test exercises extraction and timed Windows speech rendering, original video preservation, total duration, and silence between cues. Speech quality still needs listening review.

## Troubleshooting

- **No default Rust toolchain:** run `rustup default stable`.
- **Cargo.toml not found:** first run `cd C:\D\mywork\Video_RUST`.
- **Missing transcript.srt:** prepare a new project or recover the original segment timestamps; do not invent timings from plain text.
- **Overlapping/out-of-range segment:** correct the indicated SRT cue.
- **Missing model/reference:** check the preset's paths and install its required files.
- **SAPI errors:** ensure the exact voice is installed and run from your normal Windows user session.
- **Output already exists:** choose a new output filename.
- **Slow cloning:** ZipVoice generates every cue on CPU; it takes longer than Windows/Piper voices. Wait for segment progress in the terminal.
- **Wrong language/pronunciation:** use a voice that supports the transcript's language; the program does not translate text.
