// Copyright 2026 Aravindh Sampath Kumar
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Calendar-query REPORT with server-side `<C:expand>`.
//!
//! libdav 0.10's `ListCalendarResources::with_component_and_time_range`
//! builds a calendar-query that includes a `<C:time-range>` filter, but
//! it does NOT request expansion of recurring events. For a VEVENT
//! with `RRULE:FREQ=WEEKLY;BYDAY=MO` that started in February, asking
//! Fastmail for events in late April correctly matches the master (the
//! server evaluates the filter against all recurrences) — but the
//! server returns the master's DTSTART (February), not the instance
//! dates in April.
//!
//! RFC 4791 § 9.6.5 defines `<C:expand>` as a child of `<C:calendar-data>`
//! in the property request. When present, the server pre-expands each
//! matching recurring event into one VEVENT per instance within the
//! given range, each carrying its own DTSTART/DTEND plus a
//! RECURRENCE-ID. The response is a multistatus whose `<calendar-data>`
//! bodies are the already-expanded ICS fragments — we parse them
//! through the same `ics::parse_event` pipeline as non-recurring events.
//!
//! This module implements that request type directly against libdav's
//! public `DavRequest` trait. No libdav-internal helpers touched.

use chrono::{DateTime, Utc};
use http::{response::Parts, Method};
use libdav::requests::{DavRequest, ParseResponseError, PreparedRequest};
use libdav::{FetchedResource, FetchedResourceContent};
use roxmltree::Document;

const DAV_NS: &str = "DAV:";
const CALDAV_NS: &str = "urn:ietf:params:xml:ns:caldav";

/// Request the server to expand recurring events into instances within
/// a time range.
///
/// Emits a `REPORT` with body:
///
/// ```xml
/// <C:calendar-query xmlns="DAV:" xmlns:C="urn:ietf:params:xml:ns:caldav">
///   <prop>
///     <getetag/>
///     <C:calendar-data>
///       <C:expand start="YYYYMMDDTHHMMSSZ" end="YYYYMMDDTHHMMSSZ"/>
///     </C:calendar-data>
///   </prop>
///   <C:filter>
///     <C:comp-filter name="VCALENDAR">
///       <C:comp-filter name="VEVENT">
///         <C:time-range start="..." end="..."/>
///       </C:comp-filter>
///     </C:comp-filter>
///   </C:filter>
/// </C:calendar-query>
/// ```
///
/// Returns a `Vec<FetchedResource>` (same public type libdav's
/// `GetCalendarResources` returns), so downstream parsing code
/// doesn't need to special-case the expand path.
pub struct CalendarQueryExpand<'a> {
    collection_href: &'a str,
    start_utc: String,
    end_utc: String,
}

impl<'a> CalendarQueryExpand<'a> {
    /// Build a new expand query for a calendar collection and UTC range.
    /// `start` is inclusive, `end` is exclusive.
    pub fn new(collection_href: &'a str, start: DateTime<Utc>, end: DateTime<Utc>) -> Self {
        Self {
            collection_href,
            start_utc: format_for_ics(&start),
            end_utc: format_for_ics(&end),
        }
    }
}

/// Format a UTC datetime as an iCalendar-form string (`YYYYMMDDTHHMMSSZ`).
/// Duplicated locally — there's a similar function in `caldav::utils`
/// but it's `pub(crate)` inside that module and we don't want to
/// reach into it.
fn format_for_ics(dt: &DateTime<Utc>) -> String {
    dt.format("%Y%m%dT%H%M%SZ").to_string()
}

/// Response from a [`CalendarQueryExpand`]. Shape matches libdav's
/// `GetCalendarResourcesResponse` — one `FetchedResource` per row.
#[derive(Debug)]
pub struct CalendarQueryExpandResponse {
    pub resources: Vec<FetchedResource>,
}

