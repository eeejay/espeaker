// The MIT License (MIT)
//
// Copyright (c) 2022 Eitan Isaacson <eitan@monotonous.org>
//
// Permission is hereby granted, free of charge, to any person obtaining a copy of
// this software and associated documentation files (the "Software"), to deal in
// the Software without restriction, including without limitation the rights to
// use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of
// the Software, and to permit persons to whom the Software is furnished to do so,
// subject to the following conditions:

// The above copyright notice and this permission notice shall be included in all
// copies or substantial portions of the Software.

// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
// IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS
// FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR
// COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER
// IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN
// CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.

//! eSpeak NG playback library
//!
//! The main use of this library is to create and configure a [`Speaker`]
//! which in turn creates a [`SpeakerSource`] that implements a [`rodio::Source`].
//!
//! For example, here is how you would synthesize a simple phrase:
//! ```no_run
//! use rodio::{OutputStream, Sink};
//!
//! let speaker = espeaker::Speaker::new();
//! let source = speaker.speak("Hello, world!");
//! let (_stream, stream_handle) = OutputStream::try_default().unwrap();
//! let sink = Sink::try_new(&stream_handle).unwrap();
//! sink.append(source);
//! sink.sleep_until_end();
//! ```
//!
//! You can tweak the speaker's parameters via [`Speaker::params`].
//! Each change will only affect the given speaker. This is unlike
//! eSpeak NG's API where a parameter change is global:
//! ```no_run
//! let mut speaker = espeaker::Speaker::new();
//! speaker.params.pitch = Some(400);
//! speaker.params.rate = Some(80);
//! ```
//!
//! This library also supports callbacks that can be used when certain
//! speech landmarks like words or sentences are spoken.
//! Use the [`SpeakerSource::with_callback`] method to create a new source
//! that dispatches the callback:
//! ```no_run
//! let mut speaker = espeaker::Speaker::new();
//! speaker.params.rate = Some(280);
//! let source = speaker.speak("Hello world, goodbye!");
//! let source = source.with_callback(move |evt| match evt {
//!     espeaker::Event::Word(start, _len) => {
//!         println!("'Word at {}'", start);
//!     }
//!     espeaker::Event::Sentence(_) => (),
//!     espeaker::Event::Start => {
//!         println!("'Start!")
//!     }
//!     espeaker::Event::End => {
//!         println!("'End!");
//!     }
//! });
//! ```
pub use rodio;

use espeakng_sys::*;
use rodio::Source;
use std::ffi::{c_void, CStr, CString};
use std::os::raw::{c_char, c_int, c_short};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::OnceLock;
use std::sync::{Mutex, MutexGuard};
use std::thread;
use std::time::Duration;

fn espeak_init() -> &'static Mutex<u32> {
    static ESPEAK_INIT: OnceLock<Mutex<u32>> = OnceLock::new();
    ESPEAK_INIT.get_or_init(|| Mutex::new(0))
}

fn init() -> u32 {
    let mut lock = espeak_init().plock();
    if *lock == 0 {
        *lock = unsafe {
            espeak_Initialize(
                espeak_AUDIO_OUTPUT_AUDIO_OUTPUT_SYNCHRONOUS,
                0,
                std::ptr::null(),
                0,
            )
            .try_into()
            .unwrap()
        };
    }
    *lock
}

#[derive(Debug, PartialEq)]
pub enum Gender {
    Female,
    Male,
    NonBinary,
}

#[allow(dead_code)]
#[derive(Debug)]
pub struct Language {
    pub priority: i8,
    pub name: String,
}

#[allow(dead_code)]
#[derive(Debug)]
pub struct Voice {
    pub name: String,
    pub identifier: String,
    pub age: u8,
    pub gender: Gender,
    pub languages: Vec<Language>,
}

