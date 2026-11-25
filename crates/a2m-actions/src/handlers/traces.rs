//! A run's evidence as a summary and as a file: `run.trace` and
//! `trace.export` (spec section 217, "Formats and tools"; decision 0998).
//!
//! `run.trace` is for the run's own person, as `run.get` is, and gives each
//! step's kind, outcome, and time with references to its captures, never a
//! value: a whole trace is never returned inline (spec section 221).
//! `trace.export` is the auditor's, as `trace.show` is, and writes the run's
//! events, its step trace, and its text captures into one file in the
//! project's working folder, with a manifest. Every piece is checked once
//! more for a registered secret before it is written, and a piece that holds
//! one is left out and named; the file is read back before the answer gives
//! its digest.

use a2m_core::{Digest, Failure};
use a2m_ledger::{BlobRef, RunRow};
use a2m_logs::{CaptureRef, TraceCheck, TraceLine};
use serde_json::{Value as Json, json};

use super::build::{off_thread, project};
use super::handler;
use super::tools::{run_id, shown_events};
use crate::codes;
use crate::dispatch::{Next, Output};
use crate::kernel::Kernel;

/// The most steps `run.trace` lists; the rest are counted.
pub(crate) const STEPS_SHOWN: usize = 200;

/// The folder of the project's working folder an export is written to.
pub(crate) const EXPORTS_FOLDER: &str = "exports";

/// The name of the export file's format, and its version.
pub(crate) const EXPORT_FORMAT: &str = "anything2mcp.trace_export";
const EXPORT_FORMAT_VERSION: u32 = 1;

/// A run's stored trace, read back: its lines and how they check, or why
/// they could not be read.
struct ReadTrace {
    blob: BlobRef,
    lines: Result<Vec<TraceLine>, &'static str>,
    text: Option<String>,
}

impl ReadTrace {
    async fn of(kernel: &Kernel, row: &RunRow) -> Result<Option<Self>, Failure> {
        let Some(blob) = row.trace else {
            return Ok(None);
        };
        let Ok(opened) = kernel.store()?.blobs().get(&blob).await else {
            return Ok(Some(Self {
                blob,
                lines: Err("the stored trace could not be opened"),
                text: None,
            }));
        };
        let text = String::from_utf8(opened.expose().to_vec()).ok();
        let lines = a2m_logs::read_trace(opened.expose())
            .map_err(|_| "the stored trace is not a trace of this schema");
        Ok(Some(Self { blob, lines, text }))
    }

    fn check(&self) -> Option<TraceCheck> {
        self.lines
            .as_ref()
            .ok()
            .map(|lines| a2m_logs::check_trace(lines))
    }

    /// The end line, unless the lines are broken: a broken trace's
    /// signature is not passed on as if it stood.
    fn end(&self) -> Option<&a2m_logs::EndLine> {
        if self.check() == Some(TraceCheck::Broken) {
            return None;
        }
        match self.lines.as_ref().ok()?.last()? {
            TraceLine::End(end) => Some(end),
            _ => None,
        }
    }

    fn header(&self) -> Option<&a2m_logs::HeaderLine> {
        match self.lines.as_ref().ok()?.first()? {
            TraceLine::Header(header) => Some(header),
            _ => None,
        }
    }

    fn steps(&self) -> impl Iterator<Item = &a2m_logs::StepLine> {
        self.lines.iter().flatten().filter_map(|line| match line {
            TraceLine::Step(step) => Some(step.as_ref()),
            _ => None,
        })
    }
}

fn capture_json(capture: &CaptureRef) -> Json {
    json!({
        "kind": serde_json::to_value(capture.kind).unwrap_or(Json::Null),
        "at": serde_json::to_value(capture.at).unwrap_or(Json::Null),
        "blob": format!("blob:{}", capture.blob),
    })
}

/// A capture a driver handed over and the engine kept, as the run's
/// events name it: its name, its media type, where it was taken, and its
/// blob.
struct KeptCapture {
    blob: BlobRef,
    about: Json,
}

impl KeptCapture {
    fn is_text(&self) -> bool {
        let media = self.about["media_type"].as_str().unwrap_or_default();
        media.starts_with("text/") || media == "application/json" || media.ends_with("+json")
    }

    fn listed(&self) -> Json {
        let about = &self.about;
        json!({
            "name": about["capture"],
            "media_type": about["media_type"],
            "phase": about["phase"],
            "step": about["step"],
            "record": about["record"],
            "blob": format!("blob:{}", self.blob),
        })
    }
}

