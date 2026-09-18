//! The queue of not-yet-sent events, held on disk as a file of JSON lines
//! (one `Event` per line) so it survives the app closing. Every write that
//! would leave the file mid-update — eviction, [`Queue::drop_front`],
//! [`Queue::clear`] — goes through a sibling temp file and a rename, so a
//! crash partway through leaves either the old file or the new one, never a
//! half-written one. [`Queue::push`] appends directly, then evicts.

use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;

use crate::event::Event;

/// How large the queue on disk is allowed to grow before the oldest events
/// are dropped to make room. The defaults are the plan's numbers: 1,000
/// events or 1 MiB, whichever is hit first.
pub struct Limits {
    pub max_events: usize,
    pub max_bytes: u64,
}

impl Default for Limits {
    fn default() -> Limits {
        Limits {
            max_events: 1_000,
            max_bytes: 1_048_576,
        }
    }
}

/// A queue of not-yet-sent events, backed by a file of JSON lines at `path`.
pub struct Queue {
    path: PathBuf,
    limits: Limits,
}

impl Queue {
    /// Opens the queue at `path`. The file (and its parent directory) is
    /// created lazily, on first write — a `Queue` over a path that doesn't
    /// exist yet reads as empty.
    #[must_use]
    pub fn open(path: PathBuf, limits: Limits) -> Queue {
        Queue { path, limits }
    }

    /// Appends `event` as one JSON line, then evicts the oldest events if
    /// the queue is now past either limit. Refuses, without touching the
    /// file, an event whose own serialized line is already larger than
    /// `max_bytes` — eviction can only make room by dropping other events,
    /// so it can never fit one that doesn't fit on its own.
    ///
    /// # Errors
    ///
    /// Returns an error of kind [`std::io::ErrorKind::InvalidInput`] if
    /// `event`'s serialized line is larger than the queue's byte limit, or
    /// any other error if the queue's file (or its parent directory) can't
    /// be created or written to.
    pub fn push(&self, event: &Event) -> std::io::Result<()> {
        let line = serialize_line(event)?;
        let line_len = line.len() as u64;
        if line_len > self.limits.max_bytes {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!(
                    "event line is {line_len} bytes, over the queue's {}-byte limit",
                    self.limits.max_bytes
                ),
            ));
        }

        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)?;
        }

        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?;
        file.write_all(line.as_bytes())?;
        drop(file);

        self.evict()
    }

    /// Returns the oldest `n` readable events, skipping any line that
    /// doesn't parse as an `Event`.
    #[must_use]
    pub fn peek(&self, n: usize) -> Vec<Event> {
        self.readable_events().into_iter().take(n).collect()
    }

    /// Removes the first `n` readable events, and any unreadable line before
    /// or among them, by rewriting the file with what's left. Meant to be
    /// called after a batch of `peek`ed events has been sent successfully.
    ///
    /// # Errors
    ///
    /// Returns an error if the rewritten file can't be written or put in
    /// place.
    pub fn drop_front(&self, n: usize) -> std::io::Result<()> {
        let remaining: Vec<Event> = self.readable_events().into_iter().skip(n).collect();
        self.rewrite(&remaining)
    }

    /// The number of readable events currently queued. A missing file, or
    /// one with only unreadable lines, counts as zero.
    #[must_use]
    pub fn len(&self) -> usize {
        self.readable_events().len()
    }

    /// Whether the queue currently holds no readable events.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Empties the queue.
    ///
    /// # Errors
    ///
    /// Returns an error if the rewritten (empty) file can't be written or
    /// put in place.
    pub fn clear(&self) -> std::io::Result<()> {
        self.rewrite(&[])
    }

    /// Reads every line of the file, keeping the ones that parse as an
    /// `Event` and silently skipping the ones that don't. A missing file
    /// reads as empty.
    fn readable_events(&self) -> Vec<Event> {
        let Ok(file) = File::open(&self.path) else {
            return Vec::new();
        };

        BufReader::new(file)
            .lines()
            .map_while(Result::ok)
            .filter_map(|line| serde_json::from_str(&line).ok())
            .collect()
    }

    /// Rewrites the file to hold exactly `events`, one JSON line each, via a
    /// sibling temp file and `rename` — so a crash mid-write leaves either
    /// the file as it was, or the file as it will be, never half of either.
    fn rewrite(&self, events: &[Event]) -> std::io::Result<()> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)?;
        }

        let tmp_path = self.path.with_extension("tmp");
        let mut tmp = File::create(&tmp_path)?;
        for event in events {
            tmp.write_all(serialize_line(event)?.as_bytes())?;
        }
        tmp.flush()?;
        drop(tmp);

        fs::rename(&tmp_path, &self.path)
    }

    /// Drops the oldest readable events, if the file is past either limit,
    /// keeping the newest run of events that fits under both — measured by
    /// what their serialized lines will actually weigh once rewritten.
    fn evict(&self) -> std::io::Result<()> {
        let file_bytes = fs::metadata(&self.path).map_or(0, |meta| meta.len());
        let events = self.readable_events();

        if events.len() <= self.limits.max_events && file_bytes <= self.limits.max_bytes {
            return Ok(());
        }

        let lines: Vec<String> = events
            .iter()
            .map(serialize_line)
            .collect::<std::io::Result<_>>()?;

        let mut keep_from = events.len();
        let mut kept_bytes = 0u64;
        for (index, line) in lines.iter().enumerate().rev() {
            let candidate_count = events.len() - index;
            let line_len = line.len() as u64;
            if candidate_count > self.limits.max_events
                || kept_bytes + line_len > self.limits.max_bytes
            {
                break;
            }
            kept_bytes += line_len;
            keep_from = index;
        }

        self.rewrite(&events[keep_from..])
    }
}

