//! Explainers re-pinned after a late capability probe.

use super::*;

struct ReplanExplainer {
    fallback: bool,
}

impl Explainer for ReplanExplainer {
    fn language(&self) -> Option<&'static str> {
        Some(if self.fallback { "English" } else { "Burmese" })
    }

    fn compatible_snapshot(&self) -> Option<Arc<dyn Explainer>> {
        (!self.fallback).then(|| Arc::new(Self { fallback: true }) as Arc<dyn Explainer>)
    }

    fn explain(
        &self,
        _: &str,
        _: &str,
        partial: &(dyn Fn(WordInfo) + Sync),
    ) -> Result<WordInfo, Failure> {
        if self.fallback {
            Ok(info("fallback"))
        } else {
            partial(info("old partial"));
            Err(Why::Language.into())
        }
    }
}

#[test]
fn a_late_language_failure_retries_and_reports_the_actual_language() {
    let (wake, _) = asked();
    let got = ask(
        wake,
        Arc::new(ReplanExplainer { fallback: false }),
        ("fox".into(), "the fox".into(), None),
    );
    assert!(matches!(got.rx.recv().unwrap(), Got::Partial(_)));
    assert!(matches!(got.rx.recv().unwrap(), Got::Reset));
    let Got::Done(Ok(said), language) = got.rx.recv().unwrap() else {
        panic!("fallback answer");
    };
    assert_eq!(said.info.meaning, "fallback");
    assert_eq!(language, Some("English"));
}

#[test]
fn an_inline_fallback_is_filed_with_the_retried_language() {
    use crate::study::state::State;
    use crate::study::words::Asked;
    use mareader_library::Spot;

    let (wake, _) = asked();
    let mut main = Library::new(DeviceId(1));
    let mut state = State::default();
    state.set_inline(true);
    state.set_explainer(Arc::new(ReplanExplainer { fallback: false }));
    let input = Asked {
        word: "fox".into(),
        context: "the fox".into(),
        page: 0,
        chars: 4..7,
        spot: Some(Spot::Page {
            page: 0,
            rect: [1.0, 1.0, 10.0, 10.0],
        }),
    };
    let key = state.explain_mark(&mut main, None, "book", &input, 1);
    state.prepare_card(&mut main, wake, key, &input.word, &input.context);
    let filed = state.take_filed();
    assert_eq!(filed.len(), 1);
    assert_eq!(filed[0].1.info.meaning, "fallback");
    assert_eq!(filed[0].2, Some("English"));
}
