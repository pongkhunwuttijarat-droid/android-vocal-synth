//! A stateful project session: authoritative document + identity + revision.
//!
//! This is the shared logical session described in `docs/CONTRACT-v1.md` §6.
//! It is transport-neutral: the native server, the WASM facade and future JNI
//! adapters all drive this type so revision, identity and staleness rules
//! cannot diverge between hosts.
//!
//! # What is implemented in this slice
//!
//! * revision counter (§6.9) — starts at 0, +1 per committed change only;
//! * stable entity IDs (§6.2) — exposed in the snapshot and the sidecar;
//! * `ids_stable` + warnings (§6.2.4);
//! * undo/redo with the §6.9 revision rule;
//! * rejected commands leave document, revision, history and identity intact.
//!
//! # What is deliberately NOT implemented yet
//!
//! * **Group transactions (§6.8)** are refused here (`unsupported_operation`).
//!   The legacy `ProjectDoc` facade still accepts the string
//!   `group_begin`/`group_end`/`group_cancel` commands and applies them
//!   immediately — that behavior is unchanged and is *not* the v2 contract
//!   (isolated tentative transactions land with the group task). Refusing them
//!   at the session layer is what prevents a host from publishing a revision
//!   that does not match the frozen §6.9 table.
//! * the `changes[]` vocabulary is not emitted yet (the result carries
//!   revision/command name/capabilities only);
//! * bounded read models beyond the full snapshot.

use serde::Serialize;
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

impl Session {
    /// Create a session for a brand-new project.
    pub fn new(seed: u64) -> Self {
        let doc = ProjectDoc::new();
        let mut gen = IdGenerator::from_seed(seed);
        let (ids, mut warnings) = identity::reconcile(doc.project(), None, &mut gen);
        // A brand-new document has nothing to recover, so "ids were
        // regenerated" would be noise for the host.
        warnings.retain(|w| !matches!(w, IdentityWarning::IdsRegenerated));
        let session_id = mint_session_id(&mut gen);
        Self {
            doc,
            ids,
            gen,
            revision: 0,
            session_id,
            ids_stable: true,
            warnings,
            group: None,
        }
    }

    /// Open a stored project.
    ///
    /// `sidecar` is the raw sidecar payload when the host found one. An
    /// unusable sidecar (bad JSON, unknown schema, malformed ids) never fails
    /// the open: IDs are regenerated and reported through
    /// [`Session::ids_stable`] plus [`Session::warnings`] (§6.2.4).
    pub fn open(yaml: &str, sidecar: Option<&str>, seed: u64) -> Result<Self, String> {
        let mut project: UProject =
            serde_yaml::from_str(yaml).map_err(|e| format!("parse project: {e}"))?;
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

        Ok(Self {
            doc,
            ids,
            gen,
            revision: 0,
            session_id,
            ids_stable,
            warnings,
            group: None,
        })
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

    /// Snapshot DTO (§6.3) with stable IDs resolved from the identity table.
    pub fn snapshot_json(&self) -> Result<String, String> {
        let value = self.snapshot();
        serde_json::to_string(&value).map_err(|e| format!("serialize snapshot: {e}"))
    }

    // ---- mutations -------------------------------------------------------

    /// Apply one JSON command.
    ///
    /// On `Err` nothing changes: not the document, revision, history or
    /// identity (§6.9). Group commands are refused until the isolated
    /// transaction semantics exist (see the module docs).
    pub fn apply_command(&mut self, command: &str) -> Result<MutationOutcome, String> {
        if self.group.is_some() {
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

    /// Undo the last committed step.
    ///
    /// `Ok(None)` means there was nothing to undo: revision and history are
    /// unchanged (`changed: false` in §6.9).
    pub fn undo(&mut self) -> Result<Option<MutationOutcome>, String> {
        if self.group.is_some() {
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
        if self.group.is_some() {
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

    /// The ordinary (non-group) mutation path.
    fn apply_committed(&mut self, request: v2::Request) -> Result<MutationResult, SessionError> {
        if request.expected_revision != self.revision {
            return Err(SessionError::revision_conflict(
                request.expected_revision,
                self.revision,
            ));
        }
        let plan = v2::plan(&self.ids, self.doc.project(), &request.command)?;

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
                serde_json::to_value(views::note_view(&id, note)).unwrap_or(Value::Null)
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

        // Resolve IDs against the tentative document before touching it.
        let plan = {
            let group = self.group.as_ref().expect("checked above");
            v2::plan(
                &group.tentative_ids,
                group.tentative.project(),
                &request.command,
            )?
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

        let after = group.tentative.project().clone();
        let name = group.name.clone();
        self.doc
            .commit_transaction(after, &name)
            .map_err(SessionError::internal)?;
        // The tentative identity table already matches the installed document.
        self.ids = group.tentative_ids;
        self.revision += 1;

        let mut changes = group.pending;
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
