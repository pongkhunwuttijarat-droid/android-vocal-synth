//! A stateful project session: authoritative document + identity + revision.
//!
//! This is the shared logical session described in `docs/CONTRACT-v1.md` §6.
//! It is transport-neutral: the native server, the WASM facade and future JNI
//! adapters all drive this type so revision, identity and staleness rules
//! cannot diverge between hosts.
//!
//! Implemented semantics include stable IDs and sidecars, revisioned snapshots,
//! stable-ID mutations, isolated v2 groups, undo/redo, change results, bounded
//! reads and fail-closed validation. The legacy WASM facade remains available
//! through a compatibility bridge; its immediate-apply group behavior is kept
//! distinct from v2 transactions. Command families omitted from capabilities
//! remain unsupported until they have an explicit contract and implementation.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::identity::{
    self, table_from_json, table_to_json, EntityId, IdGenerator, IdentityTable, IdentityWarning,
};
use crate::v2::{
    self, Change, MutationResult, SessionError, ERR_GROUP_FAILED, ERR_INVALID_GROUP_STATE,
    ERR_SESSION_BUSY,
};
use crate::views;
use crate::ProjectDoc;
use domain::{UPart, UProject};
use phonemizer::{CompositionRequest, PhonemeComposer};
use voicebank::Voicebank;

/// Outcome of a committed state-changing operation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MutationOutcome {
    /// Authoritative revision after the operation.
    pub revision: u64,
    /// Undo-step name reported by the engine ("" for a nameless step).
    pub command_name: String,
    /// `false` only for a no-op (nothing to undo/redo); such a call does not
    /// change the revision either.
    pub changed: bool,
    pub can_undo: bool,
    pub can_redo: bool,
}

/// A project session: the authoritative document plus its identity table.
pub struct Session {
    doc: ProjectDoc,
    ids: IdentityTable,
    gen: IdGenerator,
    revision: u64,
    session_id: String,
    /// Whether a usable sidecar was adopted when the session opened. `false`
    /// means every entity received a fresh ID (§6.2.4).
    ids_stable: bool,
    warnings: Vec<IdentityWarning>,
    /// The open group transaction, if any (§6.8). At most one per session.
    group: Option<ActiveGroup>,
    /// Open group state for the compatibility command facade. CONTRACT-v1
    /// explicitly preserves its immediate-apply/undo-stack behavior (§6.8.3).
    legacy_group_open: bool,
    legacy_group_changed: bool,
}

/// An open group transaction: an isolated tentative copy of the document.
///
/// The public revision, the committed document and the real undo stack are all
/// untouched while this exists; `commit` installs the tentative document as one
/// undo step and `cancel`/failure drops it (see §6.8).
struct ActiveGroup {
    id: String,
    name: String,
    base_revision: u64,
    tentative: ProjectDoc,
    tentative_ids: IdentityTable,
    /// Merged change entries for the eventual commit result.
    pending: Vec<Change>,
    child_applied: usize,
}

/// A freshly opened group transaction.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct GroupBegin {
    pub group_id: String,
    pub base_revision: u64,
}

/// Staged Save As output. Hosts persist these bytes first, then call
/// `commit_save_as_identity` so a failed write cannot change the live session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SaveAsPayload {
    pub yaml: String,
    pub sidecar: String,
    pub pronunciation_sidecar: String,
    pub project_id: String,
}

const PRONUNCIATION_SCHEMA: &str = "rusted-utau.pronunciations/1";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PronunciationSidecar {
    schema: String,
    project_id: String,
    pronunciations: std::collections::BTreeMap<String, domain::UPronunciation>,
}

pub(crate) fn valid_pronunciation(choice: &domain::UPronunciation) -> bool {
    let valid_id = |value: &str| {
        !value.is_empty()
            && value.len() <= 32
            && value
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
    };
    valid_id(&choice.language)
        && matches!(choice.mode.as_str(), "universal" | "voicebank_native")
        && matches!(choice.style.as_str(), "cv" | "vcv" | "cvvc" | "vccv")
        && matches!(
            choice.strategy.as_str(),
            "direct_alias"
                | "phonetic_hint"
                | "japanese_vcv"
                | "mora_cv"
                | "g2p_cvvc"
                | "thai_vccv"
                | "letter_fallback"
                | "universal_ipa"
        )
        && !choice.language_phones.is_empty()
        && choice.language_phones.len() <= 64
        && choice
            .language_phones
            .iter()
            .all(|phone| !phone.is_empty() && phone.chars().count() <= 32)
        && !choice.universal_phones.is_empty()
        && choice.universal_phones.len() <= 64
        && choice
            .universal_phones
            .iter()
            .all(|phone| !phone.is_empty() && phone.chars().count() <= 32)
}

impl Session {
    /// Create a session for a brand-new project.
    pub fn new(seed: u64) -> Self {
        Self::new_with_metadata(None, None, seed).expect("default project metadata is valid")
    }

    /// Create a brand-new project using the optional §6.11 source metadata.
    /// The initial name/tempo are part of revision 0, before the session is
    /// published; they do not create undo history or a mutation revision.
    pub fn new_with_metadata(
        name: Option<&str>,
        bpm: Option<f64>,
        seed: u64,
    ) -> Result<Self, String> {
        let mut project = UProject::create();
        if let Some(name) = name.filter(|name| !name.trim().is_empty()) {
            project.name = name.trim().to_string();
        }
        if let Some(bpm) = bpm {
            if !bpm.is_finite() || bpm <= 0.0 {
                return Err("bpm must be a finite positive number".to_string());
            }
            project.bpm = bpm;
            if let Some(tempo) = project.tempos.first_mut() {
                tempo.bpm = bpm;
            }
            project.validate()?;
        }
        let doc = ProjectDoc::from_project(project);
        let mut gen = IdGenerator::from_seed(seed);
        let (ids, mut warnings) = identity::reconcile(doc.project(), None, &mut gen);
        // A brand-new document has nothing to recover, so "ids were
        // regenerated" would be noise for the host.
        warnings.retain(|w| !matches!(w, IdentityWarning::IdsRegenerated));
        let session_id = mint_session_id(&mut gen);
        Ok(Self {
            doc,
            ids,
            gen,
            revision: 0,
            session_id,
            ids_stable: true,
            warnings,
            group: None,
            legacy_group_open: false,
            legacy_group_changed: false,
        })
    }

