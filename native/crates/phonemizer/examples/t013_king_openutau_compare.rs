//! Compare the Rust Japanese VCV implementation to OpenUtau's Japanese Presamp
//! output on the ignored local `test-ustx-example/king.ustx` project.
//!
//! Extract only the bank's character metadata and oto.ini files first, then:
//! `cargo run -p phonemizer --example t013_king_openutau_compare`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use domain::UNote;
use phonemizer::{JapaneseVcvPhonemizer, Phonemizer};
use voicebank::{parse_character_yaml, parse_oto_ini, Subbank, Voicebank};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let repo = repo_root();
    let bank_path = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from("/tmp/t013-teto-bank/重音テト OU用日本語統合ライブラリー")
        });
    let openutau_report = std::env::args()
        .nth(2)
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            repo.join("poc/openutau-browser-wasm/artifacts/king-openutau-presamp.json")
        });

    let report: serde_json::Value = serde_json::from_slice(&std::fs::read(&openutau_report)?)
        .map_err(|error| {
            format!(
                "could not parse {} as JSON report: {error}",
                openutau_report.display()
            )
        })?;
    let project_name = report["Project"].as_str().unwrap_or("<unknown project>");
    let singer = load_singer(&bank_path)?;

    let mut total_expected = 0;
    let mut total_actual = 0;
    let mut total_exact = 0;
    for expected_part in report["Parts"]
        .as_array()
        .ok_or("OpenUtau report has no Parts list")?
    {
        let part_name = expected_part["Name"]
            .as_str()
            .ok_or("OpenUtau report contains a part without a name")?;
        let input = expected_part["Input"]
            .as_array()
            .ok_or_else(|| format!("OpenUtau report part {part_name:?} has no Input list"))?;
        let notes = input
            .iter()
            .map(|note| {
                Ok(UNote {
                    lyric: note["Lyric"]
                        .as_str()
                        .ok_or("note has no lyric")?
                        .to_string(),
                    tone: note["Tone"].as_i64().ok_or("note has no tone")? as i32,
                    position: note["Position"].as_i64().ok_or("note has no position")? as i32,
                    duration: note["Duration"].as_i64().ok_or("note has no duration")? as i32,
                    ..Default::default()
                })
            })
            .collect::<Result<Vec<_>, Box<dyn std::error::Error>>>()?;
        let expected_aliases = expected_part["Output"]
            .as_array()
            .ok_or("OpenUtau report part has no Output list")?
            .iter()
            .map(|phone| {
                phone["Alias"]
                    .as_str()
                    .map(str::to_string)
                    .ok_or("OpenUtau report contains a phone without an Alias")
            })
            .collect::<Result<Vec<_>, _>>()?;

        let continuations = notes
            .iter()
            .filter(|note| note.lyric.starts_with('+'))
            .count();
        if continuations != 0 {
            return Err(format!(
                "part {:?} has {continuations} tied continuation notes; this comparison runner expects untied note groups",
                part_name
            )
            .into());
        }

        let mut rust_aliases = Vec::new();
        let mut start = 0;
        while start < notes.len() {
            let mut end = start + 1;
            while end < notes.len()
                && notes[end - 1].position + notes[end - 1].duration >= notes[end].position
            {
                end += 1;
            }
            let phones = JapaneseVcvPhonemizer.process(&notes[start..end], Some(&singer));
            rust_aliases.extend(phones.into_iter().map(|phone| {
                singer
                    .lookup(&phone.phoneme, phone.tone)
                    .map(|oto| oto.alias.clone())
                    .unwrap_or(phone.phoneme)
            }));
            start = end;
        }

        let exact = expected_aliases
            .iter()
            .zip(&rust_aliases)
            .filter(|(expected, actual)| expected == actual)
            .count();
        let mismatches = expected_aliases
            .iter()
            .zip(&rust_aliases)
            .enumerate()
            .filter(|(_, (expected, actual))| expected != actual)
            .take(5)
            .map(|(i, (expected, actual))| {
                let note = notes.get(i);
                let lyric = note.map(|note| note.lyric.as_str()).unwrap_or("<no corresponding note>");
                let position = note.map(|note| note.position).unwrap_or_default();
                let previous = i.checked_sub(1).and_then(|index| notes.get(index));
                let previous = previous
                    .map(|note| format!("{:?}@{}", note.lyric, note.position))
                    .unwrap_or_else(|| "<phrase start>".into());
                format!(
                    "#{i} {lyric:?}@{position}, previous={previous}: OU=[{expected}] Rust=[{actual}]"
                )
            })
            .collect::<Vec<_>>();
        let count = expected_aliases.len().max(rust_aliases.len());
        println!(
            "{}: OpenUtau Presamp={} phones; Rust JapaneseVCV={} phones; exact aliases {}/{}",
            part_name,
            expected_aliases.len(),
            rust_aliases.len(),
            exact,
            count,
        );
        for mismatch in mismatches {
            println!("  {mismatch}");
        }
        total_expected += expected_aliases.len();
        total_actual += rust_aliases.len();
        total_exact += exact;
    }
    println!(
        "TOTAL: OpenUtau={total_expected}; Rust={total_actual}; exact alias matches={total_exact}/{}",
        total_expected.max(total_actual),
    );
    println!("Input project: {project_name}");
    println!("OpenUtau note/phone report: {}", openutau_report.display());
    println!("Singer metadata: {}", bank_path.display());
    Ok(())
}