/// Serializes `event` as a single JSON line, newline included.
fn serialize_line(event: &Event) -> std::io::Result<String> {
    let mut line = serde_json::to_string(event)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    line.push('\n');
    Ok(line)
}

#[cfg(test)]
mod tests {
    use super::{Limits, Queue};
    use crate::event::Event;
    use serde_json::Map;
    use std::fs;
    use std::io::Write;

    fn event(name: &str) -> Event {
        Event::new(name, Map::new()).unwrap()
    }

    fn uuids(events: &[Event]) -> Vec<String> {
        events.iter().map(|e| e.uuid.clone()).collect()
    }

    #[test]
    fn events_come_back_oldest_first() {
        let dir = tempfile::tempdir().unwrap();
        let queue = Queue::open(dir.path().join("queue.jsonl"), Limits::default());

        let a = event("a");
        let b = event("b");
        let c = event("c");
        queue.push(&a).unwrap();
        queue.push(&b).unwrap();
        queue.push(&c).unwrap();

        assert_eq!(uuids(&queue.peek(2)), vec![a.uuid.clone(), b.uuid.clone()]);
    }

    #[test]
    fn a_sent_batch_is_removed_from_the_front() {
        let dir = tempfile::tempdir().unwrap();
        let queue = Queue::open(dir.path().join("queue.jsonl"), Limits::default());

        let a = event("a");
        let b = event("b");
        let c = event("c");
        queue.push(&a).unwrap();
        queue.push(&b).unwrap();
        queue.push(&c).unwrap();

        queue.drop_front(2).unwrap();

        assert_eq!(uuids(&queue.peek(10)), vec![c.uuid.clone()]);
    }

    #[test]
    fn the_queue_survives_being_reopened() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("queue.jsonl");

        let queue = Queue::open(path.clone(), Limits::default());
        queue.push(&event("a")).unwrap();
        drop(queue);

