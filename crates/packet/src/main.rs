//! `packet path <packet.json> <run|pass|analysis|plan|flat|video>` prints one output path;
//! `packet show <packet.json>` prints what a packet resolves to; `packet prepare <packet.json>`
//! turns its inputs into `<out>/<name>/analysis.json` (running the packet's transcriber first
//! when only audio is given).

use packet::{Mode, Packet, Section, expand};
use std::{env, fs, path::Path, process::{Command, ExitCode}};

fn run(cmd: &mut Command, what: &str) -> Result<(), String> {
    let status = cmd.status().map_err(|e| format!("{what}: {e}"))?;
    if status.success() { Ok(()) } else { Err(format!("{what} exited with {status}")) }
}

fn prepare(p: &Packet) -> Result<(), String> {
    let io = |e: std::io::Error| e.to_string();
    fs::create_dir_all(p.pass_dir()).map_err(io)?; // also creates the run directory
    let notes = p.notes_path();
    if p.mode() == Mode::AudioOnly {
        let audio = p.audio.as_ref().expect("audio-only packet has audio");
        let argv = expand(p.transcriber.as_ref().expect("validated by Packet::load"), audio, &notes);
        run(Command::new(&argv[0]).args(&argv[1..]), "transcriber")?;
        if !notes.exists() {
            return Err(format!("the transcriber ran but wrote no {}", notes.display()));
        }
    }
    let python = env::var("CINEMA_PYTHON").unwrap_or_else(|_| "python3".into());
    let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../python/export_analysis.py");
    let mut cmd = Command::new(&python);
    fs::write(p.export_config_path(), p.section(Section::Export)?).map_err(io)?;
    cmd.arg(script).arg("--notes").arg(&notes).arg("--out").arg(p.analysis_path()).arg("--config").arg(p.export_config_path());
    if let Some(audio) = &p.audio {
        cmd.arg("--audio").arg(audio);
    }
    run(&mut cmd, "export_analysis.py")?;
    detect_anchors(p, &python)?;
    match &p.audio {
        Some(audio) if audio.extension().is_some_and(|e| e.eq_ignore_ascii_case("wav")) => drop(fs::copy(audio, p.music_path()).map_err(io)?),
        // Any other container is decoded to PCM so the soundtrack file is a real wav.
        Some(audio) => run(
            Command::new("ffmpeg").args(["-v", "error", "-y", "-i"]).arg(audio).args(["-vn", "-c:a", "pcm_s16le"]).arg(p.music_path()),
            "ffmpeg",
        )?,
        None => {
            let _ = fs::remove_file(p.music_path());
        }
    }
    Ok(())
}

/// Find motifs in the images the packet's plates use and write the merged anchors for the planner.
/// Images whose file and configuration are unchanged are not detected again.
fn detect_anchors(p: &Packet, python: &str) -> Result<(), String> {
    let io = |e: std::io::Error| e.to_string();
    fs::write(p.anchors_config_path(), p.section(Section::Anchors)?).map_err(io)?;
    fs::write(p.direction_path(), p.section(Section::Direction)?).map_err(io)?;
    let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tools/anchors.py");
    run(
        Command::new(python)
            .arg(script)
            .arg("prepare")
            .arg("--config")
            .arg(p.anchors_config_path())
            .arg("--direction")
            .arg(p.direction_path())
            .arg("--media")
            .arg(p.media())
            .arg("--dir")
            .arg(p.anchors_dir())
            .arg("--out")
            .arg(p.anchors_path()),
        "anchors.py prepare",
    )
}

fn show(p: &Packet) -> Result<(), String> {
    println!("packet   {} ({})", p.name, p.path.display());
    println!("mode     {:?}", p.mode());
    println!("audio    {}", p.audio.as_ref().map_or("-".into(), |a| a.display().to_string()));
    println!("notes    {}", p.notes_path().display());
    println!("run dir  {}", p.run_dir().display());
    println!("anchors  {}", p.anchors_dir().display());
    for s in Section::ALL {
        let text = p.section(s)?;
        println!("{:<10} {} bytes, {} overridden keys", s.name(), text.len(), p.overrides(s));
    }
    Ok(())
}

fn locate(p: &Packet, key: &str) -> Result<(), String> {
    let path = match key {
        "run" => p.run_dir(),
        "pass" => p.pass_dir(),
        "analysis" => p.analysis_path(),
        "plan" => p.plan_path(),
        "flat" => p.flat_path(),
        "video" => p.video_path(),
        _ => return Err(format!("no path `{key}` (run, pass, analysis, plan, flat, video)")),
    };
    println!("{}", path.display());
    Ok(())
}

fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();
    let result = match args.as_slice() {
        [cmd, path] if cmd == "prepare" => Packet::load(path).and_then(|p| prepare(&p)),
        [cmd, path] if cmd == "show" => Packet::load(path).and_then(|p| show(&p)),
        [cmd, path, key] if cmd == "path" => Packet::load(path).and_then(|p| locate(&p, key)),
        _ => Err("usage: packet <prepare|show> <packet.json>".into()),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("packet: {e}");
            ExitCode::FAILURE
        }
    }
}