    /// Open a stored project.
    ///
    /// `sidecar` is the raw sidecar payload when the host found one. An
    /// unusable sidecar (bad JSON, unknown schema, malformed ids) never fails
    /// the open: IDs are regenerated and reported through
    /// [`Session::ids_stable`] plus [`Session::warnings`] (§6.2.4).
    pub fn open(yaml: &str, sidecar: Option<&str>, seed: u64) -> Result<Self, String> {
        Self::open_with_pronunciation_sidecar(yaml, sidecar, None, seed)
    }

    /// Open a stored project with its optional identity and Rusted UTAU pronunciation
    /// sidecars. Invalid pronunciation sidecars are ignored as a whole; USTX
    /// remains openable and the caller can report the warning separately.
    pub fn open_with_pronunciation_sidecar(
        yaml: &str,
        sidecar: Option<&str>,
        pronunciation_sidecar: Option<&str>,
        seed: u64,
    ) -> Result<Self, String> {
        // BOM-tolerant: `UProject::from_yaml_text` strips the UTF-8 BOM that
        // every OpenUtau-written `.ustx` starts with. Parsing here directly
        // instead would fail with a misleading "more than one document" error
        // and make every real project — including the multi-track fixtures —
        // unopenable. `after_load_with_warnings` then runs exactly once below.
        let mut project: UProject = UProject::from_yaml_text(yaml)?;
        let mut warnings: Vec<IdentityWarning> = project
            .after_load_with_warnings()
            .map_err(|e| format!("after_load: {e}"))?
            .into_iter()
            .map(load_repair_warning)
            .collect();

        let doc = ProjectDoc::from_project(project);
        let mut gen = IdGenerator::from_seed(seed);

        let previous = match sidecar {
            Some(json) => table_from_json(json).ok(),
            None => None,
        };
        let ids_stable = previous.is_some();
        let (ids, identity_warnings) =
            identity::reconcile(doc.project(), previous.as_ref(), &mut gen);
        warnings.extend(identity_warnings);
        let session_id = mint_session_id(&mut gen);

        let mut session = Self {
            doc,
            ids,
            gen,
            revision: 0,
            session_id,
            ids_stable,
            warnings,
            group: None,
            legacy_group_open: false,
            legacy_group_changed: false,
        };
        if let Some(raw) = pronunciation_sidecar {
            let parsed = serde_json::from_str::<PronunciationSidecar>(raw);
            let known = session.ids.note_id_set();
            let usable = parsed.ok().filter(|parsed| {
                parsed.schema == PRONUNCIATION_SCHEMA
                    && parsed.project_id == session.ids.project_id.as_str()
                    && parsed
                        .pronunciations
                        .iter()
                        .all(|(id, choice)| known.contains(id) && valid_pronunciation(choice))
            });
            if let Some(parsed) = usable {
                session
                    .doc
                    .restore_rusted_utau_pronunciations(parsed.pronunciations);
            } else {
                session
                    .warnings
                    .push(IdentityWarning::PronunciationSidecarIgnored);
            }
        }
        Ok(session)
    }

    // ---- reads -----------------------------------------------------------

    /// Current committed revision (starts at 0 for every session).
    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// Opaque session token (host-visible only; not an entity ID).
    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    /// Whether the session adopted a usable sidecar (§6.2.4).
    pub fn ids_stable(&self) -> bool {
        self.ids_stable
    }

    /// Repairs and identity events observed while opening or editing.
    pub fn warnings(&self) -> &[IdentityWarning] {
        &self.warnings
    }

    /// The authoritative project document.
    pub fn project(&self) -> &UProject {
        self.doc.project()
    }

    /// Canonical USTX for the current document.
    ///
    /// This is what save/export/render must consume; it is computed on demand
    /// (never per mutation) because serialization dominates the cost on large
    /// projects.
    pub fn yaml(&self) -> String {
        self.doc.yaml()
    }

    /// Committed pronunciation selections for the voice parts of a track,
    /// aligned with the pipeline's project-order voice-part list and each
    /// part's note order. This is a transient render input, not a second
    /// project model.
    pub fn render_pronunciations_for_track(
        &self,
        track_index: usize,
    ) -> Vec<Vec<Option<domain::UPronunciation>>> {
        let Some(track_ids) = self.ids.tracks.get(track_index) else {
            return Vec::new();
        };
        let mut selections = Vec::new();
        let mut part_index = 0usize;
        for part in self.doc.project().parts.iter() {
            if part.track_no() != track_index as i32 {
                continue;
            }
            if let UPart::Voice(voice_part) = part {
                let notes = (0..voice_part.notes.len())
                    .map(|note_index| {
                        self.ids
                            .note_id(track_index, part_index, note_index)
                            .and_then(|id| {
                                self.doc
                                    .project()
                                    .rusted_utau_pronunciations
                                    .get(id.as_str())
                                    .cloned()
                            })
                    })
                    .collect();
                selections.push(notes);
            }
            part_index += 1;
            if part_index >= track_ids.parts.len() {
                break;
            }
        }
        selections
    }

    /// Canonical YAML through `ProjectDoc`'s lazy snapshot cache.
    pub fn yaml_snapshot(&mut self) -> &str {
        self.doc.snapshot()
    }

    /// Read-only legacy WASM view helpers. They keep the old JS facade stable
    /// while making this session's document the only authoritative holder.
    pub fn legacy_notes_json(&self, track: i32) -> String {
        self.doc.notes_json(track)
    }

    pub fn legacy_curves_json(&self, track: i32) -> String {
        self.doc.curves_json(track)
    }

    pub fn legacy_tracks_json(&self) -> String {
        self.doc.tracks_json()
    }

    pub fn legacy_meta_json(&self) -> String {
        self.doc.meta_json()
    }

    /// The identity table currently bound to the document.
    pub fn identity(&self) -> &IdentityTable {
        &self.ids
    }

    /// Look up a note ID by `(track, part, note)` index.
    pub fn note_id(&self, track: usize, part: usize, note: usize) -> Option<&EntityId> {
        self.ids.note_id(track, part, note)
    }

    /// Look up a curve ID by `(track, part, curve)` index.
    pub fn curve_id(&self, track: usize, part: usize, curve: usize) -> Option<&EntityId> {
        self.ids.curve_id(track, part, curve)
    }

    pub fn can_undo(&self) -> bool {
        self.doc.can_undo()
    }

    pub fn can_redo(&self) -> bool {
        self.doc.can_redo()
    }

    // ---- identity persistence -------------------------------------------