impl Voice {
    pub(crate) fn from_espeak_voice(v: espeak_VOICE) -> Voice {
        let name = if v.name.is_null() {
            String::default()
        } else {
            let name_cstr = unsafe { CStr::from_ptr(v.name) };
            String::from(name_cstr.to_str().unwrap())
        };

        let identifier = if v.identifier.is_null() {
            String::default()
        } else {
            let identifier_cstr = unsafe { CStr::from_ptr(v.identifier) };
            String::from(identifier_cstr.to_str().unwrap())
        };

        let age: u8 = v.age;

        let gender = match v.gender {
            1 => Gender::Male,
            2 => Gender::Female,
            _ => Gender::NonBinary,
        };

        let mut languages = Vec::<Language>::new();
        if !v.languages.is_null() {
            let mut langs_ptr = v.languages;
            while unsafe { *langs_ptr != 0 } {
                // get priority byte
                let priority = unsafe { *langs_ptr };
                langs_ptr = langs_ptr.wrapping_add(1);
                let lang_cstr = unsafe { CStr::from_ptr(langs_ptr) };
                let name = String::from(lang_cstr.to_str().unwrap());
                let name_len = name.bytes().count();
                languages.push(Language {
                    priority: priority.try_into().unwrap(),
                    name,
                });
                langs_ptr = langs_ptr.wrapping_add(name_len + 1);
            }
        }

        Voice {
            name,
            identifier,
            age,
            gender,
            languages,
        }
    }
}

pub fn list_voices() -> Vec<Voice> {
    init();
    {
        let _lock = espeak_init().plock();
        let mut result = Vec::<Voice>::new();
        let mut voice_arr = unsafe { espeak_ListVoices(std::ptr::null_mut()) };

        while unsafe { !(*voice_arr).is_null() } {
            let voice = unsafe { Voice::from_espeak_voice(**voice_arr) };
            result.push(voice);
            voice_arr = voice_arr.wrapping_add(1);
        }
        result
    }
}

#[derive(Debug, PartialEq)]
pub enum Event {
    Start,
    Word(usize, usize),
    Sentence(usize),
    End,
}

#[derive(Clone)]
pub struct SpeakerParams {
    pub rate: Option<i32>,
    pub volume: Option<i32>,
    pub pitch: Option<i32>,
    pub range: Option<i32>,
    pub punctuation: Option<i32>,
    pub capitals: Option<i32>,
    pub word_gap: Option<i32>,
    pub is_ssml: bool,
}

impl SpeakerParams {
    pub fn new() -> SpeakerParams {
        SpeakerParams {
            rate: None,
            volume: None,
            pitch: None,
            range: None,
            punctuation: None,
            capitals: None,
            word_gap: None,
            is_ssml: false,
        }
    }

    pub(crate) fn apply_params(self: SpeakerParams) {
        fn apply_param(param_enum: u32, value: Option<i32>) {
            unsafe {
                match value {
                    Some(value) => espeak_SetParameter(param_enum, value, 0),
                    None => espeak_SetParameter(param_enum, espeak_GetParameter(param_enum, 0), 0),
                };
            };
        }

        apply_param(espeak_PARAMETER_espeakRATE, self.rate);
        apply_param(espeak_PARAMETER_espeakVOLUME, self.volume);
        apply_param(espeak_PARAMETER_espeakPITCH, self.pitch);
        apply_param(espeak_PARAMETER_espeakRANGE, self.range);
        apply_param(espeak_PARAMETER_espeakPUNCTUATION, self.punctuation);
        apply_param(espeak_PARAMETER_espeakCAPITALS, self.capitals);
        apply_param(espeak_PARAMETER_espeakWORDGAP, self.word_gap);
    }
}

pub struct Speaker {
    pub params: SpeakerParams,
    voice_name: String,
}

impl Speaker {
    pub fn new() -> Speaker {
        Speaker {
            params: SpeakerParams::new(),
            voice_name: String::default(),
        }
    }

    pub fn speak(&self, text: &str) -> SpeakerSource {
        SpeakerSource::new(text, &self.voice_name, self.params.clone())
    }

    pub fn set_voice(&mut self, voice: &Voice) {
        self.voice_name = voice.name.clone();
    }
}

