//! The project packet: one JSON file that names a run's inputs and carries its configuration.
//!
//! A packet gives the audio, the transcription, or both. Audio alone needs a transcriber
//! command, a transcription alone is filmed without a soundtrack and with a loudness curve
//! taken from the notes. Beside the inputs it names the directory of authored files (feel,
//! vocabulary, matrix, direction, signals) and may override any key of any of them with `set`.
//! A packet may `extends` another, so a run states only what differs from the shipped one.
//!
//! Paths in a packet are relative to the file that contains them. Later keys replace earlier
//! ones; objects are merged key by key; `null` clears a value, which is how a packet that
//! extends a two-input packet drops its audio.

use serde::Deserialize;
use serde_json::Value;
use std::{
    collections::BTreeMap,
    fs,
    path::{Component, Path, PathBuf},
};

/// The packet an unset `CINEMA_PACKET` selects.
pub const DEFAULT_PACKET: &str = "packets/default.json";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Section {
    Feel,
    Vocabulary,
    Matrix,
    Direction,
    Signals,
    Export,
}

impl Section {
    pub const ALL: [Section; 6] = [
        Section::Feel,
        Section::Vocabulary,
        Section::Matrix,
        Section::Direction,
        Section::Signals,
        Section::Export,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Section::Feel => "feel",
            Section::Vocabulary => "vocabulary",
            Section::Matrix => "matrix",
            Section::Direction => "direction",
            Section::Signals => "signals",
            Section::Export => "export",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Both,
    /// Needs the packet's transcriber to produce the notes.
    AudioOnly,
    /// No soundtrack; loudness comes from the notes.
    NotesOnly,
}

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct Inputs {
    audio: Option<String>,
    notes: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Doc {
    name: String,
    #[serde(default)]
    inputs: Inputs,
    #[serde(default)]
    transcriber: Option<Vec<String>>,
    vocab: String,
    media: String,
    out: String,
    #[serde(default)]
    set: BTreeMap<String, Value>,
}

#[derive(Clone, Debug)]
pub struct Packet {
    pub name: String,
    pub path: PathBuf,
    pub audio: Option<PathBuf>,
    pub notes: Option<PathBuf>,
    /// Command that writes the notes file from the audio file; `{audio}` and `{notes}` are
    /// replaced by the two paths.
    pub transcriber: Option<Vec<String>>,
    vocab: PathBuf,
    media: PathBuf,
    out: PathBuf,
    set: BTreeMap<String, Value>,
}

/// Merge `over` into `base`: objects key by key, anything else replaced.
pub fn merge(base: &mut Value, over: &Value) {
    match (base, over) {
        (Value::Object(b), Value::Object(o)) => {
            for (k, v) in o {
                match b.get_mut(k) {
                    Some(slot) => merge(slot, v),
                    None => {
                        b.insert(k.clone(), v.clone());
                    }
                }
            }
        }
        (slot, v) => *slot = v.clone(),
    }
}

/// The transcriber command with its two placeholders filled in.
pub fn expand(argv: &[String], audio: &Path, notes: &Path) -> Vec<String> {
    argv.iter()
        .map(|a| a.replace("{audio}", &audio.to_string_lossy()).replace("{notes}", &notes.to_string_lossy()))
        .collect()
}

fn resolve(dir: &Path, p: &str) -> String {
    let joined = if Path::new(p).is_absolute() { PathBuf::from(p) } else { dir.join(p) };
    let mut out = PathBuf::new();
    for c in joined.components() {
        match c {
            Component::ParentDir if out.file_name().is_some() => {
                out.pop();
            }
            c => out.push(c),
        }
    }
    out.to_string_lossy().into_owned()
}

fn load_value(path: &Path, stack: &mut Vec<PathBuf>) -> Result<Value, String> {
    let path = path.canonicalize().map_err(|e| format!("packet {}: {e}", path.display()))?;
    if stack.contains(&path) {
        return Err(format!("packet {}: extends itself", path.display()));
    }
    stack.push(path.clone());
    let text = fs::read_to_string(&path).map_err(|e| format!("packet {}: {e}", path.display()))?;
    let Value::Object(mut obj) = serde_json::from_str::<Value>(&text).map_err(|e| format!("packet {}: {e}", path.display()))?
    else {
        return Err(format!("packet {}: not a JSON object", path.display()));
    };
    let dir = path.parent().unwrap_or(Path::new("."));
    for key in ["vocab", "media", "out"] {
        if let Some(Value::String(s)) = obj.get_mut(key) {
            *s = resolve(dir, s);
        }
    }
    if let Some(Value::Object(inputs)) = obj.get_mut("inputs") {
        for key in ["audio", "notes"] {
            if let Some(Value::String(s)) = inputs.get_mut(key) {
                *s = resolve(dir, s);
            }
        }
    }
    let parent = obj.remove("extends");
    let me = Value::Object(obj);
    let merged = match parent {
        None => me,
        Some(Value::String(p)) => {
            let mut base = load_value(&dir.join(p), stack)?;
            merge(&mut base, &me);
            base
        }
        Some(_) => return Err(format!("packet {}: `extends` must be a path", path.display())),
    };
    stack.pop();
    Ok(merged)
}

impl Packet {
    pub fn load(path: impl AsRef<Path>) -> Result<Packet, String> {
        let path = path.as_ref();
        let value = load_value(path, &mut vec![])?;
        let doc: Doc = serde_json::from_value(value).map_err(|e| format!("packet {}: {e}", path.display()))?;
        let bad = |m: String| Err(format!("packet {}: {m}", path.display()));
        if doc.name.is_empty() || !doc.name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
            return bad(format!("name `{}` must be letters, digits, `-` and `_`", doc.name));
        }
        if doc.inputs.audio.is_none() && doc.inputs.notes.is_none() {
            return bad("needs `inputs.audio`, `inputs.notes` or both".into());
        }
        if doc.inputs.notes.is_none() && doc.transcriber.as_ref().is_none_or(|t| t.is_empty()) {
            return bad("audio without notes needs a `transcriber` command".into());
        }
        if let Some(unknown) = doc.set.keys().find(|k| !Section::ALL.iter().any(|s| s.name() == k.as_str())) {
            return bad(format!("`set.{unknown}` names no section ({})", Section::ALL.map(Section::name).join(", ")));
        }
        Ok(Packet {
            name: doc.name,
            path: path.to_path_buf(),
            audio: doc.inputs.audio.map(PathBuf::from),
            notes: doc.inputs.notes.map(PathBuf::from),
            transcriber: doc.transcriber,
            vocab: PathBuf::from(doc.vocab),
            media: PathBuf::from(doc.media),
            out: PathBuf::from(doc.out),
            set: doc.set,
        })
    }

    /// The packet named by `CINEMA_PACKET`, or [`DEFAULT_PACKET`].
    pub fn from_env() -> Result<Packet, String> {
        Packet::load(std::env::var("CINEMA_PACKET").unwrap_or_else(|_| DEFAULT_PACKET.into()))
    }

    pub fn mode(&self) -> Mode {
        match (&self.audio, &self.notes) {
            (Some(_), Some(_)) => Mode::Both,
            (Some(_), None) => Mode::AudioOnly,
            _ => Mode::NotesOnly,
        }
    }

    /// Folder holding the fonts.
    pub fn media(&self) -> &Path {
        &self.media
    }

    /// Everything this run writes goes under `<out>/<name>/`.
    pub fn run_dir(&self) -> PathBuf {
        self.out.join(&self.name)
    }

    pub fn notes_path(&self) -> PathBuf {
        self.notes.clone().unwrap_or_else(|| self.run_dir().join("notes.json"))
    }

    /// The export configuration with this packet's overrides, written here for the exporter.
    pub fn export_config_path(&self) -> PathBuf {
        self.run_dir().join("export.json")
    }

    pub fn analysis_path(&self) -> PathBuf {
        self.run_dir().join("analysis.json")
    }

    /// Holds what the lens pass reads: the flat video and the soundtrack.
    pub fn pass_dir(&self) -> PathBuf {
        self.run_dir().join("pass")
    }

    pub fn music_path(&self) -> PathBuf {
        self.pass_dir().join("music.wav")
    }

    pub fn flat_path(&self) -> PathBuf {
        self.pass_dir().join("flat.mp4")
    }

    pub fn video_path(&self) -> PathBuf {
        self.run_dir().join("cinema.mp4")
    }

    pub fn plan_path(&self) -> PathBuf {
        self.run_dir().join("plan.json")
    }

    /// The text of an authored file with this packet's overrides applied.
    pub fn section(&self, s: Section) -> Result<String, String> {
        let file = self.vocab.join(format!("{}.json", s.name()));
        let text = fs::read_to_string(&file).map_err(|e| format!("{}: {e}", file.display()))?;
        let mut value: Value = serde_json::from_str(&text).map_err(|e| format!("{}: {e}", file.display()))?;
        if let Some(over) = self.set.get(s.name()) {
            merge(&mut value, over);
        }
        Ok(value.to_string())
    }

    /// Top-level keys this packet overrides in a section.
    pub fn overrides(&self, s: Section) -> usize {
        self.set.get(s.name()).and_then(Value::as_object).map_or(0, |o| o.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Scratch(PathBuf);

    impl Scratch {
        fn new(tag: &str) -> Scratch {
            let d = std::env::temp_dir().join(format!("packet-{}-{tag}", std::process::id()));
            let _ = fs::remove_dir_all(&d);
            fs::create_dir_all(d.join("vocab")).unwrap();
            fs::write(d.join("vocab/direction.json"), r#"{"phrase_bars":2,"fit":0.9,"nest":{"a":1,"b":2}}"#).unwrap();
            Scratch(d)
        }

        fn write(&self, name: &str, text: &str) -> PathBuf {
            let p = self.0.join(name);
            fs::write(&p, text).unwrap();
            p
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    const BASE: &str = r#"{"name":"base","inputs":{"audio":"a.wav","notes":"n.json"},"vocab":"vocab","media":"media","out":"out",
        "set":{"direction":{"phrase_bars":4,"nest":{"a":10}}}}"#;

    #[test]
    fn paths_resolve_against_the_file_and_the_mode_follows_the_inputs() {
        let s = Scratch::new("paths");
        let p = Packet::load(s.write("base.json", BASE)).unwrap();
        assert_eq!(p.mode(), Mode::Both);
        let root = s.0.canonicalize().unwrap();
        assert_eq!(p.audio.as_deref(), Some(root.join("a.wav").as_path()));
        assert_eq!(p.analysis_path(), root.join("out/base/analysis.json"));
        assert_eq!(p.notes_path(), root.join("n.json"));
    }

    #[test]
    fn a_child_changes_only_what_it_states() {
        let s = Scratch::new("extends");
        s.write("base.json", BASE);
        let child = s.write(
            "calm.json",
            r#"{"extends":"base.json","name":"calm","inputs":{"audio":null},"set":{"direction":{"nest":{"b":20}}}}"#,
        );
        let p = Packet::load(child).unwrap();
        assert_eq!(p.name, "calm");
        assert_eq!(p.mode(), Mode::NotesOnly);
        assert_eq!(p.notes_path(), s.0.canonicalize().unwrap().join("n.json"));
        let d: Value = serde_json::from_str(&p.section(Section::Direction).unwrap()).unwrap();
        assert_eq!(d["phrase_bars"], 4, "inherited override");
        assert_eq!(d["fit"], 0.9, "untouched file value");
        assert_eq!((d["nest"]["a"].clone(), d["nest"]["b"].clone()), (10.into(), 20.into()));
    }

    #[test]
    fn an_audio_only_packet_needs_a_transcriber() {
        let s = Scratch::new("audio");
        let bare = r#"{"name":"x","inputs":{"audio":"a.wav"},"vocab":"vocab","media":"media","out":"out"}"#;
        assert!(Packet::load(s.write("bare.json", bare)).unwrap_err().contains("transcriber"));
        let with = r#"{"name":"x","inputs":{"audio":"a.wav"},"transcriber":["t","{audio}","{notes}"],"vocab":"vocab","media":"media","out":"out"}"#;
        let p = Packet::load(s.write("with.json", with)).unwrap();
        assert_eq!(p.mode(), Mode::AudioOnly);
        assert!(p.notes_path().ends_with("out/x/notes.json"));
        let argv = expand(p.transcriber.as_ref().unwrap(), p.audio.as_ref().unwrap(), &p.notes_path());
        assert_eq!(argv[0], "t");
        assert!(argv[1].ends_with("a.wav") && argv[2].ends_with("out/x/notes.json"));
    }

    #[test]
    fn a_packet_overrides_one_chrome_key_and_keeps_the_others() {
        let s = Scratch::new("chrome");
        let chrome = r#"{"phrase_bars":2,"chrome":{"ruler":true,"fft_bands":48,"fft_lo_hz":40}}"#;
        fs::write(s.0.join("vocab/direction.json"), chrome).unwrap();
        s.write(
            "quiet.json",
            r#"{"name":"quiet","inputs":{"notes":"n.json"},"vocab":"vocab","media":"media","out":"out","set":{"direction":{"chrome":{"fft_bands":12}}}}"#,
        );
        let p = Packet::load(s.0.join("quiet.json")).unwrap();
        let d: Value = serde_json::from_str(&p.section(Section::Direction).unwrap()).unwrap();
        assert_eq!(d["chrome"]["fft_bands"], 12, "overridden");
        assert_eq!(d["chrome"]["fft_lo_hz"], 40, "untouched sibling");
        assert_eq!(d["chrome"]["ruler"], true, "untouched sibling");
    }

    #[test]
    fn mistakes_are_refused() {
        let s = Scratch::new("bad");
        for (what, text) in [
            ("no inputs", r#"{"name":"x","vocab":"vocab","media":"media","out":"out"}"#),
            ("name", r#"{"name":"a b","inputs":{"notes":"n"},"vocab":"vocab","media":"media","out":"out"}"#),
            ("unknown key", r#"{"name":"x","inputs":{"notes":"n"},"vocab":"vocab","media":"media","out":"out","colour":1}"#),
            ("unknown input", r#"{"name":"x","inputs":{"notes":"n","video":"v"},"vocab":"vocab","media":"media","out":"out"}"#),
            ("unknown section", r#"{"name":"x","inputs":{"notes":"n"},"vocab":"vocab","media":"media","out":"out","set":{"sound":{}}}"#),
        ] {
            assert!(Packet::load(s.write("bad.json", text)).is_err(), "{what}");
        }
        s.write("loop_a.json", r#"{"extends":"loop_b.json"}"#);
        let b = s.write("loop_b.json", r#"{"extends":"loop_a.json"}"#);
        assert!(Packet::load(b).unwrap_err().contains("itself"));
    }
}