    /// Serialize the identity table for the host to write as the sidecar.
    ///
    /// Does not compute the document fingerprint — use
    /// [`Session::save_sidecar`] when persisting.
    pub fn identity_json(&self) -> Result<String, String> {
        table_to_json(&self.ids)
    }

    /// Refresh the document fingerprint and serialize the sidecar payload.
    ///
    /// The fingerprint costs one canonical serialization, so it happens on
    /// save only — never on the mutation hot path.
    pub fn save_sidecar(&mut self) -> Result<String, String> {
        identity::set_document_fingerprint(&mut self.ids, &self.doc.yaml());
        table_to_json(&self.ids)
    }

    /// Serialize Rusted UTAU-owned note pronunciation choices separately from USTX
    /// and the stable-ID sidecar.
    pub fn save_pronunciation_sidecar(&self) -> Result<String, String> {
        let known = self.ids.note_id_set();
        let pronunciations = self
            .doc
            .project()
            .rusted_utau_pronunciations
            .iter()
            .filter(|(id, _)| known.contains(*id))
            .map(|(id, choice)| (id.clone(), choice.clone()))
            .collect();
        serde_json::to_string(&PronunciationSidecar {
            schema: PRONUNCIATION_SCHEMA.to_string(),
            project_id: self.ids.project_id.as_str().to_string(),
            pronunciations,
        })
        .map_err(|e| format!("serialize pronunciation sidecar: {e}"))
    }

    /// Prepare a Save As bundle with a forked project ID and unchanged entity
    /// IDs. This is side-effect free; hosts persist it before committing.
    pub fn prepare_save_as(&self) -> Result<SaveAsPayload, String> {
        let yaml = self.doc.yaml();
        let mut ids = self.ids.clone();
        let mut gen = self.gen.clone();
        identity::fork_for_save_as(&mut ids, &mut gen);
        identity::set_document_fingerprint(&mut ids, &yaml);
        let sidecar = table_to_json(&ids)?;
        let known = ids.note_id_set();
        let pronunciations = self
            .doc
            .project()
            .rusted_utau_pronunciations
            .iter()
            .filter(|(id, _)| known.contains(*id))
            .map(|(id, choice)| (id.clone(), choice.clone()))
            .collect();
        let pronunciation_sidecar = serde_json::to_string(&PronunciationSidecar {
            schema: PRONUNCIATION_SCHEMA.to_string(),
            project_id: ids.project_id.as_str().to_string(),
            pronunciations,
        })
        .map_err(|e| format!("serialize pronunciation sidecar: {e}"))?;
        Ok(SaveAsPayload {
            yaml,
            sidecar,
            pronunciation_sidecar,
            project_id: ids.project_id.as_str().to_string(),
        })
    }

    /// Adopt the identity fork prepared for a successful Save As. Musical
    /// entity IDs and the session revision remain unchanged.
    pub fn commit_save_as_identity(&mut self) -> String {
        identity::fork_for_save_as(&mut self.ids, &mut self.gen);
        identity::set_document_fingerprint(&mut self.ids, &self.doc.yaml());
        self.ids.project_id.as_str().to_string()
    }

    /// Snapshot DTO (§6.3) with stable IDs resolved from the identity table.
    pub fn snapshot_json(&self) -> Result<String, String> {
        let value = self.snapshot();
        serde_json::to_string(&value).map_err(|e| format!("serialize snapshot: {e}"))
    }

    /// Revision-tagged composition read. The request addresses a note by
    /// stable IDs, while candidate meanings are produced by the shared Rust
    /// phonemizer and the active host-provided voicebank.
    pub fn phoneme_compose_json(
        &self,
        request_json: &str,
        composer: &PhonemeComposer,
        voicebank: Option<&Voicebank>,
    ) -> Result<String, SessionError> {
        if request_json.len() > v2::MAX_COMMAND_BYTES {
            return Err(SessionError::payload_too_large(
                "max_command_bytes",
                v2::MAX_COMMAND_BYTES,
            ));
        }
        #[derive(Deserialize)]
        struct ComposeEnvelope {
            project_id: String,
            part_id: String,
            note_id: String,
            expected_revision: u64,
            #[serde(flatten)]
            request: CompositionRequest,
        }
        let mut envelope: ComposeEnvelope =
            serde_json::from_str(request_json).map_err(|error| {
                SessionError::validation(format!("invalid compose request: {error}"))
            })?;
        if envelope.expected_revision != self.revision {
            return Err(SessionError::revision_conflict(
                envelope.expected_revision,
                self.revision,
            ));
        }
        if envelope.project_id != self.ids.project_id.as_str() {
            return Err(SessionError::not_found("project", &envelope.project_id));
        }
        let located = v2::locate_note(&self.ids, self.doc.project(), &envelope.note_id)?;
        if located.part.part_id != envelope.part_id {
            return Err(SessionError::not_found("note", &envelope.note_id));
        }
        let note = match self.doc.project().parts.get(located.part.flat_index) {
            Some(UPart::Voice(part)) => part.notes.get(located.note_index),
            _ => None,
        }
        .ok_or_else(|| SessionError::not_found("note", &envelope.note_id))?;
        envelope.request.tone = note.tone;
        match voicebank {
            Some(bank) => {
                if envelope
                    .request
                    .voicebank_id
                    .as_deref()
                    .is_some_and(|requested| requested != bank.id)
                {
                    return Err(SessionError::validation_field(
                        "voicebank_id",
                        "voicebank_id does not match the active session voicebank",
                    ));
                }
                envelope.request.voicebank_id = Some(bank.id.clone());
            }
            None => envelope.request.voicebank_id = None,
        }
        let result = composer.compose(&envelope.request, voicebank);
        serde_json::to_string(&json!({"revision": self.revision, "result": result}))
            .map_err(|error| SessionError::internal(format!("serialize compose result: {error}")))
    }

    // ---- mutations -------------------------------------------------------

    /// Apply one JSON command.
    ///
    /// On `Err` nothing changes: not the document, revision, history or
    /// identity (§6.9). This low-level command entry point rejects group
    /// markers; use the v2 group methods or legacy compatibility bridge.
    pub fn apply_command(&mut self, command: &str) -> Result<MutationOutcome, String> {
        if self.group.is_some() || self.legacy_group_open {
            return Err(
                "session_busy: a group transaction is open (commit or cancel it first)".to_string(),
            );
        }
        if is_group_command(command) {
            return Err(
                "unsupported_operation: group transactions are not implemented in the session \
                 yet (use ProjectDoc for the legacy immediate-apply behavior)"
                    .to_string(),
            );
        }
        let name = self.doc.apply(command)?;
        self.revision += 1;
        self.reconcile_ids();
        Ok(self.outcome(name.unwrap_or_default(), true))
    }