        let reopened = Queue::open(path, Limits::default());
        assert_eq!(reopened.len(), 1);
    }

    #[test]
    fn past_the_event_limit_the_oldest_go_first() {
        let dir = tempfile::tempdir().unwrap();
        let limits = Limits {
            max_events: 3,
            max_bytes: u64::MAX,
        };
        let queue = Queue::open(dir.path().join("queue.jsonl"), limits);

        let events: Vec<Event> = (1..=5).map(|i| event(&format!("e{i}"))).collect();
        for e in &events {
            queue.push(e).unwrap();
        }

        assert_eq!(
            uuids(&queue.peek(10)),
            vec![
                events[2].uuid.clone(),
                events[3].uuid.clone(),
                events[4].uuid.clone(),
            ]
        );
    }

    #[test]
    fn past_the_byte_limit_the_oldest_go_first() {
        let dir = tempfile::tempdir().unwrap();

        // Two events' worth of serialized lines, measured for real rather
        // than guessed, so the limit sits just above two and just below
        // three.
        let a = event("a");
        let b = event("b");
        let c = event("c");
        let line_len = |e: &Event| serde_json::to_string(e).unwrap().len() as u64 + 1;
        let two_lines = line_len(&a) + line_len(&b);

        let limits = Limits {
            max_events: usize::MAX,
            max_bytes: two_lines + 1,
        };
        let queue = Queue::open(dir.path().join("queue.jsonl"), limits);

        queue.push(&a).unwrap();
        queue.push(&b).unwrap();
        queue.push(&c).unwrap();

        assert_eq!(queue.len(), 2);
        assert_eq!(uuids(&queue.peek(10)), vec![b.uuid.clone(), c.uuid.clone()]);
    }

    #[test]
    fn a_corrupt_line_is_skipped_not_fatal() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("queue.jsonl");

        let first = event("first");
        let second = event("second");
        let mut file = fs::File::create(&path).unwrap();
        writeln!(file, "not json").unwrap();
        writeln!(file, "{}", serde_json::to_string(&first).unwrap()).unwrap();
        writeln!(file, "{}", serde_json::to_string(&second).unwrap()).unwrap();
        drop(file);

        let queue = Queue::open(path, Limits::default());
        assert_eq!(
            uuids(&queue.peek(10)),
            vec![first.uuid.clone(), second.uuid.clone()]
        );

        queue.drop_front(1).unwrap();
        assert_eq!(uuids(&queue.peek(10)), vec![second.uuid.clone()]);
    }

    #[test]
    fn a_missing_file_is_an_empty_queue() {
        let dir = tempfile::tempdir().unwrap();
        let queue = Queue::open(dir.path().join("does-not-exist.jsonl"), Limits::default());

        assert_eq!(queue.len(), 0);
        assert!(queue.peek(5).is_empty());
    }

    #[test]
    fn clear_empties_it() {
        let dir = tempfile::tempdir().unwrap();
        let queue = Queue::open(dir.path().join("queue.jsonl"), Limits::default());

        queue.push(&event("a")).unwrap();
        queue.push(&event("b")).unwrap();
        queue.clear().unwrap();

        assert_eq!(queue.len(), 0);
    }

    #[test]
    fn push_refuses_an_event_that_cannot_fit_the_queue_at_all() {
        let dir = tempfile::tempdir().unwrap();
        let e = event("a");
        let line_len = serde_json::to_string(&e).unwrap().len() as u64 + 1;

        let limits = Limits {
            max_events: usize::MAX,
            max_bytes: line_len - 1,
        };
        let queue = Queue::open(dir.path().join("queue.jsonl"), limits);

        let err = queue.push(&e).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput);
        assert_eq!(queue.len(), 0);
    }

    #[test]
    fn an_oversized_push_leaves_earlier_events_in_place() {
        let dir = tempfile::tempdir().unwrap();
        let a = event("a");
        let b = event("b");
        let line_len = |e: &Event| serde_json::to_string(e).unwrap().len() as u64 + 1;
        let two_lines = line_len(&a) + line_len(&b);

        let limits = Limits {
            max_events: usize::MAX,
            max_bytes: two_lines,
        };
        let queue = Queue::open(dir.path().join("queue.jsonl"), limits);
        queue.push(&a).unwrap();
        queue.push(&b).unwrap();

        let mut big_properties = Map::new();
        big_properties.insert("pad".to_string(), "x".repeat(1024).into());
        let big = Event::new("big", big_properties).unwrap();
        assert!(serde_json::to_string(&big).unwrap().len() as u64 + 1 > two_lines);

        let err = queue.push(&big).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput);
        assert_eq!(uuids(&queue.peek(10)), vec![a.uuid.clone(), b.uuid.clone()]);
    }

    #[test]
    fn an_event_exactly_at_the_byte_limit_is_accepted() {
        let dir = tempfile::tempdir().unwrap();
        let e = event("a");
        let line_len = serde_json::to_string(&e).unwrap().len() as u64 + 1;

        let limits = Limits {
            max_events: usize::MAX,
            max_bytes: line_len,
        };
        let queue = Queue::open(dir.path().join("queue.jsonl"), limits);

        queue.push(&e).unwrap();
        assert_eq!(queue.len(), 1);
    }
}