/// The captures the run's events name, in the order they were kept.
async fn kept_captures(kernel: &Kernel, run: a2m_core::RunId) -> Result<Vec<KeptCapture>, Failure> {
    let events = kernel
        .store()?
        .read(move |tx| tx.ledger().events(run, 0, 10_000))
        .await?;
    Ok(events
        .iter()
        .filter(|event| event.kind == a2m_ledger::EventKind::Need)
        .filter_map(|event| {
            let about = serde_json::from_str::<Json>(&event.payload).ok()?;
            about.get("capture")?;
            Some(KeptCapture {
                blob: event.evidence?,
                about,
            })
        })
        .collect())
}

fn step_summary(step: &a2m_logs::StepLine) -> Json {
    let mut shown = json!({
        "seq": step.seq,
        "pass": step.pass,
        "phase": serde_json::to_value(step.phase).unwrap_or(Json::Null),
        "step": step.step,
        "kind": step.kind.as_str(),
        "took_us": step.took_us,
        "outcome": serde_json::to_value(&step.outcome).unwrap_or(Json::Null),
        "values": step.values.len(),
        "captures": step.captures.iter().map(capture_json).collect::<Vec<_>>(),
    });
    if let (Some(begins), Some(map)) = (&step.begins, shown.as_object_mut()) {
        map.insert("operation".to_owned(), json!(begins.operation.as_str()));
        map.insert("mode".to_owned(), json!(begins.mode.as_str()));
    }
    shown
}

/// What every answer about a run's trace says of the run.
fn run_facts(row: &RunRow) -> serde_json::Map<String, Json> {
    let mut facts = serde_json::Map::new();
    facts.insert("run_id".to_owned(), json!(row.id.to_string()));
    facts.insert("tool".to_owned(), json!(row.tool_name.as_str()));
    facts.insert("target".to_owned(), json!(row.target.as_str()));
    facts.insert("status".to_owned(), json!(row.state.as_str()));
    facts.insert("attempt".to_owned(), json!(row.attempt));
    facts.insert(
        "compiled_target_hash".to_owned(),
        json!(row.target_version.to_string()),
    );
    facts
}

/// What a read trace says of itself: its reference, its check, its
/// signature and counts when it stands, and its limitations.
fn trace_facts(read: &ReadTrace, facts: &mut serde_json::Map<String, Json>) {
    facts.insert("trace".to_owned(), json!(format!("blob:{}", read.blob)));
    match (&read.lines, read.check()) {
        (Err(why), _) => {
            facts.insert("trace_unreadable".to_owned(), json!(why));
        }
        (Ok(_), Some(check)) => {
            facts.insert("trace_check".to_owned(), json!(check.as_str()));
        }
        (Ok(_), None) => {}
    }
    if let Some(header) = read.header() {
        facts.insert(
            "level".to_owned(),
            serde_json::to_value(header.level).unwrap_or(Json::Null),
        );
        facts.insert("engine".to_owned(), json!(header.engine.to_string()));
    }
    if let Some(end) = read.end() {
        facts.insert("signature".to_owned(), json!(end.signature.to_string()));
        facts.insert("records".to_owned(), json!(end.records));
        facts.insert("kept".to_owned(), json!(end.kept));
        facts.insert(
            "limitations".to_owned(),
            serde_json::to_value(&end.limitations).unwrap_or(Json::Array(Vec::new())),
        );
    }
}

handler!(
    /// `run.trace`: a summary of one of the caller's own runs' trace, step
    /// by step, with references to its kept captures and no value.
    run_trace,
    |kernel, call| {
        let row = kernel.own_run(&call, run_id(&call)?).await?;
        let mut out = run_facts(&row);
        // What the engine kept beside the trace: each capture by its
        // reference, never its content.
        let kept = kept_captures(kernel, row.id).await?;
        out.insert(
            "captures".to_owned(),
            Json::Array(kept.iter().map(KeptCapture::listed).collect()),
        );
        let Some(read) = ReadTrace::of(kernel, &row).await? else {
            out.insert("steps".to_owned(), json!([]));
            out.insert("steps_total".to_owned(), json!(0));
            return Ok(Output::ok(Json::Object(out)).warn(
                "The run has no stored trace yet: a run's trace is stored when an attempt of it ends.",
            ));
        };
        trace_facts(&read, &mut out);
        let steps: Vec<&a2m_logs::StepLine> = read.steps().collect();
        out.insert("steps_total".to_owned(), json!(steps.len()));
        out.insert(
            "steps".to_owned(),
            Json::Array(
                steps
                    .iter()
                    .take(STEPS_SHOWN)
                    .map(|step| step_summary(step))
                    .collect(),
            ),
        );
        let mut output = Output::ok(Json::Object(out));
        if steps.len() > STEPS_SHOWN {
            output = output.warn(format!(
                "The trace has {} steps; the first {STEPS_SHOWN} are listed. `trace.export` writes them all to a file.",
                steps.len()
            ));
        }
        if read.check() == Some(TraceCheck::Broken) {
            output = output.warn(
                "The trace's lines do not give the signature its end line states: a line is missing, added, or moved. No signature is given.",
            );
        }
        Ok(output.then(Next::new(
            "trace.show",
            json!({"run_id": row.id.to_string()}),
            "to see the run's events",
        )))
    }
);