pub struct SpeakerSource {
    rx: Receiver<(Vec<i16>, Vec<(u32, Event)>)>,
    sample_rate: u32,
    data: Vec<i16>,
    events: Vec<(u32, Event)>,
    iter_index: Option<usize>,
}

impl SpeakerSource {
    pub fn new(text: &str, voice_name: &str, params: SpeakerParams) -> SpeakerSource {
        let (mut tx, rx) = channel::<(Vec<i16>, Vec<(u32, Event)>)>();
        let sample_rate = init();

        let voice_name_cstr = CString::new(if voice_name.is_empty() {
            "en"
        } else {
            voice_name
        })
        .expect("Failed to convert &str to CString");
        let text_cstr = CString::new(text).expect("Failed to convert &str to CString");
        thread::spawn(move || {
            let _lock = espeak_init().plock();
            let flags = if params.is_ssml {
                espeakSSML | espeakCHARS_AUTO
            } else {
                espeakCHARS_AUTO
            };
            params.apply_params();
            let tx_ptr: *mut c_void = &mut tx as *mut _ as *mut c_void;

            unsafe {
                espeak_SetVoiceByName(voice_name_cstr.as_ptr() as *const c_char);
            }

            unsafe {
                espeak_SetSynthCallback(Some(Self::synth_callback));
            }

            let position = 0u32;
            let position_type: espeak_POSITION_TYPE = 0;
            let end_position = 0u32;

            let identifier = std::ptr::null_mut();
            unsafe {
                espeak_Synth(
                    text_cstr.as_ptr() as *const c_void,
                    500,
                    position,
                    position_type,
                    end_position,
                    flags,
                    identifier,
                    tx_ptr,
                );
            }
        });

        SpeakerSource {
            rx,
            sample_rate,
            data: Vec::new(),
            events: Vec::new(),
            iter_index: Some(0),
        }
    }

    pub fn with_callback<F>(self, callback: F) -> SpeakerSourceWithCallback<F>
    where
        F: FnMut(Event),
    {
        SpeakerSourceWithCallback {
            inner: self,
            callback,
        }
    }

    pub fn iter_audio_and_events(self) -> IterAudioAndEvents {
        IterAudioAndEvents { inner: self }
    }

    fn next_sample_and_events(&mut self) -> (Option<i16>, Option<Vec<Event>>) {
        match self.iter_index {
            None => (None, None),
            Some(i) => {
                while i >= self.data.len() {
                    match self.rx.recv() {
                        Err(_) => {
                            return (None, Some(vec![Event::End]));
                        }
                        Ok((mut wav_vec, mut events_vec)) => {
                            self.data.append(&mut wav_vec);
                            self.events.append(&mut events_vec);
                        }
                    }
                }
                let mut events = Vec::<Event>::new();
                while let Some((audio_position, _)) = self.events.first() {
                    let at_sample = (audio_position * self.sample_rate / 1000) as usize;
                    if at_sample > i {
                        break;
                    }
                    let (_, event) = self.events.remove(0);
                    events.push(event);
                }

                let sample = if i < self.data.len() {
                    self.iter_index = Some(i + 1usize);
                    Some(self.data[i])
                } else {
                    None
                };
                (
                    sample,
                    if events.is_empty() {
                        None
                    } else {
                        Some(events)
                    },
                )
            }
        }
    }

