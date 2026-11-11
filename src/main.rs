use anyhow::{ensure, Context, Result};
use clap::{Parser, Subcommand};
use serde::Deserialize;
use std::io::{Seek, SeekFrom};
use std::{
    collections::BTreeMap,
    ffi::OsStr,
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

#[derive(Parser)]
#[command(version, about = "Offline CPU video transcription and dubbing")]
struct Cli {
    #[arg(long, default_value = "config.json", global = true)]
    config: PathBuf,
    #[command(subcommand)]
    action: Action,
}

#[derive(Subcommand)]
enum Action {
    /// Extract speech.wav, transcript.txt and editable timed transcript.srt.
    Prepare {
        video: PathBuf,
        project: PathBuf,
        #[arg(long, default_value = "auto")]
        language: String,
    },
    /// Fit each transcript.srt segment to its time slot, preserving video timing.
    Render {
        project: PathBuf,
        output: PathBuf,
        #[arg(long, default_value = "windows")]
        voice: String,
    },
    /// List configured presets and locally installed Windows SAPI voices.
    Voices,
    /// Check local dependencies for transcription and an optional voice.
    Doctor {
        #[arg(long)]
        voice: Option<String>,
    },
}

#[derive(Deserialize)]
struct Config {
    ffmpeg: PathBuf,
    ffprobe: PathBuf,
    whisper: PathBuf,
    whisper_model: PathBuf,
    threads: usize,
    voices: BTreeMap<String, Voice>,
}

#[derive(Deserialize)]
#[serde(tag = "engine", rename_all = "lowercase")]
enum Voice {
    Sapi {
        name: Option<String>,
        rate: i32,
        volume: u32,
    },
    Piper {
        executable: PathBuf,
        model: PathBuf,
        speaker: u32,
    },
    Zipvoice {
        executable: PathBuf,
        model_dir: PathBuf,
        vocoder: PathBuf,
        reference_audio: PathBuf,
        reference_text: PathBuf,
        steps: u32,
    },
}

fn file(path: &Path) -> Result<()> {
    ensure!(path.is_file(), "Missing file: {}", path.display());
    Ok(())
}

fn run(cmd: &mut Command) -> Result<()> {
    let label = format!("{:?}", cmd.get_program());
    let status = cmd
        .status()
        .with_context(|| format!("Could not launch {label}"))?;
    ensure!(status.success(), "{label} failed: {status}");
    Ok(())
}

// No shell interpolation is used for media paths or edited text.
fn ffmpeg(c: &Config) -> Command {
    let mut cmd = Command::new(&c.ffmpeg);
    cmd.args(["-hide_banner", "-nostdin", "-v", "error", "-y"]);
    cmd
}

fn duration(c: &Config, path: &Path, stream: &str) -> Result<f64> {
    let result = Command::new(&c.ffprobe)
        .args([
            "-v",
            "error",
            "-select_streams",
            stream,
            "-show_entries",
            "stream=duration",
            "-of",
            "csv=p=0",
        ])
        .arg(path)
        .output()
        .context("Launching ffprobe")?;
    ensure!(
        result.status.success(),
        "ffprobe failed: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    let raw = String::from_utf8_lossy(&result.stdout);
    if let Ok(n) = raw.trim().parse::<f64>() {
        if n.is_finite() && n > 0.0 {
            return Ok(n);
        }
    }
    // Some containers do not report stream duration. Verify a stream exists first.
    let check = Command::new(&c.ffprobe)
        .args([
            "-v",
            "error",
            "-select_streams",
            stream,
            "-show_entries",
            "stream=index",
            "-of",
            "csv=p=0",
        ])
        .arg(path)
        .output()?;
    ensure!(
        check.status.success() && !check.stdout.is_empty(),
        "No {stream} stream in {}",
        path.display()
    );
    let fallback = Command::new(&c.ffprobe)
        .args([
            "-v",
            "error",
            "-show_entries",
            "format=duration",
            "-of",
            "csv=p=0",
        ])
        .arg(path)
        .output()?;
    ensure!(fallback.status.success(), "Cannot probe media duration");
    let n: f64 = String::from_utf8_lossy(&fallback.stdout)
        .trim()
        .parse()
        .context("Invalid media duration")?;
    ensure!(n.is_finite() && n > 0.0, "Media duration must be positive");
    Ok(n)
}

fn ps(script: &str) -> Command {
    let mut cmd = Command::new("powershell.exe");
    cmd.args([
        "-NoLogo",
        "-NoProfile",
        "-NonInteractive",
        "-Command",
        script,
    ]);
    cmd
}

fn synthesize(c: &Config, voice: &Voice, text_path: &Path, wav: &Path) -> Result<()> {
    let text = fs::read_to_string(text_path).context("Transcript must be UTF-8")?;
    ensure!(!text.trim().is_empty(), "Transcript is empty");
    match voice {
        Voice::Sapi { name, rate, volume } => {
            ensure!(cfg!(windows), "SAPI requires Windows");
            ensure!(
                (-10..=10).contains(rate) && *volume <= 100,
                "Invalid SAPI rate/volume"
            );
            let mut cmd = ps(include_str!("../scripts/speak.ps1"));
            cmd.env("DUB_TEXT", text_path)
                .env("DUB_WAV", wav)
                .env("DUB_VOICE", name.as_deref().unwrap_or(""))
                .env("DUB_RATE", rate.to_string())
                .env("DUB_VOLUME", volume.to_string());
            run(&mut cmd)?;
        }
        Voice::Piper {
            executable,
            model,
            speaker,
        } => {
            file(model)?;
            file(&PathBuf::from(format!("{}.json", model.display())))?;
            // Read from a file handle: avoids pipe deadlocks and handles multiline UTF-8.
            let mut cmd = Command::new(executable);
            cmd.arg("--model")
                .arg(model)
                .arg("--speaker")
                .arg(speaker.to_string())
                .arg("--output_file")
                .arg(wav)
                .stdin(Stdio::from(fs::File::open(text_path)?));
            run(&mut cmd)?;
        }
        Voice::Zipvoice {
            executable,
            model_dir,
            vocoder,
            reference_audio,
            reference_text,
            steps,
        } => {
            ensure!(*steps > 0, "Cloning steps must be positive");
            for p in [vocoder, reference_audio, reference_text] {
                file(p)?;
            }
            let reference = fs::read_to_string(reference_text)?;
            ensure!(
                !reference.trim().is_empty(),
                "Reference transcript is empty"
            );
            let mut cmd = Command::new(executable);
            for (flag, name) in [
                ("encoder", "encoder.int8.onnx"),
                ("decoder", "decoder.int8.onnx"),
                ("lexicon", "lexicon.txt"),
                ("tokens", "tokens.txt"),
            ] {
                let path = model_dir.join(name);
                file(&path)?;
                cmd.arg(format!("--zipvoice-{flag}={}", path.display()));
            }
            ensure!(
                model_dir.join("espeak-ng-data").is_dir(),
                "Missing espeak-ng-data directory"
            );
            cmd.arg(format!(
                "--zipvoice-data-dir={}",
                model_dir.join("espeak-ng-data").display()
            ))
            .arg(format!("--zipvoice-vocoder={}", vocoder.display()))
            .arg(format!("--reference-audio={}", reference_audio.display()))
            .arg(format!("--reference-text={}", reference.trim()))
            .arg(format!("--num-steps={steps}"))
            .arg(format!("--num-threads={}", c.threads))
            .arg("--provider=cpu")
            .arg(format!("--output-filename={}", wav.display()))
            .arg(text.trim());
            // Windows command lines have a 32767 UTF-16 character limit.
            let units: usize = cmd
                .get_args()
                .map(|a| a.to_string_lossy().encode_utf16().count() + 3)
                .sum();
            ensure!(
                units < 28000,
                "Clone text too long for Windows CLI; split into smaller projects"
            );
            run(&mut cmd)?;
        }
    }
    file(wav)?;
    Ok(())
}

fn prepare(c: &Config, video: &Path, project: &Path, language: &str) -> Result<()> {
    file(video)?;
    file(&c.whisper_model)?;
    let source = fs::canonicalize(video)?;
    // Atomic create prevents accidental reuse or overwriting of earlier edits.
    fs::create_dir(project).context("Project directory must be new (parent must exist)")?;
    let work = tempfile::tempdir_in(project)?;
    let staged_video = work.path().join("source.mp4");
    // Retain original container extension; FFmpeg detects actual container on input.
    fs::copy(&source, &staged_video).context("Copying source video")?;
    duration(c, &staged_video, "v:0")?;
    let wav = work.path().join("speech.wav");
    run(ffmpeg(c)
        .arg("-i")
        .arg(&staged_video)
        .args([
            "-map",
            "0:a:0",
            "-vn",
            "-ac",
            "1",
            "-ar",
            "16000",
            "-c:a",
            "pcm_s16le",
        ])
        .arg(&wav))?;
    let base = work.path().join("transcript");
    run(Command::new(&c.whisper)
        .arg("-m")
        .arg(&c.whisper_model)
        .arg("-f")
        .arg(&wav)
        .args(["--no-gpu", "-t"])
        .arg(c.threads.to_string())
        .args(["-l", language, "-otxt", "-osrt", "-of"])
        .arg(&base))?;
    file(&base.with_extension("txt"))?;
    parse_srt(&fs::read_to_string(base.with_extension("srt"))?)?;
    for name in [
        "source.mp4",
        "speech.wav",
        "transcript.txt",
        "transcript.srt",
    ] {
        fs::rename(work.path().join(name), project.join(name))?;
    }
    println!("Edit {} (keep the timestamps), save as UTF-8, then run render. transcript.txt is a plain-text reference only.", project.join("transcript.srt").display());
    Ok(())
}

fn render(c: &Config, project: &Path, output: &Path, voice: &Voice) -> Result<()> {
    ensure!(
        output.extension() == Some(OsStr::new("mp4")),
        "Output must use .mp4 extension"
    );
    ensure!(!output.exists(), "Output already exists; choose a new path");
    let project = fs::canonicalize(project)?;
    let source = project.join("source.mp4");
    file(&source)?;
    let work = tempfile::tempdir_in(&project)?;
    let video_seconds = duration(c, &source, "v:0")?;
    let srt = project.join("transcript.srt");
    let segments = parse_srt(&fs::read_to_string(&srt).context("Missing or unreadable transcript.srt. Re-run prepare to extract timestamps; edit transcript.srt before rendering.")?)?;
    validate_timeline(&segments, video_seconds)?;
    let speech = timed_speech(c, voice, &segments, video_seconds, work.path())?;
    println!(
        "Preserving original video timing: {video_seconds:.3}s; {} timed segments",
        segments.len()
    );
    let staged = work.path().join("dubbed.mp4");
    run(ffmpeg(c)
        .arg("-i")
        .arg(&source)
        .args(["-f", "s16le", "-ar", "48000", "-ac", "1", "-i"])
        .arg(&speech)
        .args([
            "-map", "0:v:0", "-map", "1:a:0", "-c:v", "copy", "-c:a", "aac", "-b:a", "192k", "-t",
        ])
        .arg(format!("{video_seconds:.9}"))
        .args(["-movflags", "+faststart"])
        .arg(&staged))?;
    duration(c, &staged, "v:0")?;
    duration(c, &staged, "a:0")?;
    // persist_noclobber works across volumes and never overwrites an existing file.
    let parent = output
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let mut destination = tempfile::NamedTempFile::new_in(parent)?;
    std::io::copy(&mut fs::File::open(&staged)?, &mut destination)?;
    destination.as_file().sync_all()?;
    destination.persist_noclobber(output).map_err(|e| e.error)?;
    println!("Created {}", output.display());
    Ok(())
}

#[derive(Debug)]
struct Segment {
    start_ms: u64,
    end_ms: u64,
    text: String,
}

fn timestamp_ms(value: &str) -> Result<u64> {
    let parts: Vec<&str> = value.trim().split([':', ',']).collect();
    ensure!(
        parts.len() == 4
            && parts[0].len() >= 2
            && parts[1].len() == 2
            && parts[2].len() == 2
            && parts[3].len() == 3
            && parts.iter().all(|p| p.bytes().all(|b| b.is_ascii_digit())),
        "Invalid SRT timestamp: {value}; expected HH:MM:SS,mmm"
    );
    let n: Vec<u64> = parts
        .iter()
        .map(|p| p.parse())
        .collect::<std::result::Result<_, _>>()?;
    ensure!(n[1] < 60 && n[2] < 60, "Invalid SRT timestamp: {value}");
    n[0].checked_mul(3_600_000)
        .and_then(|v| v.checked_add(n[1] * 60_000 + n[2] * 1000 + n[3]))
        .context("Timestamp is too large")
}

fn parse_srt(input: &str) -> Result<Vec<Segment>> {
    let normalized = input.trim_start_matches('\u{feff}').replace("\r\n", "\n");
    let mut segments = Vec::new();
    let mut lines = normalized.lines().peekable();
    while lines.peek().is_some() {
        while lines.peek().is_some_and(|line| line.trim().is_empty()) {
            lines.next();
        }
        let Some(index) = lines.next() else { break };
        index
            .trim()
            .parse::<u64>()
            .context("Expected a numeric SRT cue index")?;
        let timing = lines.next().context("Missing SRT timestamps")?;
        let (start, end) = timing
            .split_once("-->")
            .context("Expected SRT start --> end")?;
        let mut text = Vec::new();
        while lines.peek().is_some_and(|line| !line.trim().is_empty()) {
            text.push(lines.next().unwrap().trim());
        }
        segments.push(Segment {
            start_ms: timestamp_ms(start)?,
            end_ms: timestamp_ms(end)?,
            text: text.join(" "),
        });
    }
    ensure!(!segments.is_empty(), "Timed transcript has no segments");
    Ok(segments)
}

fn validate_timeline(segments: &[Segment], seconds: f64) -> Result<()> {
    ensure!(
        seconds.is_finite() && seconds > 0.0,
        "Invalid video duration"
    );
    let last_ms = (seconds * 1000.0).round() as u64;
    let mut previous_end = 0;
    for (i, segment) in segments.iter().enumerate() {
        ensure!(
            segment.end_ms > segment.start_ms,
            "Segment {} must end after it starts",
            i + 1
        );
        ensure!(
            segment.start_ms >= previous_end,
            "Segment {} overlaps or precedes the previous segment",
            i + 1
        );
        ensure!(
            segment.end_ms <= last_ms,
            "Segment {} ends beyond the video ({seconds:.3}s)",
            i + 1
        );
        previous_end = segment.end_ms;
    }
    Ok(())
}

// FFmpeg atempo preserves pitch; chain moderate factors for very short/long slots.
fn tempo_filter(mut factor: f64) -> Result<String> {
    ensure!(factor.is_finite() && factor > 0.0, "Invalid speech tempo");
    let mut filters = Vec::new();
    while factor > 2.0 {
        filters.push("atempo=2".to_owned());
        factor /= 2.0;
    }
    while factor < 0.5 {
        filters.push("atempo=0.5".to_owned());
        factor /= 0.5;
    }
    filters.push(format!("atempo={factor:.12}"));
    Ok(filters.join(","))
}

fn timed_speech(
    c: &Config,
    voice: &Voice,
    segments: &[Segment],
    seconds: f64,
    work: &Path,
) -> Result<PathBuf> {
    let timeline_path = work.join("timeline.pcm");
    let mut timeline = fs::File::create(&timeline_path)?;
    // Mono 48 kHz signed 16-bit PCM. New file ranges are zero-filled silence.
    timeline.set_len((seconds * 48000.0).round() as u64 * 2)?;
    let text_path = work.join("segment.txt");
    let natural = work.join("segment.wav");
    let fitted = work.join("segment.pcm");
    for (i, segment) in segments.iter().enumerate() {
        println!(
            "Segment {}/{}: {:.3}s to {:.3}s",
            i + 1,
            segments.len(),
            segment.start_ms as f64 / 1000.0,
            segment.end_ms as f64 / 1000.0
        );
        if segment.text.trim().is_empty() {
            continue;
        }
        fs::write(&text_path, &segment.text)?;
        synthesize(c, voice, &text_path, &natural)
            .with_context(|| format!("Synthesizing segment {}", i + 1))?;
        let natural_seconds = duration(c, &natural, "a:0")?;
        let slot_ms = segment.end_ms - segment.start_ms;
        let filter = format!(
            "aresample=48000,{},apad,atrim=end_sample={},asetpts=PTS-STARTPTS",
            tempo_filter(natural_seconds / (slot_ms as f64 / 1000.0))?,
            slot_ms * 48
        );
        run(ffmpeg(c)
            .arg("-i")
            .arg(&natural)
            .args(["-af", &filter, "-ac", "1", "-ar", "48000", "-f", "s16le"])
            .arg(&fitted))?;
        ensure!(
            fs::metadata(&fitted)?.len() == slot_ms * 96,
            "Segment {} did not fit its time slot",
            i + 1
        );
        timeline.seek(SeekFrom::Start(segment.start_ms * 96))?;
        std::io::copy(&mut fs::File::open(&fitted)?, &mut timeline)?;
    }
    timeline.sync_all()?;
    Ok(timeline_path)
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let config_path = fs::canonicalize(&cli.config)
        .context("Copy config.example.json to config.json and edit local paths")?;
    let c: Config = serde_json::from_slice(&fs::read(&config_path)?)?;
    ensure!(c.threads > 0, "threads must be positive");
    // Resolve config paths against its directory, but keep CLI paths relative to caller.
    let caller = std::env::current_dir()?;
    let absolute = |p: &Path| {
        if p.is_absolute() {
            p.to_owned()
        } else {
            caller.join(p)
        }
    };
    std::env::set_current_dir(config_path.parent().context("Config directory missing")?)?;
    match cli.action {
        Action::Prepare {
            video,
            project,
            language,
        } => prepare(&c, &absolute(&video), &absolute(&project), &language),
        Action::Render {
            project,
            output,
            voice,
        } => {
            let v = c
                .voices
                .get(&voice)
                .with_context(|| format!("Unknown voice {voice}; use voices"))?;
            render(&c, &absolute(&project), &absolute(&output), v)
        }
        Action::Voices => {
            for name in c.voices.keys() {
                println!("Preset: {name}");
            }
            run(&mut ps("$ErrorActionPreference='Stop'; Add-Type -AssemblyName System.Speech; $s=New-Object System.Speech.Synthesis.SpeechSynthesizer; try { $s.GetInstalledVoices() | ForEach-Object { $_.VoiceInfo.Name } } finally { $s.Dispose() }"))
        }
        Action::Doctor { voice } => {
            run(Command::new(&c.ffmpeg).arg("-version"))?;
            run(Command::new(&c.ffprobe).arg("-version"))?;
            run(Command::new(&c.whisper).arg("--help"))?;
            file(&c.whisper_model)?;
            if let Some(name) = voice {
                let v = c.voices.get(&name).context("Unknown voice")?;
                let tmp = tempfile::tempdir()?;
                let text = tmp.path().join("test.txt");
                fs::write(&text, "This is an offline voice test.")?;
                let wav = tmp.path().join("test.wav");
                synthesize(&c, v, &text, &wav)?;
                duration(&c, &wav, "a:0")?;
            }
            println!("Local dependency checks passed.");
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn timestamps_and_multiline_cues_preserve_gaps() {
        let s = parse_srt("\u{feff}1\r\n00:00:01,250 --> 00:00:02,750\r\nHello\r\nworld\r\n\r\n2\r\n00:00:04,000 --> 00:00:05,000\r\nNext\r\n").unwrap();
        assert_eq!((s[0].start_ms, s[0].end_ms), (1250, 2750));
        assert_eq!(s[0].text, "Hello world");
        validate_timeline(&s, 6.0).unwrap();
        assert!(validate_timeline(&s, 4.0).is_err());
        assert_eq!(timestamp_ms("01:02:03,456").unwrap(), 3_723_456);
        assert!(timestamp_ms("00:60:00,000").is_err());
        assert!(timestamp_ms("00:00:00,1").is_err());
        assert!(parse_srt("Hello, no timestamps").is_err());
    }
    #[test]
    fn invalid_timeline_is_rejected() {
        for timing in [
            "00:00:02,000 --> 00:00:02,000",
            "00:00:03,000 --> 00:00:02,000",
            "00:00:01,500 --> 00:00:03,000",
        ] {
            let s = parse_srt(&format!(
                "1\n00:00:00,000 --> 00:00:02,000\nFirst\n\n2\n{timing}\nSecond\n"
            ))
            .unwrap();
            assert!(validate_timeline(&s, 10.0).is_err());
        }
    }
    #[test]
    fn tempo_factors_stay_in_pitch_preserving_range() {
        for ratio in [0.01, 0.5, 1.0, 2.0, 12.5] {
            let filter = tempo_filter(ratio).unwrap();
            let values: Vec<f64> = filter
                .split(',')
                .map(|v| v.strip_prefix("atempo=").unwrap().parse().unwrap())
                .collect();
            assert!(values.iter().all(|v| (0.5..=2.0).contains(v)));
            assert!((values.iter().product::<f64>() - ratio).abs() < 1e-9);
        }
        assert!(tempo_filter(0.0).is_err());
    }
    #[test]
    fn example_config_is_valid() {
        let c: Config = serde_json::from_str(include_str!("../config.example.json")).unwrap();
        assert!(matches!(c.voices["clone"], Voice::Zipvoice { .. }));
        assert!(matches!(c.voices["windows"], Voice::Sapi { .. }));
    }
}
