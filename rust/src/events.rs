use std::sync::Arc;

use basis::{Event, EventLine, EventSink};

use crate::BasisError;

/// Receives the run's event stream, one JSON line per event.
///
/// Each line is exactly what `basis spawn --json` writes: a flat object with
/// `seq` and `type`, versioned by the header's `schema_version`. Called on a
/// basis thread, in order; return quickly and hop to your own actor for UI.
/// Throwing ends the run, as a refused sink write does.
#[uniffi::export(with_foreign)]
pub trait EventObserver: Send + Sync {
    fn on_event(&self, line: String) -> Result<(), BasisError>;
}

/// Numbers events per turn and forwards them as lines.
pub(crate) struct ObserverSink {
    observer: Option<Arc<dyn EventObserver>>,
    next_seq: u64,
}

impl ObserverSink {
    pub(crate) fn new(observer: Option<Arc<dyn EventObserver>>) -> Self {
        Self {
            observer,
            next_seq: 0,
        }
    }
}

impl EventSink for ObserverSink {
    fn emit(&mut self, event: Event) -> std::io::Result<()> {
        let Some(observer) = &self.observer else {
            return Ok(());
        };
        let line = EventLine::new(self.next_seq, event);
        self.next_seq += 1;
        let encoded = serde_json::to_string(&line).map_err(std::io::Error::other)?;
        observer.on_event(encoded).map_err(std::io::Error::other)
    }
}
