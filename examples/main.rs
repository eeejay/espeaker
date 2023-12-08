use espeaker;
use rodio::{OutputStream, Sink};

fn main() {
    let s = String::from(
        "I was going to smoke the marijuana like a cigarette. I shall hide behind the couch.",
    );
    let mut speaker = espeaker::Speaker::new();
    speaker.params.pitch = Some(400);
    let source = speaker.speak(&s);
    let source = source.with_callback(move |evt| match evt {
        espeaker::Event::Start => {
            println!("START!");
        }
        espeaker::Event::Word(start, len) => {
            println!("'{}'", &s[..][start..(start + len)]);
        }
        espeaker::Event::Sentence(_) => (),
        espeaker::Event::End => {
            println!("END!");
        }
    });
    let (_stream, stream_handle) = OutputStream::try_default().unwrap();
    let sink = Sink::try_new(&stream_handle).unwrap();

    sink.append(source);
    sink.sleep_until_end();

    let s = String::from(
        "I was going to smoke the marijuana like a cigarette. I shall hide behind the couch.",
    );
    let mut speaker = espeaker::Speaker::new();
    speaker.params.rate = Some(280);
    let source = speaker.speak(&s);
    let source = source.with_callback(move |evt| match evt {
        espeaker::Event::Start => {
            println!("START!");
        }
        espeaker::Event::Word(start, len) => {
            println!("'{}'", &s[..][start..(start + len)]);
        }
        espeaker::Event::Sentence(_) => (),
        espeaker::Event::End => {
            println!("END!");
        }
    });
    let (_stream, stream_handle) = OutputStream::try_default().unwrap();
    let sink = Sink::try_new(&stream_handle).unwrap();

    sink.append(source);
    sink.sleep_until_end();

    let s = String::from("كنت سأدخن الماريجوانا مثل السيجارة. سأختبئ خلف الأريكة.");
    let mut speaker = espeaker::Speaker::new();

    let voices = espeaker::list_voices();
    let voice = voices.into_iter().find(|v| v.name == "Arabic").unwrap();
    speaker.set_voice(&voice);
    let source = speaker.speak(&s);
    let source = source.with_callback(move |evt| match evt {
        espeaker::Event::Start => {
            println!("START!");
        }
        espeaker::Event::Word(start, len) => {
            println!("{} {}", start, len);
        }
        espeaker::Event::Sentence(_) => (),
        espeaker::Event::End => {
            println!("END!");
        }
    });
    let (_stream, stream_handle) = OutputStream::try_default().unwrap();
    let sink = Sink::try_new(&stream_handle).unwrap();

    sink.append(source);
    sink.sleep_until_end();
}