    /// Apply the pre-v2 command vocabulary used by the existing WASM facade.
    /// Document ownership, IDs, history and revisions still live in this
    /// `Session`. Legacy groups keep their frozen immediate-apply behavior;
    /// only the v2 `group_*` methods use isolated tentative state.
    pub fn apply_legacy_command(&mut self, command: &str) -> Result<Option<String>, String> {
        let value: serde_json::Value =
            serde_json::from_str(command).map_err(|e| format!("parse command: {e}"))?;
        let kind = value
            .get("cmd")
            .and_then(|v| v.as_str())
            .ok_or_else(|| "command has no `cmd` field".to_string())?;
        if self.group.is_some() {
            return Err("session_busy: a v2 group transaction is open".into());
        }
        let was_legacy_group_open = self.legacy_group_open;
        let name = self.doc.apply(command)?;
        match kind {
            "group_begin" => {
                self.legacy_group_open = true;
                self.legacy_group_changed = false;
            }
            "group_end" => {
                self.legacy_group_open = false;
                self.legacy_group_changed = false;
            }
            "group_cancel" => {
                self.legacy_group_open = false;
                if self.legacy_group_changed {
                    self.revision += 1;
                    self.reconcile_ids();
                }
                self.legacy_group_changed = false;
            }
            _ => {
                self.revision += 1;
                self.reconcile_ids();
                if was_legacy_group_open {
                    self.legacy_group_changed = true;
                }
            }
        }
        Ok(name)
    }

    /// Undo the last committed step.
    ///
    /// `Ok(None)` means there was nothing to undo: revision and history are
    /// unchanged (`changed: false` in §6.9).
    pub fn undo(&mut self) -> Result<Option<MutationOutcome>, String> {
        if self.group.is_some() || self.legacy_group_open {
            return Err(
                "session_busy: a group transaction is open (commit or cancel it first)".to_string(),
            );
        }
        match self.doc.undo()? {
            Some(name) => {
                self.revision += 1;
                self.reconcile_ids();
                Ok(Some(self.outcome(name, true)))
            }
            None => Ok(None),
        }
    }

    /// Redo the last undone step (`Ok(None)` = nothing to redo).
    pub fn redo(&mut self) -> Result<Option<MutationOutcome>, String> {
        if self.group.is_some() || self.legacy_group_open {
            return Err(
                "session_busy: a group transaction is open (commit or cancel it first)".to_string(),
            );
        }
        match self.doc.redo()? {
            Some(name) => {
                self.revision += 1;
                self.reconcile_ids();
                Ok(Some(self.outcome(name, true)))
            }
            None => Ok(None),
        }
    }

    // ---- v2 surface (§6.4, §6.10) ---------------------------------------

    /// Apply one v2 request: envelope + stable-ID command.
    ///
    /// Order matters and mirrors §6.9: check `expected_revision`, resolve every
    /// ID *before* mutating, apply through the engine, then bump the revision,
    /// re-infer identity and only then build the change list (so a newly
    /// created entity already has its ID).
    pub fn apply_v2(&mut self, request_json: &str) -> Result<MutationResult, SessionError> {
        if self.legacy_group_open {
            return Err(SessionError::new(
                ERR_SESSION_BUSY,
                "a legacy group is open; finish it before using the v2 session surface",
            ));
        }
        // §6.15: the declared limit is enforced, not merely advertised.
        if request_json.len() > v2::MAX_COMMAND_BYTES {
            return Err(SessionError::payload_too_large(
                "max_command_bytes",
                v2::MAX_COMMAND_BYTES,
            ));
        }
        let request = v2::parse_request(request_json)?;
        match request.group_id.clone() {
            Some(group_id) => self.apply_group_child(request, &group_id),
            None => {
                if let Some(group_id) = self.group_id() {
                    return Err(SessionError::new(
                        ERR_SESSION_BUSY,
                        "a group transaction is open: include its group_id, or commit/cancel it",
                    )
                    .with_details(json!({"group_id": group_id})));
                }
                self.apply_committed(request)
            }
        }
    }

    /// Undo with the required optimistic revision guard (§6.9).
    pub fn undo_v2(
        &mut self,
        expected_revision: u64,
    ) -> Result<Option<MutationOutcome>, SessionError> {
        self.ensure_history_not_busy()?;
        self.check_expected_revision(expected_revision)?;
        self.undo().map_err(|message| {
            if message.starts_with("session_busy:") {
                SessionError::new(ERR_SESSION_BUSY, message)
            } else {
                SessionError::internal(message)
            }
        })
    }

    /// Redo with the required optimistic revision guard (§6.9).
    pub fn redo_v2(
        &mut self,
        expected_revision: u64,
    ) -> Result<Option<MutationOutcome>, SessionError> {
        self.ensure_history_not_busy()?;
        self.check_expected_revision(expected_revision)?;
        self.redo().map_err(|message| {
            if message.starts_with("session_busy:") {
                SessionError::new(ERR_SESSION_BUSY, message)
            } else {
                SessionError::internal(message)
            }
        })
    }

    fn check_expected_revision(&self, expected_revision: u64) -> Result<(), SessionError> {
        if expected_revision != self.revision {
            return Err(SessionError::revision_conflict(
                expected_revision,
                self.revision,
            ));
        }
        Ok(())
    }

    fn ensure_history_not_busy(&self) -> Result<(), SessionError> {
        if self.group.is_some() || self.legacy_group_open {
            return Err(SessionError::new(
                ERR_SESSION_BUSY,
                "cannot undo or redo while a group is open",
            ));
        }
        Ok(())
    }

