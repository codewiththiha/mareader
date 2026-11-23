#![allow(clippy::expect_used, clippy::unwrap_used)]

use std::sync::Mutex;

use super::*;
use crate::backend::block;

fn model(at_once: u8) -> ModelPreset {
    ModelPreset {
        id: PresetId(1),
        name: "Fast".into(),
        provider: Provider::Ollama,
        at_once,
        ..ModelPreset::default()
    }
}

#[test]
fn a_full_model_turns_try_away_and_a_dropped_ticket_frees_it() {
    let (slots, p) = (Slots::default(), model(1));
    let first = slots.try_take(&p, TaskId::Explain);
    assert!(first.is_some());
    assert!(slots.try_take(&p, TaskId::Explain).is_none());
    let other = ModelPreset {
        id: PresetId(2),
        ..model(1)
    };
    assert!(slots.try_take(&other, TaskId::Explain).is_some());
    assert_eq!(slots.in_use(p.id), 1);
    drop(first);
    assert_eq!(slots.in_use(p.id), 0);
    let two = model(2);
    let held: Vec<_> = (0..3)
        .filter_map(|_| slots.try_take(&two, TaskId::Explain))
        .collect();
    assert_eq!(held.len(), 2, "room for two holds two");
}

#[test]
fn work_made_ahead_leaves_the_last_place_free() {
    let (slots, p) = (Slots::default(), model(2));
    let deck = slots.try_take(&p, TaskId::StudyCards);
    assert!(deck.is_some());
    assert!(slots.try_take(&p, TaskId::Organize).is_none());
    assert!(slots.try_take(&p, TaskId::Explain).is_some());
    // With room for one, it takes the only place.
    let one = ModelPreset {
        id: PresetId(3),
        ..model(1)
    };
    assert!(slots.try_take(&one, TaskId::StudyCards).is_some());
}

#[test]
fn the_reader_s_task_goes_first_then_arrival() {
    let (slots, p) = (Slots::default(), model(1));
    let held = slots.try_take(&p, TaskId::Organize);
    let order = std::sync::Arc::new(Mutex::new(Vec::new()));
    block(async {
        let mut jobs = Vec::new();
        for task in [TaskId::StudyCards, TaskId::Organize, TaskId::Explain] {
            let (slots, p, order) = (slots.clone(), p.clone(), order.clone());
            jobs.push(tokio::spawn(async move {
                let ticket = slots.take(&p, task, None, &|_| {}).await;
                order.lock().unwrap().push(task);
                drop(ticket);
            }));
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert_eq!(slots.queued(p.id), 3, "all three wait");
        assert!(slots.waited_for(p.id), "Explain is in line");
        drop(held);
        for job in jobs {
            job.await.unwrap();
        }
    });
    let order = order.lock().unwrap().clone();
    let first = [TaskId::Explain, TaskId::StudyCards, TaskId::Organize];
    assert_eq!(order, first);
}

#[test]
fn a_class_the_reader_picked_decides_the_order() {
    let (slots, p) = (Slots::default(), model(1));
    let held = slots.try_take(&p, TaskId::Explain);
    let order = std::sync::Arc::new(Mutex::new(Vec::new()));
    let soon = Asker {
        task: TaskId::Organize,
        class: Class::Waited,
    };
    let later = Asker {
        task: TaskId::Explain,
        class: Class::Ahead,
    };
    block(async {
        let mut jobs = Vec::new();
        for who in [later, soon] {
            let (slots, p, order) = (slots.clone(), p.clone(), order.clone());
            jobs.push(tokio::spawn(async move {
                let ticket = slots.take(&p, who, None, &|_| {}).await;
                order.lock().unwrap().push(who.task);
                drop(ticket);
            }));
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert!(
            slots.waited_for(p.id),
            "the organizer, picked Right away, waits"
        );
        drop(held);
        for job in jobs {
            job.await.unwrap();
        }
    });
    let order = order.lock().unwrap().clone();
    assert_eq!(order, [TaskId::Organize, TaskId::Explain]);
}

#[test]
fn a_fuse_gives_up_and_leaves_the_line() {
    let (slots, p) = (Slots::default(), model(1));
    let _held = slots.try_take(&p, TaskId::Explain);
    let ahead = Mutex::new(None);
    let fuse = Some(Duration::from_millis(50));
    let say = |n| *ahead.lock().unwrap() = Some(n);
    let got = block(slots.take(&p, TaskId::StudyJudge, fuse, &say));
    assert!(got.is_none());
    assert_eq!(*ahead.lock().unwrap(), Some(1), "one runs before it");
    assert_eq!(slots.queued(p.id), 0, "it left the line");
}

#[test]
fn waiting_behind_another_task_is_news_and_behind_its_own_is_not() {
    let (slots, p) = (Slots::default(), model(1));
    let held = slots.try_take(&p, TaskId::StudyCards);
    let fuse = Some(Duration::from_millis(10));
    let _ = block(slots.take(&p, TaskId::StudyCards, fuse, &|_| {}));
    assert!(slots.news().is_empty(), "a deck behind a deck");
    let _ = block(slots.take(&p, TaskId::Explain, fuse, &|_| {}));
    let news = slots.news();
    assert_eq!(news.len(), 1);
    let said = news[0].said();
    let told = "“Fast” is writing study cards and runs 1 task at a time. \
        Explaining a word starts when it's free.";
    assert_eq!(said, told);
    assert!(slots.news().is_empty(), "taken once");
    drop(held);
}

#[test]
fn room_is_at_once_per_usable_key_once_counted() {
    let slots = Slots::default();
    let keyed = ModelPreset {
        provider: Provider::Custom,
        ..model(2)
    };
    assert_eq!(slots.room(&keyed), 2, "one lane until counted");
    slots.count(Provider::Custom, 3);
    assert_eq!(slots.room(&keyed), 6);
    slots.count(Provider::Ollama, 3);
    assert_eq!(slots.room(&model(2)), 2, "a keyless server has one lane");
}