    #[allow(non_upper_case_globals)]
    #[allow(non_snake_case)]
    extern "C" fn synth_callback(
        wav: *mut c_short,
        sample_count: c_int,
        events: *mut espeak_EVENT,
    ) -> c_int {
        let mut events_copy = events.clone();
        let mut events_vec = Vec::<(u32, Event)>::new();
        while unsafe { (*events_copy).type_ != espeak_EVENT_TYPE_espeakEVENT_LIST_TERMINATED } {
            // let at_sample = audio_position * self.sample_rate * 1000;
            let evt = match unsafe { (*events_copy).type_ } {
                espeak_EVENT_TYPE_espeakEVENT_SAMPLERATE => {
                    // This is the start of the utterance
                    Some(Event::Start)
                }
                espeak_EVENT_TYPE_espeakEVENT_WORD => {
                    let text_position: usize =
                        unsafe { (*events_copy).text_position.try_into().unwrap() };
                    let length: usize = unsafe { (*events_copy).length.try_into().unwrap() };
                    Some(Event::Word(text_position.saturating_sub(1), length))
                }
                espeak_EVENT_TYPE_espeakEVENT_SENTENCE => {
                    let text_position: usize =
                        unsafe { (*events_copy).text_position.try_into().unwrap() };
                    Some(Event::Sentence(text_position.saturating_sub(1)))
                }
                _ => None,
            };
            if let Some(evt) = evt {
                let audio_position: u32 =
                    unsafe { (*events_copy).audio_position.try_into().unwrap() };
                events_vec.push((audio_position, evt));
            }
            events_copy = events_copy.wrapping_add(1);
        }

        let tx_ptr = unsafe { (*events).user_data };
        let tx: &mut Sender<(Vec<i16>, Vec<(u32, Event)>)> =
            unsafe { &mut *(tx_ptr as *mut Sender<(Vec<i16>, Vec<(u32, Event)>)>) };
        let mut wav_vec: Vec<i16> = Vec::new();
        if !wav.is_null() {
            let wav_slice = unsafe { std::slice::from_raw_parts(wav, sample_count as usize) };
            wav_vec = wav_slice
                .into_iter()
                .map(|f| f.clone() as i16)
                .collect::<Vec<i16>>();
        }
        match tx.send((wav_vec, events_vec)) {
            Err(_) => 1,
            Ok(_) => 0,
        }
    }
}

impl Source for SpeakerSource {
    fn current_frame_len(&self) -> Option<usize> {
        None
    }

    fn channels(&self) -> u16 {
        1
    }

    fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    fn total_duration(&self) -> Option<Duration> {
        None
    }
}

impl Iterator for SpeakerSource {
    type Item = i16;

    fn next(&mut self) -> Option<i16> {
        let (sample, _) = self.next_sample_and_events();
        return sample;
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        (0, None)
    }
}

pub struct SpeakerSourceWithCallback<F> {
    inner: SpeakerSource,
    callback: F,
}

impl<F> SpeakerSourceWithCallback<F> where F: FnMut(Event) {}

impl<F> Source for SpeakerSourceWithCallback<F>
where
    F: FnMut(Event),
{
    fn current_frame_len(&self) -> Option<usize> {
        self.inner.current_frame_len()
    }

    fn channels(&self) -> u16 {
        self.inner.channels()
    }

    fn sample_rate(&self) -> u32 {
        self.inner.sample_rate()
    }

    fn total_duration(&self) -> Option<Duration> {
        self.inner.total_duration()
    }
}

impl<F> Iterator for SpeakerSourceWithCallback<F>
where
    F: FnMut(Event),
{
    type Item = i16;

    fn next(&mut self) -> Option<i16> {
        let (sample, events) = self.inner.next_sample_and_events();

        match events {
            None => (),
            Some(events) => {
                for event in events {
                    (self.callback)(event);
                }
            }
        }

        return sample;
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.inner.size_hint()
    }
}

pub struct IterAudioAndEvents {
    inner: SpeakerSource,
}

impl Iterator for IterAudioAndEvents {
    type Item = (i16, Option<Vec<Event>>);

    fn next(&mut self) -> Option<(i16, Option<Vec<Event>>)> {
        let (sample, events) = self.inner.next_sample_and_events();

        match sample {
            None => None,
            Some(sample) => Some((sample, events)),
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.inner.size_hint()
    }
}

trait PoisonlessLock<T> {
    fn plock(&self) -> MutexGuard<T>;
}

impl<T> PoisonlessLock<T> for Mutex<T> {
    fn plock(&self) -> MutexGuard<T> {
        match self.lock() {
            Ok(l) => l,
            Err(e) => e.into_inner(),
        }
    }
}
