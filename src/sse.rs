use rocket::response::stream::{Event, EventStream};
use rocket::tokio::sync::broadcast;
use serde::Serialize;

/// SSE event data sent to subscribers.
#[derive(Debug, Clone, Serialize)]
pub struct SseEvent {
    pub event_type: String,
    pub monitor_id: String,
    pub data: serde_json::Value,
}

/// Global event broadcaster. Subscribers receive all events.
pub struct EventBroadcaster {
    pub sender: broadcast::Sender<SseEvent>,
}

impl EventBroadcaster {
    pub fn new(capacity: usize) -> Self {
        let (sender, _) = broadcast::channel(capacity);
        EventBroadcaster { sender }
    }

    pub fn send(&self, event: SseEvent) {
        // Ignore send errors (no subscribers)
        let _ = self.sender.send(event);
    }
}

/// Create an SSE stream for all events.
pub fn global_stream(broadcaster: &EventBroadcaster) -> EventStream![Event + '_] {
    let mut rx = broadcaster.sender.subscribe();
    EventStream! {
        loop {
            match rx.recv().await {
                Ok(evt) => {
                    let data = serde_json::to_string(&serde_json::json!({
                        "monitor_id": evt.monitor_id,
                        "data": evt.data,
                    })).unwrap_or_default();
                    yield Event::data(data).event(evt.event_type);
                }
                Err(broadcast::error::RecvError::Lagged(n)) => {
                    yield Event::data(format!("{{\"skipped\":{}}}", n)).event("lag");
                }
                Err(broadcast::error::RecvError::Closed) => break,
            }
        }
    }
}

/// Create an SSE stream filtered to a specific monitor.
pub fn monitor_stream<'a>(broadcaster: &'a EventBroadcaster, monitor_id: String) -> EventStream![Event + 'a] {
    let mut rx = broadcaster.sender.subscribe();
    EventStream! {
        loop {
            match rx.recv().await {
                Ok(evt) if evt.monitor_id == monitor_id => {
                    let data = serde_json::to_string(&evt.data).unwrap_or_default();
                    yield Event::data(data).event(evt.event_type);
                }
                Ok(_) => continue, // Different monitor, skip
                Err(broadcast::error::RecvError::Lagged(n)) => {
                    yield Event::data(format!("{{\"skipped\":{}}}", n)).event("lag");
                }
                Err(broadcast::error::RecvError::Closed) => break,
            }
        }
    }
}