    /// The ordinary (non-group) mutation path.
    fn apply_committed(&mut self, request: v2::Request) -> Result<MutationResult, SessionError> {
        if request.expected_revision != self.revision {
            return Err(SessionError::revision_conflict(
                request.expected_revision,
                self.revision,
            ));
        }
        if let Some(plan) = v2::plan_document(&self.ids, self.doc.project(), &request.command)? {
            if plan.after == *self.doc.project() {
                return Ok(MutationResult {
                    revision: self.revision,
                    command_name: plan.command_name,
                    changed: false,
                    can_undo: self.doc.can_undo(),
                    can_redo: self.doc.can_redo(),
                    changes: Vec::new(),
                    snapshot: if request.include_snapshot {
                        Some(
                            serde_json::to_value(self.snapshot())
                                .map_err(|e| SessionError::internal(e.to_string()))?,
                        )
                    } else {
                        None
                    },
                    group_id: None,
                });
            }
            let history_before = (self.doc.can_undo(), self.doc.can_redo());
            let (command_name, mut changes, warnings) =
                Self::apply_document_plan(&mut self.doc, &mut self.ids, &mut self.gen, plan)?;
            self.warnings.extend(warnings);
            self.revision += 1;
            let history_after = (self.doc.can_undo(), self.doc.can_redo());
            if history_before != history_after {
                changes.push(Change::patch(
                    "history.changed",
                    json!({"can_undo": history_after.0, "can_redo": history_after.1}),
                ));
            }
            let snapshot = if request.include_snapshot {
                Some(
                    serde_json::to_value(self.snapshot())
                        .map_err(|e| SessionError::internal(e.to_string()))?,
                )
            } else {
                None
            };
            return Ok(MutationResult {
                revision: self.revision,
                command_name,
                changed: true,
                can_undo: self.doc.can_undo(),
                can_redo: self.doc.can_redo(),
                changes,
                snapshot,
                group_id: None,
            });
        }
        let plan = v2::plan(&self.ids, self.doc.project(), &request.command)?;

        if plan.kind == "track.set_mix_fx" {
            if let v2::Target::Track { track, .. } = &plan.target {
                let track = *track;
                let requested = request
                    .command
                    .get("mix_fx")
                    .cloned()
                    .unwrap_or(Value::Null);
                let requested: Option<domain::UMixFx> = if requested.is_null() {
                    None
                } else {
                    Some(serde_json::from_value(requested).map_err(|error| {
                        SessionError::validation_field(
                            "mix_fx",
                            format!("invalid Mix FX object: {error}"),
                        )
                    })?)
                };
                if self
                    .doc
                    .project()
                    .tracks
                    .get(track)
                    .and_then(|value| value.mix_fx.as_ref())
                    == requested.as_ref()
                    && (requested.is_some()
                        || self
                            .doc
                            .project()
                            .tracks
                            .get(track)
                            .is_some_and(|value| value.mix_fx.is_none()))
                {
                    return Ok(MutationResult {
                        revision: self.revision,
                        command_name: "Set track Mix FX".into(),
                        changed: false,
                        can_undo: self.doc.can_undo(),
                        can_redo: self.doc.can_redo(),
                        changes: Vec::new(),
                        snapshot: if request.include_snapshot {
                            Some(
                                serde_json::to_value(self.snapshot())
                                    .map_err(|e| SessionError::internal(e.to_string()))?,
                            )
                        } else {
                            None
                        },
                        group_id: None,
                    });
                }
            }
        }

        let before_note_ids = self.ids.note_id_set();
        let history_before = (self.doc.can_undo(), self.doc.can_redo());

        let name = self
            .doc
            .apply(&plan.legacy_command)
            .map_err(SessionError::validation)?;
        self.revision += 1;
        self.reconcile_ids();

        let history_after = (self.doc.can_undo(), self.doc.can_redo());
        let changes = {
            let ctx = v2::ChangeContext {
                project: self.doc.project(),
                ids: &self.ids,
                before_note_ids: &before_note_ids,
                history_before,
                history_after,
            };
            v2::changes_for(&plan, &ctx)
        };
        let snapshot = if request.include_snapshot {
            Some(
                serde_json::to_value(self.snapshot())
                    .map_err(|e| SessionError::internal(e.to_string()))?,
            )
        } else {
            None
        };

        Ok(MutationResult {
            revision: self.revision,
            command_name: name.unwrap_or_default(),
            changed: true,
            can_undo: history_after.0,
            can_redo: history_after.1,
            changes,
            snapshot,
            group_id: None,
        })
    }

    /// The §6.15 capability descriptor: versions, features and limits that are
    /// actually implemented by this build.
    pub fn capabilities(&self) -> v2::Capabilities {
        v2::capabilities()
    }

    /// [`Session::capabilities`] serialized for a transport.
    pub fn capabilities_json(&self) -> Result<String, SessionError> {
        serde_json::to_string(&self.capabilities())
            .map_err(|e| SessionError::internal(e.to_string()))
    }

    /// Revision-tagged project metadata/maps bounded read (§6.10).
    pub fn project_json(&self) -> Result<String, SessionError> {
        let project = self.doc.project();
        let project_view = views::project_meta_view(self.ids.project_id.as_str(), project);
        Ok(json!({"revision": self.revision, "project": project_view}).to_string())
    }

    /// Revision-tagged single-part bounded read (§6.10).
    pub fn part_json(&self, part_id: &str) -> Result<String, SessionError> {
        let project = self.doc.project();
        let located = v2::locate_part(&self.ids, project, part_id)?;
        let part = v2::part_payload(&self.ids, project, &located.track_id, located.part_in_track)
            .ok_or_else(|| {
            SessionError::internal(format!("part {part_id} could not be serialized"))
        })?;
        Ok(json!({"revision": self.revision, "track_id": located.track_id, "part_id": located.part_id, "part": part}).to_string())
    }

    /// Notes of one part, tagged with the authoritative revision (§6.10).
    ///
    /// A client that receives `authority: "invalidate"` refetches this; a
    /// response whose `revision` is older than what the client holds must be
    /// discarded (§6.10 stale-response rule).
    pub fn part_notes_json(&self, part_id: &str) -> Result<String, SessionError> {
        let project = self.doc.project();
        let located = v2::locate_part(&self.ids, project, part_id)?;
        let vp = v2::part_at(project, &located)?;
        let notes: Vec<Value> = vp
            .notes
            .iter()
            .enumerate()
            .map(|(index, note)| {
                let id = self
                    .ids
                    .note_id(located.track, located.part_in_track, index)
                    .map(|id| id.as_str().to_string())
                    .unwrap_or_default();
                serde_json::to_value(views::note_view(
                    &id,
                    note,
                    project.rusted_utau_pronunciations.get(&id),
                ))
                .unwrap_or(Value::Null)
            })
            .collect();
        Ok(json!({
            "revision": self.revision,
            "track_id": located.track_id,
            "part_id": located.part_id,
            "position_ticks": vp.position,
            "duration_ticks": vp.duration,
            "notes": notes,
        })
        .to_string())
    }

