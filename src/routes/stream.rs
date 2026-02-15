use rocket::{get, State};
use rocket::response::stream::{Event, EventStream};
use crate::sse::EventBroadcaster;
use std::sync::Arc;

#[get("/events")]
pub fn global_events(broadcaster: &State<Arc<EventBroadcaster>>) -> EventStream![Event + '_] {
    crate::sse::global_stream(broadcaster)
}

#[get("/monitors/<id>/events")]
pub fn monitor_events<'a>(id: &'a str, broadcaster: &'a State<Arc<EventBroadcaster>>) -> EventStream![Event + 'a] {
    crate::sse::monitor_stream(broadcaster, id.to_string())
}
