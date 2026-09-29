use project_session::ProjectDoc;

const PROJECT: &str = include_str!("../../../tools/synth-cli/tests/data/demo-song.ustx");

#[test]
fn load_command_snapshot_and_undo_round_trip() {
    let mut doc = ProjectDoc::from_yaml(PROJECT).expect("load project");
    let before: serde_json::Value = serde_json::from_str(&doc.notes_json(0)).unwrap();
    assert_eq!(before["parts"][0]["notes"][2]["tone"], 64);

    doc.apply(r#"{"cmd":"change_tone","part":0,"note":2,"tone":67}"#)
        .unwrap();
    let after: serde_json::Value = serde_json::from_str(&doc.notes_json(0)).unwrap();
    assert_eq!(after["parts"][0]["notes"][2]["tone"], 67);

    let reopened = ProjectDoc::from_yaml(&doc.yaml()).unwrap();
    let reopened_notes: serde_json::Value = serde_json::from_str(&reopened.notes_json(0)).unwrap();
    assert_eq!(reopened_notes["parts"][0]["notes"][2]["tone"], 67);

    doc.undo().unwrap();
    let undone: serde_json::Value = serde_json::from_str(&doc.notes_json(0)).unwrap();
    assert_eq!(undone["parts"][0]["notes"][2]["tone"], 64);
}

#[test]
fn rejected_command_keeps_project_and_snapshot_unchanged() {
    let mut doc = ProjectDoc::from_yaml(PROJECT).unwrap();
    let yaml = doc.yaml();
    let snapshot = doc.snapshot().to_string();
    let undo_len = doc.undo_len();
    assert!(doc.apply(r#"{"cmd":"does_not_exist"}"#).is_err());
    assert_eq!(doc.yaml(), yaml);
    assert_eq!(doc.snapshot(), snapshot);
    assert_eq!(doc.undo_len(), undo_len);
}

#[test]
fn grouped_commands_undo_as_one_step() {
    let mut doc = ProjectDoc::from_yaml(PROJECT).unwrap();
    doc.apply(r#"{"cmd":"group_begin","name":"Edit note"}"#)
        .unwrap();
    doc.apply(r#"{"cmd":"change_tone","part":0,"note":2,"tone":65}"#)
        .unwrap();
    doc.apply(r#"{"cmd":"change_lyric","part":0,"note":2,"lyric":"ka"}"#)
        .unwrap();
    doc.apply(r#"{"cmd":"group_end"}"#).unwrap();
    assert!(doc.can_undo());
    doc.undo().unwrap();
    let notes: serde_json::Value = serde_json::from_str(&doc.notes_json(0)).unwrap();
    assert_eq!(notes["parts"][0]["notes"][2]["tone"], 64);
    assert_eq!(notes["parts"][0]["notes"][2]["lyric"], "ah[3]");
}

#[test]
fn cancel_group_rolls_back_prior_commands_after_failure() {
    let mut doc = ProjectDoc::from_yaml(PROJECT).unwrap();
    let before = doc.yaml();
    doc.apply(r#"{"cmd":"group_begin","name":"Drag"}"#).unwrap();
    doc.apply(r#"{"cmd":"change_tone","part":0,"note":2,"tone":70}"#)
        .unwrap();
    assert!(doc
        .apply(r#"{"cmd":"change_lyric","part":99,"note":0,"lyric":"bad"}"#)
        .is_err());
    doc.apply(r#"{"cmd":"group_cancel"}"#).unwrap();
    assert_eq!(doc.yaml(), before);
    assert!(!doc.can_undo());
}