    /// One curve by ID, tagged with the authoritative revision (§6.10).
    pub fn curve_json(&self, part_id: &str, curve_id: &str) -> Result<String, SessionError> {
        let project = self.doc.project();
        let located = v2::locate_curve(&self.ids, project, curve_id)?;
        if located.part.part_id != part_id {
            // The curve exists but not under the part the client asked about.
            return Err(SessionError::not_found("curve", curve_id));
        }
        let vp = v2::part_at(project, &located.part)?;
        let curve = vp
            .curves
            .get(located.curve_index)
            .ok_or_else(|| SessionError::internal(format!("curve {curve_id} out of range")))?;
        let view = views::curve_view(&located.curve_id, curve);
        Ok(json!({
            "revision": self.revision,
            "track_id": located.part.track_id,
            "part_id": located.part.part_id,
            "curve_id": view.id,
            "abbr": view.abbr,
            "points": view.points,
        })
        .to_string())
    }

    /// Revision-tagged expression descriptor read for the project's canonical
    /// expression registry.
    pub fn expression_descriptor_json(&self, abbr: &str) -> Result<String, SessionError> {
        let project = self.doc.project();
        let descriptor = project
            .expressions
            .get(abbr)
            .ok_or_else(|| SessionError::not_found("expression_descriptor", abbr))?;
        Ok(json!({
            "revision": self.revision,
            "descriptor": descriptor,
        })
        .to_string())
    }