/// One piece of an export, or why it was left out.
struct Piece {
    part: &'static str,
    blob: Option<BlobRef>,
    capture: Option<CaptureRef>,
    /// What the run's events say of a capture the engine kept.
    kept: Option<Json>,
    content: Result<Json, &'static str>,
}

impl Piece {
    fn listed(&self, bytes: Option<usize>) -> Json {
        let mut listed = json!({"part": self.part});
        if let Some(map) = listed.as_object_mut() {
            if let Some(blob) = self.blob {
                map.insert("blob".to_owned(), json!(format!("blob:{blob}")));
            }
            if let Some(Json::Object(about)) = &self.kept {
                for (key, value) in about {
                    if key != "blob" {
                        map.insert(key.clone(), value.clone());
                    }
                }
            }
            if let Some(capture) = &self.capture {
                map.insert(
                    "kind".to_owned(),
                    serde_json::to_value(capture.kind).unwrap_or(Json::Null),
                );
                map.insert(
                    "at".to_owned(),
                    serde_json::to_value(capture.at).unwrap_or(Json::Null),
                );
            }
            match (&self.content, bytes) {
                (Err(why), _) => {
                    map.insert("left_out".to_owned(), json!(why));
                }
                (Ok(_), Some(bytes)) => {
                    map.insert("bytes".to_owned(), json!(bytes));
                }
                (Ok(_), None) => {}
            }
        }
        listed
    }
}

/// The capture of a step, opened as text, or why it is left out.
async fn capture_piece(kernel: &Kernel, capture: &CaptureRef) -> Result<Piece, Failure> {
    let blob = BlobRef::from_digest(capture.blob);
    let content = if capture.kind.is_text() {
        match kernel.store()?.blobs().get(&blob).await {
            Ok(opened) => String::from_utf8(opened.expose().to_vec())
                .map(Json::String)
                .map_err(|_| "the capture is not text"),
            Err(_) => Err("the capture could not be opened: retention may have removed it"),
        }
    } else {
        Err("an image is not written into the export's text; it is named here by its reference")
    };
    Ok(Piece {
        part: "capture",
        blob: Some(blob),
        capture: Some(capture.clone()),
        kept: None,
        content,
    })
}

/// A capture the engine kept, opened as text, or why it is left out.
async fn kept_piece(kernel: &Kernel, kept: &KeptCapture) -> Result<Piece, Failure> {
    let content = if kept.is_text() {
        match kernel.store()?.blobs().get(&kept.blob).await {
            Ok(opened) => String::from_utf8(opened.expose().to_vec())
                .map(Json::String)
                .map_err(|_| "the capture is not text"),
            Err(_) => Err("the capture could not be opened: retention may have removed it"),
        }
    } else {
        Err(
            "a capture that is not text is not written into the export; it is named here by its reference",
        )
    };
    Ok(Piece {
        part: "capture",
        blob: Some(kept.blob),
        capture: None,
        kept: Some(kept.listed()),
        content,
    })
}

/// The pieces of one run's evidence, in the order they are written.
async fn pieces(kernel: &Kernel, row: &RunRow) -> Result<(Vec<Piece>, Option<ReadTrace>), Failure> {
    let mut pieces = vec![Piece {
        part: "events",
        blob: None,
        capture: None,
        kept: None,
        content: Ok(Json::Array(shown_events(kernel, row.id).await?)),
    }];
    for kept in kept_captures(kernel, row.id).await? {
        pieces.push(kept_piece(kernel, &kept).await?);
    }
    let read = ReadTrace::of(kernel, row).await?;
    if let Some(read) = &read {
        let content = match (&read.lines, &read.text) {
            (Ok(lines), Some(_)) => {
                serde_json::to_value(lines).map_err(|_| "the trace could not be written as JSON")
            }
            (Err(why), _) => Err(*why),
            (Ok(_), None) => Err("the stored trace is not text"),
        };
        pieces.push(Piece {
            part: "trace",
            blob: Some(read.blob),
            capture: None,
            kept: None,
            content,
        });
        let captures: Vec<CaptureRef> = read
            .steps()
            .flat_map(|step| step.captures.iter().cloned())
            .collect();
        for capture in &captures {
            pieces.push(capture_piece(kernel, capture).await?);
        }
    }
    Ok((pieces, read))
}

