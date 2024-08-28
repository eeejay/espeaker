#[cfg(test)]
mod tests {
    use espeaker::{list_voices, Event, Gender, Speaker};
    use rodio::Source;
    use std::cell::Cell;

    macro_rules! assert_within {
        ($left:expr, $right:expr, $range:expr $(,)?) => {{
            match (&$left, &$right, &$range) {
                (left_val, right_val, range_val) => {
                    assert!(
                        *left_val <= (*right_val).saturating_add(*range_val)
                            && *left_val >= (*right_val).saturating_sub(*range_val),
                        "{} should be within {} of {}",
                        $left,
                        $range,
                        $right
                    )
                }
            }
        }};
    }

    #[test]
    fn source_iterates() {
        let mut speaker = Speaker::new();
        // speaker.set_voice(&voices[4]);
        let source = speaker.speak("Hello, world");
        let count = source.count();
        assert_within!(count, 21748usize, 500);

        // Higher speech rate generates less samples
        speaker.params.rate = Some(400);
        let source = speaker.speak("Hello, world");
        let count = source.count();
        assert_within!(count, 6902usize, 500);
    }
    #[test]
    fn synth_twice_without_crashing() {
        let speaker = Speaker::new();
        let source = speaker.speak("Hello, world");
        let count = source.count();
        assert_within!(count, 21748usize, 500);
        let source = speaker.speak("Goodbye");
        let count = source.count();
        assert_within!(count, 11888usize, 500);
    }
    #[test]
    fn has_samplerate() {
        let speaker = Speaker::new();
        let source = speaker.speak("Hello, world");
        assert_eq!(22050, source.sample_rate());
    }

    #[test]
    fn has_voices() {
        let mut found = false;
        for voice in list_voices() {
            if voice.name == "French (Switzerland)" {
                found = true;
                assert_eq!(voice.identifier, "roa/fr-CH");
                assert_eq!(voice.age, 0);
                assert_eq!(voice.gender, Gender::Male);
                assert_eq!(voice.languages.len(), 2);
                let lang = voice.languages.get(0).unwrap();
                assert_eq!(lang.name, "fr-ch");
                assert_eq!(lang.priority, 5);
                let lang = voice.languages.get(1).unwrap();
                assert_eq!(lang.name, "fr");
                assert_eq!(lang.priority, 8);
            }
        }
        assert!(found);
    }

    #[test]
    fn callbacks_called() {
        let mut events = Vec::<(usize, Event)>::new();
        let current_sample: Cell<usize> = Cell::new(0);
        let cb = |evt| {
            events.push((current_sample.get(), evt));
        };
        let speaker = Speaker::new();
        let source = speaker
            .speak("Hello world. Goodbye world")
            .with_callback(cb);
        for _sample in source {
            current_sample.set(current_sample.get() + 1);
        }
        let expected = [
            (0usize, Event::Start),
            (0usize, Event::Sentence(0)),
            (0usize, Event::Word(0, 5)),
            (6769usize, Event::Word(6, 5)),
            (22675usize, Event::Sentence(13)),
            (22675usize, Event::Word(13, 7)),
            (31355usize, Event::Word(21, 5)),
            (40786usize, Event::End),
        ];

        assert_eq!(events.len(), expected.len());
        for (i, (at_sample, event)) in events.iter().enumerate() {
            assert_eq!(*event, expected[i].1);
            assert_within!(*at_sample, expected[i].0, 25);
        }
    }
}