    /// Revision-tagged curve/base and descriptor context used to prepare a
    /// transport-reduced `curve.apply_stroke`. This read does not mutate or
    /// canonicalize session state; callers must still submit one normal v2
    /// command at the returned revision.
    pub fn curve_stroke_context_json(
        &self,
        part_id: &str,
        curve_id: &str,
    ) -> Result<String, SessionError> {
        let project = self.doc.project();
        let located = v2::locate_curve(&self.ids, project, curve_id)?;
        if located.part.part_id != part_id {
            return Err(SessionError::not_found("curve", curve_id));
        }
        let vp = v2::part_at(project, &located.part)?;
        let curve = vp
            .curves
            .get(located.curve_index)
            .ok_or_else(|| SessionError::internal(format!("curve {curve_id} out of range")))?;
        let descriptor = project.expressions.get(&located.abbr).ok_or_else(|| {
            SessionError::validation_reason(
                "abbr",
                "unsupported_curve_descriptor",
                format!("curve descriptor '{}' is not registered", located.abbr),
            )
        })?;
        if descriptor.r#type != domain::UExpressionType::Curve {
            return Err(SessionError::validation_reason(
                "abbr",
                "unsupported_curve_descriptor",
                format!("expression '{}' is not a Curve descriptor", located.abbr),
            ));
        }
        let min = descriptor.min as i32;
        let max = descriptor.max as i32;
        Ok(json!({
            "revision": self.revision,
            "part_id": located.part.part_id,
            "curve_id": located.curve_id,
            "abbr": located.abbr,
            "points": views::curve_view(&located.curve_id, curve).points,
            "descriptor": {
                "min_value": min,
                "max_value": max,
                "default_value": descriptor.custom_default_value() as i32,
                "simplify_tolerance": 5.0_f64.min((descriptor.max as f64 - descriptor.min as f64) * 0.005),
            },
        })
        .to_string())
    }

    /// Revision-tagged stroke context for a curve descriptor in a part. When
    /// no curve exists yet, return its descriptor defaults and an empty point
    /// list so a first stroke can be prepared without a frontend guess.
    pub fn curve_stroke_context_for_abbr_json(
        &self,
        part_id: &str,
        abbr: &str,
        curve_id: Option<&str>,
    ) -> Result<String, SessionError> {
        let project = self.doc.project();
        let part_location = v2::locate_part(&self.ids, project, part_id)?;
        let part = v2::part_at(project, &part_location)?;
        let descriptor = project.expressions.get(abbr).ok_or_else(|| {
            SessionError::validation_reason(
                "abbr",
                "unsupported_curve_descriptor",
                format!("curve descriptor '{abbr}' is not registered"),
            )
        })?;
        if descriptor.r#type != domain::UExpressionType::Curve {
            return Err(SessionError::validation_reason(
                "abbr",
                "unsupported_curve_descriptor",
                format!("expression '{abbr}' is not a Curve descriptor"),
            ));
        }

        let existing = if let Some(curve_id) = curve_id {
            let located = v2::locate_curve(&self.ids, project, curve_id)?;
            if located.part.part_id != part_id || located.abbr != abbr {
                return Err(SessionError::not_found("curve", curve_id));
            }
            Some((located.curve_id, located.curve_index))
        } else {
            part.curves
                .iter()
                .position(|curve| curve.abbr == abbr)
                .and_then(|index| {
                    self.ids
                        .curve_id(part_location.track, part_location.part_in_track, index)
                        .map(|id| (id.as_str().to_string(), index))
                })
        };
        let (resolved_curve_id, points) = match existing {
            Some((id, index)) => {
                let curve = part.curves.get(index).ok_or_else(|| {
                    SessionError::internal("curve index disappeared during stroke-context read")
                })?;
                (Some(id), views::curve_view("", curve).points)
            }
            None => (None, Vec::new()),
        };
        Ok(json!({
            "revision": self.revision,
            "track_id": part_location.track_id,
            "part_id": part_location.part_id,
            "curve_id": resolved_curve_id,
            "abbr": abbr,
            "points": points,
            "descriptor": {
                "min_value": descriptor.min as i32,
                "max_value": descriptor.max as i32,
                "default_value": descriptor.custom_default_value() as i32,
                "simplify_tolerance": 5.0_f64.min((descriptor.max as f64 - descriptor.min as f64) * 0.005),
            },
        })
        .to_string())
    }

    /// Track metadata for every track, tagged with the authoritative revision.
    pub fn tracks_json(&self) -> Result<String, SessionError> {
        let project = self.doc.project();
        let tracks: Vec<Value> = project
            .tracks
            .iter()
            .enumerate()
            .map(|(index, track)| {
                let id = self
                    .ids
                    .track_id(index)
                    .map(|id| id.as_str().to_string())
                    .unwrap_or_default();
                serde_json::to_value(views::track_meta_view(&id, track)).unwrap_or(Value::Null)
            })
            .collect();
        Ok(json!({"revision": self.revision, "tracks": tracks}).to_string())
    }

    // ---- group transactions (§6.8) ---------------------------------------

    /// The open group's id, if any.
    pub fn group_id(&self) -> Option<String> {
        self.group.as_ref().map(|g| g.id.clone())
    }

    /// Whether a group transaction is open.
    pub fn group_is_open(&self) -> bool {
        self.group.is_some()
    }

    /// Open an isolated tentative transaction (§6.8).
    ///
    /// The committed document, the public revision and the undo history are all
    /// unchanged until [`Session::group_commit`]. At most one group may be open.
    pub fn group_begin(
        &mut self,
        expected_revision: u64,
        name: &str,
    ) -> Result<GroupBegin, SessionError> {
        if self.legacy_group_open {
            return Err(SessionError::new(
                ERR_SESSION_BUSY,
                "a legacy group is open; finish it before beginning a v2 group",
            ));
        }
        if self.group.is_some() {
            return Err(SessionError::new(
                ERR_INVALID_GROUP_STATE,
                "a group transaction is already open (nested groups are not supported)",
            ));
        }
        if expected_revision != self.revision {
            return Err(SessionError::revision_conflict(
                expected_revision,
                self.revision,
            ));
        }
        let group_id = self.gen.next_id().as_str().to_string();
        let name = if name.trim().is_empty() {
            "Edit".to_string()
        } else {
            name.to_string()
        };
        // Lazy snapshot: opening a transaction must not serialize the project.
        let tentative = ProjectDoc::from_project_lazy(self.doc.project().clone());
        let tentative_ids = self.ids.clone();
        let base_revision = self.revision;
        self.group = Some(ActiveGroup {
            id: group_id.clone(),
            name,
            base_revision,
            tentative,
            tentative_ids,
            pending: Vec::new(),
            child_applied: 0,
        });
        Ok(GroupBegin {
            group_id,
            base_revision,
        })
    }

    /// Apply one command inside the open group.
    ///
    /// Children see only the tentative state, never bump the public revision and
    /// return no `changes` (§6.8.2). A child that fails discards the whole
    /// tentative state and closes the group (`group_failed`), so a partially
    /// applied transaction can never be committed or inspected.
    fn apply_group_child(
        &mut self,
        request: v2::Request,
        group_id: &str,
    ) -> Result<MutationResult, SessionError> {
        if self.group.as_ref().map(|g| g.id.as_str()) != Some(group_id) {
            return Err(SessionError::new(
                ERR_INVALID_GROUP_STATE,
                format!("no open group '{group_id}'"),
            )
            .with_details(json!({"group_id": group_id})));
        }
        if request.expected_revision != self.revision {
            return Err(SessionError::revision_conflict(
                request.expected_revision,
                self.revision,
            ));
        }

        let document_plan = {
            let group = self.group.as_ref().expect("checked above");
            v2::plan_document(
                &group.tentative_ids,
                group.tentative.project(),
                &request.command,
            )
        };
        let document_plan = match document_plan {
            Ok(plan) => plan,
            Err(cause) => {
                self.group = None;
                return Err(
                    SessionError::new(ERR_GROUP_FAILED, cause.message).with_details(json!({
                        "group_id": group_id,
                        "cause": cause.code,
                    })),
                );
            }
        };
        if let Some(document_plan) = document_plan {
            let mut failed = None;
            let mut changes = Vec::new();
            if let Some(group) = self.group.as_mut() {
                if document_plan.after == *group.tentative.project() {
                    return Ok(MutationResult {
                        revision: self.revision,
                        command_name: String::new(),
                        changed: false,
                        can_undo: self.doc.can_undo(),
                        can_redo: self.doc.can_redo(),
                        changes: Vec::new(),
                        snapshot: None,
                        group_id: Some(group_id.to_string()),
                    });
                }
                match Self::apply_document_plan(
                    &mut group.tentative,
                    &mut group.tentative_ids,
                    &mut self.gen,
                    document_plan,
                ) {
                    Ok((_, pending, warnings)) => {
                        changes = pending;
                        self.warnings.extend(warnings);
                    }
                    Err(error) => failed = Some(error),
                }
                group.child_applied += 1;
            }
            if let Some(error) = failed {
                self.group = None;
                return Err(SessionError::new(ERR_GROUP_FAILED, error.message)
                    .with_details(json!({"group_id":group_id,"cause":error.code})));
            }
            if let Some(group) = self.group.as_mut() {
                for change in changes {
                    v2::merge_change(&mut group.pending, change);
                }
            }
            return Ok(MutationResult {
                revision: self.revision,
                command_name: String::new(),
                changed: true,
                can_undo: self.doc.can_undo(),
                can_redo: self.doc.can_redo(),
                changes: Vec::new(),
                snapshot: None,
                group_id: Some(group_id.to_string()),
            });
        }

        // Resolve ordinary stable-ID commands against the tentative document.
        let plan = {
            let group = self.group.as_ref().expect("checked above");
            v2::plan(
                &group.tentative_ids,
                group.tentative.project(),
                &request.command,
            )
        };
        let plan = match plan {
            Ok(plan) => plan,
            Err(cause) => {
                self.group = None;
                return Err(
                    SessionError::new(ERR_GROUP_FAILED, cause.message).with_details(json!({
                        "group_id": group_id,
                        "cause": cause.code,
                    })),
                );
            }
        };

        let mut failure: Option<String> = None;
        let mut new_warnings = Vec::new();
        let mut changes = Vec::new();
        {
            let group = self.group.as_mut().expect("checked above");
            let before_note_ids = group.tentative_ids.note_id_set();
            let history_before = (group.tentative.can_undo(), group.tentative.can_redo());
            match group.tentative.apply(&plan.legacy_command) {
                Ok(_) => {
                    let (ids, warnings) = identity::reconcile(
                        group.tentative.project(),
                        Some(&group.tentative_ids),
                        &mut self.gen,
                    );
                    group.tentative_ids = ids;
                    new_warnings = warnings;
                    let history_after = (group.tentative.can_undo(), group.tentative.can_redo());
                    let ctx = v2::ChangeContext {
                        project: group.tentative.project(),
                        ids: &group.tentative_ids,
                        before_note_ids: &before_note_ids,
                        history_before,
                        history_after,
                    };
                    changes = v2::changes_for(&plan, &ctx);
                    group.child_applied += 1;
                }
                Err(e) => failure = Some(e),
            }
        }

        self.warnings.extend(new_warnings);
        if let Some(message) = failure {
            // Fail closed: drop the tentative state and close the group.
            self.group = None;
            return Err(SessionError::new(ERR_GROUP_FAILED, message)
                .with_details(json!({"group_id": group_id})));
        }
        if let Some(group) = self.group.as_mut() {
            for change in changes {
                v2::merge_change(&mut group.pending, change);
            }
        }
        Ok(MutationResult {
            revision: self.revision,
            command_name: String::new(),
            changed: true,
            can_undo: self.doc.can_undo(),
            can_redo: self.doc.can_redo(),
            changes: Vec::new(),
            snapshot: None,
            group_id: Some(group_id.to_string()),
        })
    }

    fn apply_document_plan(
        doc: &mut ProjectDoc,
        ids: &mut IdentityTable,
        gen: &mut IdGenerator,
        mut plan: v2::DocumentPlan,
    ) -> Result<(String, Vec<Change>, Vec<IdentityWarning>), SessionError> {
        let command_name = plan.command_name.clone();
        let mut next_gen = gen.clone();
        let (next_ids, warnings) = identity::reconcile(&plan.after, Some(ids), &mut next_gen);
        plan.after
            .rusted_utau_pronunciations
            .retain(|note_id, _| next_ids.find_note(note_id).is_some());
        doc.commit_transaction(plan.after.clone(), &command_name)
            .map_err(SessionError::validation)?;
        *ids = next_ids;
        *gen = next_gen;
        let changes = v2::document_changes(&plan, ids, doc.project());
        Ok((command_name, changes, warnings))
    }

    /// Commit the group as exactly one undo step and one revision increment.
    ///
    /// A group with no applied child is a no-op: no revision change, no undo
    /// step, `changed: false` (§6.9). A stale base revision discards the
    /// tentative state and leaves the committed document untouched.
    pub fn group_commit(
        &mut self,
        group_id: &str,
        expected_revision: u64,
    ) -> Result<MutationResult, SessionError> {
        let Some(group) = self.group.take() else {
            return Err(SessionError::not_found("group", group_id));
        };
        if group.id != group_id {
            self.group = Some(group);
            return Err(SessionError::not_found("group", group_id));
        }
        if expected_revision != self.revision || group.base_revision != self.revision {
            return Err(SessionError::revision_conflict(
                expected_revision,
                self.revision,
            ));
        }
        if group.child_applied == 0 {
            return Ok(MutationResult {
                revision: self.revision,
                command_name: group.name,
                changed: false,
                can_undo: self.doc.can_undo(),
                can_redo: self.doc.can_redo(),
                changes: Vec::new(),
                snapshot: None,
                group_id: Some(group_id.to_string()),
            });
        }

        let history_before = (self.doc.can_undo(), self.doc.can_redo());
        let after = group.tentative.project().clone();
        let name = group.name.clone();
        self.doc
            .commit_transaction(after, &name)
            .map_err(SessionError::internal)?;
        // The tentative identity table already matches the installed document.
        self.ids = group.tentative_ids;
        self.revision += 1;

        let mut changes = group.pending;
        let history_after = (self.doc.can_undo(), self.doc.can_redo());
        if history_before != history_after {
            changes.push(Change::patch(
                "history.changed",
                json!({"can_undo": history_after.0, "can_redo": history_after.1}),
            ));
        }
        changes.push(Change::invalidate("render.invalidated").with_reason("group committed"));
        Ok(MutationResult {
            revision: self.revision,
            command_name: name,
            changed: true,
            can_undo: self.doc.can_undo(),
            can_redo: self.doc.can_redo(),
            changes,
            snapshot: None,
            group_id: Some(group_id.to_string()),
        })
    }

    /// Cancel the group: the committed document, revision and history are
    /// exactly as before `group_begin` (§6.8.2). Not idempotent — a closed
    /// group reports `not_found`.
    pub fn group_cancel(&mut self, group_id: &str) -> Result<(), SessionError> {
        match self.group.take() {
            Some(group) if group.id == group_id => Ok(()),
            Some(group) => {
                self.group = Some(group);
                Err(SessionError::not_found("group", group_id))
            }
            None => Err(SessionError::not_found("group", group_id)),
        }
    }

    // ---- internals -------------------------------------------------------

    fn outcome(&self, command_name: String, changed: bool) -> MutationOutcome {
        MutationOutcome {
            revision: self.revision,
            command_name,
            changed,
            can_undo: self.doc.can_undo(),
            can_redo: self.doc.can_redo(),
        }
    }

    /// Re-infer identity after a change. IDs are matched against the previous
    /// table, so a reorder/edit keeps them and a new entity gets a fresh one;
    /// unmatched entries become tombstones.
    fn reconcile_ids(&mut self) {
        let (ids, warnings) =
            identity::reconcile(self.doc.project(), Some(&self.ids), &mut self.gen);
        self.ids = ids;
        self.warnings.extend(warnings);
    }

    fn snapshot(&self) -> views::Snapshot {
        views::snapshot(
            &self.session_id,
            self.revision,
            self.doc.can_undo(),
            self.doc.can_redo(),
            self.ids_stable,
            self.doc.project(),
            &self.ids,
        )
    }
}

