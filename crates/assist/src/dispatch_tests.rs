#![allow(clippy::expect_used, clippy::unwrap_used)]

use std::time::Duration;

use mareader_settings::{PresetId, Provider};
use serde_json::json;

use super::*;

fn runtime() -> Runtime {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build();
    rt.expect("a runtime")
}

fn quiet() -> Waker {
    Arc::new(|| {})
}

fn prompt() -> Prompt {
    Prompt {
        name: "t",
        system: String::new(),
        user: String::new(),
        schema: json!({}),
    }
}

/// Answers in turn, repeating the last; logs `name` per ask.
struct Turns {
    name: &'static str,
    turns: Mutex<Vec<Result<Value, Failure>>>,
    log: Arc<Mutex<Vec<&'static str>>>,
}

impl Backend for Turns {
    fn ask<'a>(&'a self, _: &'a Prompt, _: Partial<'a>) -> BoxFuture<'a, Result<Value, Failure>> {
        self.log.lock().unwrap().push(self.name);
        let mut turns = self.turns.lock().unwrap();
        let next = match turns.len() {
            0 | 1 => turns.first().cloned(),
            _ => Some(turns.remove(0)),
        };
        Box::pin(async move { next.unwrap_or(Err(Why::NotHere.into())) })
    }
}

type Log = Arc<Mutex<Vec<&'static str>>>;

fn seat(id: u32, name: &'static str, turns: Vec<Result<Value, Failure>>, log: &Log) -> Seat {
    let preset = ModelPreset {
        id: PresetId(id),
        name: name.into(),
        provider: Provider::Ollama,
        ..ModelPreset::default()
    };
    let backend = Turns {
        name,
        turns: Mutex::new(turns),
        log: Arc::clone(log),
    };
    Seat {
        preset,
        backend: Arc::new(backend),
    }
}

/// Asks `route` for `id` once per model, as a task's one request.
fn ask(rt: &Runtime, slots: &Slots, id: TaskId, route: Vec<Seat>) -> Running<Value> {
    let d = Dispatch {
        rt: Some(rt),
        waker: &quiet(),
        slots,
    };
    d.ask(id, route, |backend: Arc<dyn Backend>, _, _| async move {
        backend.ask(&prompt(), &|_| {}).await
    })
}

#[test]
fn a_waited_job_falls_through_its_route_and_names_every_model() {
    let (rt, slots, log) = (runtime(), Slots::default(), Log::default());
    let busy = seat(1, "A", vec![Err(Why::Busy.into())], &log);
    let away = seat(2, "B", vec![Err(Why::Offline.into())], &log);
    let failed = ask(&rt, &slots, TaskId::StudyJudge, vec![busy, away]).wait();
    let f = failed.expect_err("both fail");
    assert_eq!(f.why, Why::Offline, "the last model's failure");
    assert_eq!(f.detail.as_deref(), Some("asked “A”, then “B”"));
    let ok = seat(3, "C", vec![Ok(json!(1))], &log);
    let busy = seat(1, "A", vec![Err(Why::Busy.into())], &log);
    let got = ask(&rt, &slots, TaskId::Explain, vec![busy, ok]).wait();
    assert_eq!(got, Ok(json!(1)));
    // A refusal is the model's answer; the next isn't asked.
    let no = seat(1, "A", vec![Err(Why::Refused.into())], &log);
    let ok = seat(3, "C", vec![Ok(json!(1))], &log);
    let got = ask(&rt, &slots, TaskId::Explain, vec![no, ok]).wait();
    assert_eq!(got.map_err(|f| f.why), Err(Why::Refused));
}

#[test]
fn other_jobs_ask_only_their_first_model() {
    let (rt, slots, log) = (runtime(), Slots::default(), Log::default());
    let busy = seat(1, "A", vec![Err(Why::Busy.into())], &log);
    let ok = seat(2, "B", vec![Ok(json!(1))], &log);
    let got = ask(&rt, &slots, TaskId::StudyCards, vec![busy, ok]).wait();
    assert_eq!(got.map_err(|f| f.why), Err(Why::Busy));
    assert_eq!(*log.lock().unwrap(), ["A"]);
    let none = ask(&rt, &slots, TaskId::Organize, Vec::new()).wait();
    let said = none.expect_err("no model").said();
    assert!(said.contains("organizing your library"), "{said}");
}

#[test]
fn the_job_hears_where_it_runs_and_each_ask_again() {
    let (rt, slots, log) = (runtime(), Slots::default(), Log::default());
    let turns = vec![Err(Why::Busy.into()), Ok(json!(2))];
    let route = vec![seat(1, "A", turns, &log)];
    let d = Dispatch {
        rt: Some(&rt),
        waker: &quiet(),
        slots: &slots,
    };
    let mut job: Running<Value> = d.ask(TaskId::Organize, route, |b, _, _| async move {
        let _ = b.ask(&prompt(), &|_| {}).await;
        b.ask(&prompt(), &|_| {}).await
    });
    let mut seen = Vec::new();
    let started = std::time::Instant::now();
    while !matches!(seen.last(), Some(Event::Done(_))) {
        assert!(started.elapsed() < Duration::from_secs(10), "{seen:?}");
        seen.extend(job.poll());
        std::thread::sleep(Duration::from_millis(5));
    }
    let by = Event::Started { by: "A".into() };
    let again = Event::Again {
        why: Why::Busy,
        attempt: 2,
    };
    assert_eq!(seen, [by, again, Event::Done(Ok(json!(2)))]);
}

#[test]
fn a_flow_steps_aside_between_requests_for_a_waited_job() {
    let (rt, slots, log) = (runtime(), Slots::default(), Log::default());
    let organizer = seat(1, "flow", vec![Ok(json!(1))], &log);
    let preset = organizer.preset.clone();
    let d = Dispatch {
        rt: Some(&rt),
        waker: &quiet(),
        slots: &slots,
    };
    let watch = slots.clone();
    let job: Running<Value> = d.flow(TaskId::Organize, Some(organizer), move |b, _| async move {
        let _ = b.ask(&prompt(), &|_| {}).await;
        while !watch.waited_for(PresetId(1)) {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        b.ask(&prompt(), &|_| {}).await
    });
    let started = std::time::Instant::now();
    while log.lock().unwrap().is_empty() {
        assert!(started.elapsed() < Duration::from_secs(10), "the flow asks");
        std::thread::sleep(Duration::from_millis(5));
    }
    let (explain, log2) = (slots.clone(), Arc::clone(&log));
    let waited = rt.spawn(async move {
        let ticket = explain.take(&preset, TaskId::Explain, None, &|_| {}).await;
        log2.lock().unwrap().push("explain");
        drop(ticket);
    });
    assert_eq!(job.wait(), Ok(json!(1)));
    rt.block_on(waited).unwrap();
    assert_eq!(*log.lock().unwrap(), ["flow", "explain", "flow"]);
}
