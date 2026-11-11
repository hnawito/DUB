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

## Ava multilingual

The project does not bundle or imitate a proprietary Ava model. If an Ava voice is installed and exposed through SAPI it can be selected by exact name; otherwise select an available local Piper model or clone a permitted reference with the supported language model. An exact Ava voice and broad multilingual cloning are not promised by these offline engines.

## Validation and cleanup

Run `cargo fmt --check`, `cargo test`, `cargo clippy --all-targets -- -D warnings`, then `scripts/smoke-test.ps1` after provisioning the default engines. The smoke test creates a synthetic video, transcribes it, substitutes known text, renders Windows speech, and checks duration. It requires a working default SAPI voice and the configured Whisper model. The speech-quality result needs listening review. Test each optional voice with `doctor --voice NAME` and a real project on your target hardware.

Generated dependency/build directories are excluded by .gitignore. To reclaim compilation artifacts run `cargo clean`. Do not delete models or working projects unless you no longer need them. The delivered directory contains only project sources, configuration example, documentation and a source ZIP; no model caches or temporary media.
