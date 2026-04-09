use super::*;
use std::time::{Duration, Instant};

const MAX_HIGH_PRIORITY_DEFERRALS: usize = 3;

#[derive(Debug)]
pub(super) struct LspScheduler {
    background_mode: bool,
    canceled_request_ids: BTreeSet<String>,
    pending_diagnostics: Option<PendingDiagnostics>,
    next_revision: u64,
    diagnostic_debounce: Duration,
}

#[derive(Debug)]
struct PendingDiagnostics {
    revision: u64,
    due_at: Instant,
    notifications: Vec<Value>,
    high_priority_deferrals: usize,
}

impl LspScheduler {
    pub(super) fn new(debounce_ms: u64) -> Self {
        Self {
            background_mode: false,
            canceled_request_ids: BTreeSet::new(),
            pending_diagnostics: None,
            next_revision: 0,
            diagnostic_debounce: Duration::from_millis(debounce_ms),
        }
    }

    pub(super) fn enable_background_mode(&mut self) {
        self.background_mode = true;
    }

    pub(super) fn disable_background_mode(&mut self) {
        self.background_mode = false;
        self.pending_diagnostics = None;
        self.canceled_request_ids.clear();
    }

    pub(super) fn schedule_diagnostics(&mut self, notifications: Vec<Value>) {
        self.next_revision += 1;
        self.pending_diagnostics = Some(PendingDiagnostics {
            revision: self.next_revision,
            due_at: Instant::now() + self.diagnostic_debounce,
            notifications,
            high_priority_deferrals: 0,
        });
    }

    pub(super) fn immediate_or_deferred_notifications(&mut self) -> Vec<Value> {
        if self.background_mode { Vec::new() } else { self.flush_all() }
    }

    pub(super) fn next_wait_duration(&self) -> Option<Duration> {
        let pending = self.pending_diagnostics.as_ref()?;
        Some(pending.due_at.saturating_duration_since(Instant::now()))
    }

    pub(super) fn flush_due_after(&mut self, method: &str) -> Vec<Value> {
        if !self.background_mode {
            return Vec::new();
        }
        let Some(pending) = self.pending_diagnostics.as_mut() else {
            return Vec::new();
        };
        if Instant::now() < pending.due_at {
            return Vec::new();
        }
        if is_high_priority_method(method)
            && pending.high_priority_deferrals < MAX_HIGH_PRIORITY_DEFERRALS
        {
            pending.high_priority_deferrals += 1;
            return Vec::new();
        }
        self.flush_all()
    }

    pub(super) fn flush_due_timeout(&mut self) -> Vec<Value> {
        if self.pending_diagnostics.as_ref().is_some_and(|pending| Instant::now() >= pending.due_at)
        {
            return self.flush_all();
        }
        Vec::new()
    }

    pub(super) fn flush_all(&mut self) -> Vec<Value> {
        self.pending_diagnostics
            .take()
            .map(|pending| {
                let _revision = pending.revision;
                pending.notifications
            })
            .unwrap_or_default()
    }

    pub(super) fn cancel_request(&mut self, id: &Value) {
        if let Some(key) = request_id_key(id) {
            self.canceled_request_ids.insert(key);
        }
    }

    pub(super) fn take_cancellation(&mut self, id: Option<&Value>) -> bool {
        let Some(id) = id else {
            return false;
        };
        let Some(key) = request_id_key(id) else {
            return false;
        };
        self.canceled_request_ids.remove(&key)
    }
}

impl Default for LspScheduler {
    fn default() -> Self {
        Self::new(40)
    }
}

fn request_id_key(id: &Value) -> Option<String> {
    if let Some(number) = id.as_i64() {
        return Some(format!("n:{number}"));
    }
    id.as_str().map(|text| format!("s:{text}"))
}

fn is_high_priority_method(method: &str) -> bool {
    matches!(
        method,
        "textDocument/hover"
            | "textDocument/completion"
            | "textDocument/signatureHelp"
            | "textDocument/definition"
            | "textDocument/references"
            | "textDocument/rename"
            | "textDocument/codeAction"
            | "textDocument/documentSymbol"
            | "workspace/symbol"
            | "textDocument/formatting"
    )
}