handler!(
    /// `trace.export`: writes one run's evidence to one file in the
    /// project's working folder, `.anything2mcp/exports/<run>.json`, with a
    /// manifest of what it holds and what was left out and why.
    trace_export,
    |kernel, call| {
        let run = run_id(&call)?;
        let row = kernel.run_in_workspace(&call, run).await?;
        let folder = call.project_folder()?.to_path_buf();
        let (mut pieces, read) = pieces(kernel, &row).await?;
        // The check on the way out, once more (spec section 217): a piece
        // that still holds a registered secret is left out, never written.
        let redact = kernel.redact();
        for piece in &mut pieces {
            if let Ok(content) = &piece.content
                && redact.contains_secret(&content.to_string())
            {
                piece.content = Err("it held a registered secret at the check on the way out");
            }
        }
        let mut manifest = run_facts(&row);
        manifest.insert("format".to_owned(), json!(EXPORT_FORMAT));
        manifest.insert("format_version".to_owned(), json!(EXPORT_FORMAT_VERSION));
        if let Some(read) = &read {
            trace_facts(read, &mut manifest);
        }
        let sizes: Vec<Option<usize>> = pieces
            .iter()
            .map(|piece| {
                piece
                    .content
                    .as_ref()
                    .ok()
                    .map(|content| content.to_string().len())
            })
            .collect();
        manifest.insert(
            "pieces".to_owned(),
            Json::Array(
                pieces
                    .iter()
                    .zip(&sizes)
                    .map(|(piece, bytes)| piece.listed(*bytes))
                    .collect(),
            ),
        );
        let left_out = pieces.iter().filter(|piece| piece.content.is_err()).count();
        let captures: Vec<Json> = pieces
            .iter()
            .filter(|piece| piece.part == "capture")
            .filter_map(|piece| {
                let content = piece.content.as_ref().ok()?;
                let mut shown = piece.listed(None);
                shown
                    .as_object_mut()?
                    .insert("text".to_owned(), content.clone());
                Some(shown)
            })
            .collect();
        let part = |name: &str| {
            pieces
                .iter()
                .find(|piece| piece.part == name)
                .and_then(|piece| piece.content.as_ref().ok().cloned())
                .unwrap_or(Json::Null)
        };
        let document = json!({
            "manifest": Json::Object(manifest),
            "events": part("events"),
            "trace": part("trace"),
            "captures": captures,
        });
        let mut bytes = serde_json::to_vec_pretty(&document).map_err(|_| {
            codes::input(
                codes::SCHEMA_INVALID,
                "The export could not be written as JSON.",
                "Report this as a fault of the engine.",
            )
        })?;
        bytes.push(b'\n');
        let relative = format!(
            "{}/{EXPORTS_FOLDER}/{run}.json",
            crate::init::WORKING_FOLDER
        );
        let written = Digest::of(&bytes);
        let size = bytes.len();
        let path = relative.clone();
        // Written by way of a new file that then takes the name, never
        // through a link; then read back, and the answer gives the digest
        // only when the file holds what was written.
        let read_back = off_thread(move || {
            let file = project(&folder)?.join(&path)?;
            if let Some((parent, _)) = path.rsplit_once('/') {
                project(&folder)?.join(parent)?.create_dir_all()?;
            }
            file.write(&bytes)?;
            file.read(bytes.len() as u64 + 1)
        })
        .await?;
        if Digest::of(&read_back) != written {
            return Err(codes::stale(
                codes::EXPORT_CHANGED,
                format!(
                    "`{relative}` does not hold what was written: something changed it in between."
                ),
                "Export again, and let nothing else write in the project's working folder meanwhile.",
            ));
        }
        let mut output = Output::ok(json!({
            "run_id": run.to_string(),
            "file": relative,
            "bytes": size,
            "digest": written.to_string(),
            "pieces": pieces.len(),
            "left_out": left_out,
        }));
        if left_out > 0 {
            output = output.warn(format!(
                "{left_out} of the run's pieces of evidence were left out of the file; its manifest names each and why."
            ));
        }
        Ok(output)
    }
);