impl Default for Session {
    fn default() -> Self {
        Self::new(0)
    }
}

fn mint_session_id(gen: &mut IdGenerator) -> String {
    gen.next_id().as_str().to_string()
}

/// A `group_*` command (legacy facade naming).
fn is_group_command(command: &str) -> bool {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(command) else {
        return false;
    };
    value
        .get("cmd")
        .and_then(|v| v.as_str())
        .is_some_and(|kind| kind.starts_with("group_"))
}

fn load_repair_warning(repair: domain::LoadRepair) -> IdentityWarning {
    match repair {
        domain::LoadRepair::ArraysTruncated { .. } => IdentityWarning::RepairedCurveArrays,
        domain::LoadRepair::UnregisteredAbbr { abbr, .. } => {
            IdentityWarning::UnregisteredCurveAbbr { abbr }
        }
        domain::LoadRepair::DuplicateAbbr { abbr, .. } => {
            IdentityWarning::DuplicateCurveAbbr { abbr }
        }
        domain::LoadRepair::MissingTracksRepaired { created } => {
            IdentityWarning::MissingTracksRepaired { created }
        }
        domain::LoadRepair::PartReferencesMissingTrack { part, track_no } => {
            IdentityWarning::PartReferencesMissingTrack { part, track_no }
        }
    }
}

/// Count voice parts across every track (test/support helper).
pub fn voice_part_count(project: &UProject) -> usize {
    project
        .parts
        .iter()
        .filter(|p| matches!(p, UPart::Voice(_)))
        .count()
}
