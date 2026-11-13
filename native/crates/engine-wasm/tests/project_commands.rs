//! P0 gate: the Rust-owned project document.
//!
//! Proves the command protocol on the real wasm-facing API:
//!   load .ustx → edit via commands → undo/redo → render
//! and that a command-edited project renders **bit-identically** to the same
//! edit applied to the YAML text directly. That equivalence is what lets the
//! UI edit through commands while the render path stays unchanged.

use std::path::{Path, PathBuf};

use engine_wasm::Engine;

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..")
}

fn demo_song() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tools/synth-cli/tests/data/demo-song.ustx")
}

fn teto_library() -> PathBuf {
    repo_root().join("test/golden/teto-english/library")
}

fn read(path: &Path) -> Vec<u8> {
    std::fs::read(path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

fn install_teto(engine: &mut Engine, library: &Path) {
    engine.set_voicebank(
        "teto-english",
        "Teto",
        "voice",
        &read(&library.join("voice/oto.ini")),
    );
    for key in engine.wav_keys().lines() {
        let path = library.join(key);
        let wav = read(&path);
        let frq_path = voicebank::frq_path_for_wav(&path);
        match std::fs::read(&frq_path) {
            Ok(frq) => {
                engine
                    .add_sample_with_frq(key, &wav, &frq)
                    .expect("add_sample_with_frq");
            }
            Err(_) => engine.add_sample(key, &wav).expect("add_sample"),
        }
    }
}

#[test]
fn session_render_sample_keys_are_sorted_and_revision_fenced() {
    let library = teto_library();
    if !library.is_dir() {
        eprintln!("skip: {} not present", library.display());
        return;
    }
    let mut engine = Engine::new();
    engine.set_voicebank(
        "teto-english",
        "Teto",
        "voice",
        &read(&library.join("voice/oto.ini")),
    );
    let opened: serde_json::Value = serde_json::from_str(
        &engine
            .session_open_yaml(
                include_str!("../../../tools/synth-cli/tests/data/demo-song.ustx"),
                "",
            )
            .unwrap(),
    )
    .unwrap();
    let planned: serde_json::Value = serde_json::from_str(
        &engine
            .session_render_sample_keys_json("[0]")
            .expect("sample plan"),
    )
    .unwrap();
    let keys = planned["sampleKeys"].as_array().expect("sample key list");
    assert!(
        !keys.is_empty(),
        "the demo voice track should need mapped sample WAVs"
    );
    assert_eq!(planned["sessionId"], engine.session_id());
    assert_eq!(planned["revision"], opened["revision"]);
    let values: Vec<_> = keys.iter().map(|key| key.as_str().unwrap()).collect();
    let mut sorted = values.clone();
    sorted.sort_unstable();
    sorted.dedup();
    assert_eq!(values, sorted, "sample keys must be sorted and unique");
    assert_eq!(
        engine.session_revision(),
        opened["revision"].as_u64().unwrap()
    );
}

/// `project_load_yaml` must round-trip without losing anything the renderer
/// needs: loading then re-serializing must render the same audio.
#[test]
fn project_load_round_trips_and_renders_unchanged() {
    let library = teto_library();
    if !library.is_dir() {
        eprintln!("skip: {} not present", library.display());
        return;
    }
    let mut engine = Engine::new();
    install_teto(&mut engine, &library);

    let original = String::from_utf8(read(&demo_song())).expect("utf8 project");
    let normalized = engine.project_load_yaml(&original).expect("load");
    let yaml = engine.project_yaml();
    assert_eq!(normalized, yaml, "load did not return the current YAML");

    let a = engine
        .render_project(&original, 0)
        .expect("render from file text");
    let b = engine
        .render_project(&yaml, 0)
        .expect("render from round-tripped text");
    assert_eq!(a.len(), b.len(), "round-trip changed the render length");
    let max_diff = a
        .iter()
        .zip(&b)
        .fold(0.0f32, |m, (x, y)| m.max((x - y).abs()));
    assert_eq!(
        max_diff, 0.0,
        "round-trip changed the audio: {max_diff:.3e}"
    );
    println!("round-trip: {} samples, identical", a.len());
}

/// View-model accessors agree with the project they describe.
#[test]
fn view_models_describe_the_project() {
    let mut engine = Engine::new();
    let yaml = String::from_utf8(read(&demo_song())).expect("utf8 project");
    engine.project_load_yaml(&yaml).expect("load");

    let meta: serde_json::Value =
        serde_json::from_str(&engine.meta_json().expect("meta")).expect("meta json");
    assert_eq!(meta["bpm"], 120);
    assert_eq!(meta["beat_per_bar"], 4);
    assert_eq!(meta["resolution"], 480);
    assert!(meta["track_count"].as_u64().unwrap() >= 1);

    let tracks: serde_json::Value =
        serde_json::from_str(&engine.tracks_json().expect("tracks")).expect("tracks json");
    assert_eq!(
        tracks.as_array().unwrap().len(),
        1,
        "demo song has one track"
    );

    let notes: serde_json::Value =
        serde_json::from_str(&engine.notes_json(0).expect("notes")).expect("notes json");
    let parts = notes["parts"].as_array().expect("parts");
    assert_eq!(parts.len(), 1);
    let note_list = parts[0]["notes"].as_array().expect("notes");
    assert_eq!(note_list.len(), 4, "demo song has 4 notes");
    // Notes are reported in project order.
    let positions: Vec<i64> = note_list
        .iter()
        .map(|n| n["position"].as_i64().unwrap())
        .collect();
    let mut sorted = positions.clone();
    sorted.sort_unstable();
    assert_eq!(positions, sorted, "notes are not in tick order");

    let curves = engine.curves_json(0).expect("curves");
    assert!(curves.contains("\"parts\""), "curves json shape: {curves}");
}

/// An edit through a command must produce exactly the same audio as the same
/// edit written into the YAML by hand.
#[test]
fn command_edit_renders_like_yaml_edit() {
    let library = teto_library();
    if !library.is_dir() {
        eprintln!("skip: {} not present", library.display());
        return;
    }
    let mut engine = Engine::new();
    install_teto(&mut engine, &library);

    let original = String::from_utf8(read(&demo_song())).expect("utf8 project");
    engine.project_load_yaml(&original).expect("load");

    // --- edit through the command protocol ---
    let name = engine
        .apply_command(r#"{"cmd":"change_tone","part":0,"note":2,"tone":67}"#)
        .expect("change_tone");
    assert_eq!(name, "Change tone");
    let via_command = engine.project_yaml();

    // --- the same edit applied to the YAML text ---
    // The 3rd note's tone is 62 in demo-song (notes are ordered by position).
    let via_yaml = {
        let mut nth = 0;
        let mut out = String::new();
        for line in original.lines() {
            let trimmed = line.trim_start();
            if trimmed.starts_with("tone:") {
                if nth == 2 {
                    let indent = &line[..line.len() - trimmed.len()];
                    out.push_str(&format!("{indent}tone: 67\n"));
                    nth += 1;
                    continue;
                }
                nth += 1;
            }
            out.push_str(line);
            out.push('\n');
        }
        out
    };

    let a = engine
        .render_project(&via_command, 0)
        .expect("render via command");
    let b = engine
        .render_project(&via_yaml, 0)
        .expect("render via yaml");
    assert_eq!(a.len(), b.len(), "command vs yaml render length differs");
    let max_diff = a
        .iter()
        .zip(&b)
        .fold(0.0f32, |m, (x, y)| m.max((x - y).abs()));
    assert_eq!(
        max_diff, 0.0,
        "command edit and yaml edit produced different audio: {max_diff:.3e}"
    );
    println!("command vs yaml edit: {} samples, identical", a.len());
}

/// Undo restores the exact prior state, and redo re-applies it.
#[test]
fn undo_and_redo_restore_the_document() {
    let mut engine = Engine::new();
    let original = String::from_utf8(read(&demo_song())).expect("utf8 project");
    engine.project_load_yaml(&original).expect("load");
    let before = engine.project_yaml();
    assert!(!engine.can_undo(), "fresh load must have nothing to undo");

    engine
        .apply_command(r#"{"cmd":"move_note","part":0,"note":0,"delta_pos":120,"delta_tone":1}"#)
        .expect("move_note");
    let moved = engine.project_yaml();
    assert_ne!(moved, before, "move_note changed nothing");
    assert!(engine.can_undo());

    engine.undo().expect("undo");
    assert_eq!(
        engine.project_yaml(),
        before,
        "undo did not restore the state"
    );
    assert!(engine.can_redo(), "redo should be available after undo");

    engine.redo().expect("redo");
    assert_eq!(
        engine.project_yaml(),
        moved,
        "redo did not re-apply the edit"
    );
}

/// A rejected command must leave the project untouched and out of the undo
/// history (the `commands` contract, exercised through the wasm surface).
#[test]
fn invalid_command_leaves_the_project_untouched() {
    let mut engine = Engine::new();
    let original = String::from_utf8(read(&demo_song())).expect("utf8 project");
    engine.project_load_yaml(&original).expect("load");
    let before = engine.project_yaml();

    for bad in [
        r#"{"cmd":"nope"}"#,
        r#"{"cmd":"change_tone","part":0,"note":99,"tone":60}"#,
        r#"{"cmd":"change_tone","part":0}"#,
        r#"{"note":3}"#,
        "not json at all",
    ] {
        let result = engine.apply_command(bad);
        assert!(result.is_err(), "should have been rejected: {bad}");
        assert_eq!(
            engine.project_yaml(),
            before,
            "project changed after a rejected command: {bad}"
        );
        assert!(
            !engine.can_undo(),
            "rejected command entered the undo stack: {bad}"
        );
    }
}

/// Grouping batches several edits into one undo step.
#[test]
fn group_edits_into_one_undo_step() {
    let mut engine = Engine::new();
    let original = String::from_utf8(read(&demo_song())).expect("utf8 project");
    engine.project_load_yaml(&original).expect("load");
    let before = engine.project_yaml();

    engine
        .apply_command(r#"{"cmd":"group_begin","name":"Move notes"}"#)
        .expect("group_begin");
    engine
        .apply_command(r#"{"cmd":"change_tone","part":0,"note":0,"tone":60}"#)
        .expect("edit 1");
    engine
        .apply_command(r#"{"cmd":"change_tone","part":0,"note":1,"tone":61}"#)
        .expect("edit 2");
    engine
        .apply_command(r#"{"cmd":"group_end"}"#)
        .expect("group_end");

    let grouped = engine.project_yaml();
    assert_ne!(grouped, before);
    engine.undo().expect("undo");
    assert_eq!(
        engine.project_yaml(),
        before,
        "one undo should revert the whole group"
    );
}

/// Multi-track volume/pan/mute/solo go through commands too.
#[test]
fn track_commands_update_the_track_list() {
    let mut engine = Engine::new();
    let original = String::from_utf8(read(&demo_song())).expect("utf8 project");
    engine.project_load_yaml(&original).expect("load");

    engine
        .apply_command(r#"{"cmd":"set_track_volume","track":0,"volume":-6.0}"#)
        .expect("volume");
    engine
        .apply_command(r#"{"cmd":"set_track_pan","track":0,"pan":0.25}"#)
        .expect("pan");
    engine
        .apply_command(r#"{"cmd":"set_track_mute","track":0,"mute":true}"#)
        .expect("mute");
    engine
        .apply_command(r#"{"cmd":"set_track_name","track":0,"name":"Lead"}"#)
        .expect("name");

    let tracks: serde_json::Value =
        serde_json::from_str(&engine.tracks_json().expect("tracks")).expect("tracks json");
    let t = &tracks[0];
    assert_eq!(t["volume"].as_f64().unwrap(), -6.0);
    assert_eq!(t["pan"].as_f64().unwrap(), 0.25);
    assert_eq!(t["mute"].as_bool().unwrap(), true);
    assert_eq!(t["name"].as_str().unwrap(), "Lead");
}

/// The document drives planning/rendering without YAML crossing the boundary.
#[test]
fn plan_from_document_matches_plan_from_yaml() {
    let library = teto_library();
    if !library.is_dir() {
        eprintln!("skip: {} not present", library.display());
        return;
    }
    let mut engine = Engine::new();
    install_teto(&mut engine, &library);
    let yaml = String::from_utf8(read(&demo_song())).expect("utf8 project");
    engine.project_load_yaml(&yaml).expect("load");

    let a = engine.plan_document(0).expect("plan_document");
    let b = engine.plan_project(&yaml, 0).expect("plan_project");
    assert_eq!(a, b, "planning from the document differs from the YAML");
    assert!(a >= 1, "expected at least one chunk");
}