impl DavRequest for CalendarQueryExpand<'_> {
    type Response = CalendarQueryExpandResponse;
    type ParseError = ParseResponseError;
    type Error<E> = libdav::dav::WebDavError<E>;

    fn prepare_request(&self) -> Result<PreparedRequest, http::Error> {
        // String-built XML: the body is static-shaped and we only
        // interpolate two attribute values (which are restricted to
        // digits + 'T'/'Z' by our own serializer — no user input), so
        // we don't need a full XML writer. Escaping is not required
        // for this shape.
        let body = format!(
            r#"<C:calendar-query xmlns="DAV:" xmlns:C="urn:ietf:params:xml:ns:caldav">
  <prop>
    <getetag/>
    <C:calendar-data>
      <C:expand start="{s}" end="{e}"/>
    </C:calendar-data>
  </prop>
  <C:filter>
    <C:comp-filter name="VCALENDAR">
      <C:comp-filter name="VEVENT">
        <C:time-range start="{s}" end="{e}"/>
      </C:comp-filter>
    </C:comp-filter>
  </C:filter>
</C:calendar-query>"#,
            s = self.start_utc,
            e = self.end_utc
        );

        Ok(PreparedRequest {
            method: Method::from_bytes(b"REPORT")?,
            path: self.collection_href.to_string(),
            body,
            headers: vec![
                ("Depth".to_string(), "1".to_string()),
                (
                    "Content-Type".to_string(),
                    r#"application/xml; charset="utf-8""#.to_string(),
                ),
            ],
        })
    }

    fn parse_response(
        &self,
        parts: &Parts,
        body: &[u8],
    ) -> Result<Self::Response, Self::ParseError> {
        if !parts.status.is_success() {
            return Err(ParseResponseError::BadStatusCode(parts.status));
        }
        let body_str = std::str::from_utf8(body)?; // NotUtf8 via From
        let resources = parse_multistatus_with_calendar_data(body_str)
            .map_err(ParseResponseError::InvalidResponse)?;
        Ok(CalendarQueryExpandResponse { resources })
    }
}

/// Parse a DAV multistatus whose responses carry `<C:calendar-data>`.
///
/// Skips responses without a success status (HTTP 4xx/5xx on a
/// specific href) but records them as `Err(status)` on the
/// corresponding `FetchedResource`, matching libdav's own semantic.
fn parse_multistatus_with_calendar_data(xml: &str) -> Result<Vec<FetchedResource>, String> {
    let doc = Document::parse(xml).map_err(|e| format!("xml parse: {e}"))?;
    let root = doc.root_element();
    if root.tag_name().name() != "multistatus" || root.tag_name().namespace() != Some(DAV_NS) {
        return Err(format!(
            "expected root <multistatus> in DAV namespace, got <{}>",
            root.tag_name().name()
        ));
    }

    let mut out = Vec::new();
    for response in root.children().filter(is_dav_response) {
        let Some(href) = find_child(response, DAV_NS, "href").and_then(text_of) else {
            continue; // malformed; ignore this row rather than failing the whole batch
        };

        let Some(propstat) = find_child(response, DAV_NS, "propstat") else {
            out.push(FetchedResource {
                href,
                content: Err(http::StatusCode::INTERNAL_SERVER_ERROR),
            });
            continue;
        };
        let status = find_child(propstat, DAV_NS, "status")
            .and_then(text_of)
            .and_then(|s| parse_status_line(&s))
            .unwrap_or(http::StatusCode::OK);
        if !status.is_success() {
            out.push(FetchedResource {
                href,
                content: Err(status),
            });
            continue;
        }
        let Some(prop) = find_child(propstat, DAV_NS, "prop") else {
            out.push(FetchedResource {
                href,
                content: Err(http::StatusCode::INTERNAL_SERVER_ERROR),
            });
            continue;
        };
        let etag = find_child(prop, DAV_NS, "getetag")
            .and_then(text_of)
            .unwrap_or_default();
        let data = find_child(prop, CALDAV_NS, "calendar-data")
            .and_then(text_of)
            .unwrap_or_default();

        out.push(FetchedResource {
            href,
            content: Ok(FetchedResourceContent { data, etag }),
        });
    }
    Ok(out)
}

fn is_dav_response(n: &roxmltree::Node) -> bool {
    n.is_element() && n.tag_name().name() == "response" && n.tag_name().namespace() == Some(DAV_NS)
}

fn find_child<'a, 'input>(
    parent: roxmltree::Node<'a, 'input>,
    ns: &str,
    name: &str,
) -> Option<roxmltree::Node<'a, 'input>> {
    parent.children().find(|c| {
        c.is_element() && c.tag_name().name() == name && c.tag_name().namespace() == Some(ns)
    })
}

fn text_of(n: roxmltree::Node) -> Option<String> {
    // `calendar-data` can contain escaped newlines + have leading/trailing
    // whitespace from pretty-printed XML. Trim and return owned.
    n.text().map(|s| s.trim().to_owned())
}