fn load_singer(path: &Path) -> Result<Voicebank, Box<dyn std::error::Error>> {
    let config_bytes = std::fs::read(path.join("character.yaml"))?;
    let config = parse_character_yaml(
        config_bytes
            .strip_prefix(&[0xEF, 0xBB, 0xBF])
            .unwrap_or(&config_bytes),
    )?;
    let mut oto_paths = Vec::new();
    collect_oto_paths(path, &mut oto_paths)?;
    oto_paths.sort();
    let mut oto_bytes = Vec::new();
    for oto_path in oto_paths {
        oto_bytes.extend(std::fs::read(oto_path)?);
        oto_bytes.push(b'\n');
    }

    let parsed = parse_oto_ini(&oto_bytes, config.text_file_encoding.as_deref());
    let mut singer = Voicebank::from_oto_bytes(
        "local-teto",
        config.name.as_deref().unwrap_or("Teto"),
        path.parent().unwrap_or(path),
        path,
        &[],
    );
    singer.oto_map = parsed
        .otos
        .iter()
        .cloned()
        .map(|oto| (oto.alias.clone(), oto))
        .collect::<HashMap<_, _>>();
    singer.otos = parsed.otos.clone();
    if let Some(set) = singer.oto_sets.first_mut() {
        set.otos = parsed.otos;
    }
    singer.subbanks = config
        .subbanks
        .into_iter()
        .map(|subbank| {
            Subbank::from_ranges(
                subbank.color.unwrap_or_default(),
                subbank.prefix.unwrap_or_default(),
                subbank.suffix.unwrap_or_default(),
                subbank.tone_ranges,
            )
        })
        .collect();
    Ok(singer)
}

fn collect_oto_paths(dir: &Path, out: &mut Vec<PathBuf>) -> std::io::Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        if path.is_dir() {
            collect_oto_paths(&path, out)?;
        } else if path.file_name().is_some_and(|name| name == "oto.ini") {
            out.push(path);
        }
    }
    Ok(())
}

fn repo_root() -> PathBuf {
    let mut current = std::env::current_dir().expect("current directory");
    loop {
        if current.join("native").is_dir() && current.join("ref(openutau+openutau mobile)").is_dir()
        {
            return current;
        }
        assert!(current.pop(), "could not find repository root");
    }
}