/// Parse `"HTTP/1.1 200 OK"` → `StatusCode::OK`. Tolerant of spacing.
fn parse_status_line(line: &str) -> Option<http::StatusCode> {
    line.split_whitespace()
        .nth(1)
        .and_then(|code| code.parse::<u16>().ok())
        .and_then(|n| http::StatusCode::from_u16(n).ok())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn prepare_request_includes_expand_and_time_range() {
        let start = Utc.with_ymd_and_hms(2026, 4, 27, 0, 0, 0).unwrap();
        let end = Utc.with_ymd_and_hms(2026, 4, 28, 0, 0, 0).unwrap();
        let req = CalendarQueryExpand::new("/cal/x/", start, end);
        let prepared = req.prepare_request().unwrap();

        assert_eq!(prepared.method.as_str(), "REPORT");
        assert_eq!(prepared.path, "/cal/x/");
        assert!(
            prepared.body.contains("C:expand"),
            "body: {}",
            prepared.body
        );
        assert!(prepared.body.contains("C:time-range"));
        // Both start/end should appear in the body (in expand + time-range).
        let starts = prepared.body.matches("20260427T000000Z").count();
        let ends = prepared.body.matches("20260428T000000Z").count();
        assert_eq!(starts, 2, "start must appear in expand + time-range");
        assert_eq!(ends, 2, "end must appear in expand + time-range");
        // Depth and content-type headers.
        assert!(
            prepared
                .headers
                .iter()
                .any(|(k, v)| k == "Depth" && v == "1"),
            "headers: {:?}",
            prepared.headers
        );
    }

    #[test]
    fn parse_multistatus_returns_expanded_instance() {
        // One master gym event expanded into three Monday instances in
        // April. The server gives each a RECURRENCE-ID and a DTSTART
        // matching the specific week. We should surface all three as
        // separate FetchedResources with their distinct ICS bodies.
        let xml = r#"<?xml version="1.0" encoding="utf-8"?>
<multistatus xmlns="DAV:" xmlns:C="urn:ietf:params:xml:ns:caldav">
  <response>
    <href>/cal/gym.ics</href>
    <propstat>
      <prop>
        <getetag>"etag-apr6"</getetag>
        <C:calendar-data>BEGIN:VCALENDAR
BEGIN:VEVENT
UID:gym-master
RECURRENCE-ID;TZID=Europe/Berlin:20260406T080000
DTSTART;TZID=Europe/Berlin:20260406T080000
DTEND;TZID=Europe/Berlin:20260406T090000
SUMMARY:Gym
END:VEVENT
END:VCALENDAR</C:calendar-data>
      </prop>
      <status>HTTP/1.1 200 OK</status>
    </propstat>
  </response>
  <response>
    <href>/cal/gym.ics</href>
    <propstat>
      <prop>
        <getetag>"etag-apr13"</getetag>
        <C:calendar-data>BEGIN:VCALENDAR
BEGIN:VEVENT
UID:gym-master
RECURRENCE-ID;TZID=Europe/Berlin:20260413T080000
DTSTART;TZID=Europe/Berlin:20260413T080000
DTEND;TZID=Europe/Berlin:20260413T090000
SUMMARY:Gym
END:VEVENT
END:VCALENDAR</C:calendar-data>
      </prop>
      <status>HTTP/1.1 200 OK</status>
    </propstat>
  </response>
</multistatus>"#;

        let rows = parse_multistatus_with_calendar_data(xml).unwrap();
        assert_eq!(rows.len(), 2);
        for row in &rows {
            assert_eq!(row.href, "/cal/gym.ics");
            let content = row.content.as_ref().expect("success row");
            assert!(content.data.contains("RECURRENCE-ID"));
            assert!(content.data.contains("Gym"));
        }
    }

    #[test]
    fn parse_multistatus_records_http_errors_per_row() {
        // A 404 on a specific href should be surfaced as Err(status),
        // not drop the whole batch.
        let xml = r#"<?xml version="1.0" encoding="utf-8"?>
<multistatus xmlns="DAV:" xmlns:C="urn:ietf:params:xml:ns:caldav">
  <response>
    <href>/cal/missing.ics</href>
    <propstat>
      <prop><getetag/></prop>
      <status>HTTP/1.1 404 Not Found</status>
    </propstat>
  </response>
</multistatus>"#;
        let rows = parse_multistatus_with_calendar_data(xml).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(
            rows[0].content.as_ref().err().copied(),
            Some(http::StatusCode::NOT_FOUND)
        );
    }

    #[test]
    fn parse_multistatus_rejects_non_multistatus_root() {
        let xml = "<not-multistatus/>";
        let err = parse_multistatus_with_calendar_data(xml).unwrap_err();
        assert!(err.contains("multistatus"));
    }

    #[test]
    fn parse_status_line_reads_200_and_404() {
        assert_eq!(
            parse_status_line("HTTP/1.1 200 OK"),
            Some(http::StatusCode::OK)
        );
        assert_eq!(
            parse_status_line("HTTP/1.1 404 Not Found"),
            Some(http::StatusCode::NOT_FOUND)
        );
        assert_eq!(parse_status_line("garbage"), None);
    }
}
