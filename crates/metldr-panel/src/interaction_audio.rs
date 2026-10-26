use std::{cell::RefCell, rc::Rc};

use js_sys::{Math, RegExp};
use wasm_bindgen::{JsCast, JsValue, closure::Closure};
use web_sys::{
    AudioBuffer, AudioContext, AudioContextState, AudioNode, AudioScheduledSourceNode,
    BiquadFilterNode, BiquadFilterType, Document, Element, Event, GainNode, OscillatorNode,
    OscillatorType, PageTransitionEvent, Window,
};

use crate::preferences::UiPreferences;

const SELECTOR: &str = "button,a[href],input:not([type='hidden']),select,textarea,summary,[role='button'],[role='checkbox'],[role='menuitem'],[role='menuitemcheckbox'],[role='menuitemradio'],[role='option'],[role='radio'],[role='switch'],[role='tab']";
const SILENCE: f64 = 0.0001;

thread_local! {
    static LISTENER: RefCell<Option<Listener>> = const { RefCell::new(None) };
    static AUDIO: RefCell<AudioState> = const { RefCell::new(AudioState { context: None, master: None }) };
}

struct Listener {
    document: Document,
    window: Window,
    click: Closure<dyn FnMut(Event)>,
    pagehide: Closure<dyn FnMut(Event)>,
}

impl Drop for Listener {
    fn drop(&mut self) {
        let _ = self.document.remove_event_listener_with_callback_and_bool(
            "click",
            self.click.as_ref().unchecked_ref(),
            true,
        );
        let _ = self.window.remove_event_listener_with_callback(
            "pagehide",
            self.pagehide.as_ref().unchecked_ref(),
        );
    }
}

#[derive(Default)]
struct AudioState {
    context: Option<AudioContext>,
    master: Option<GainNode>,
}

#[derive(Clone, Copy)]
enum Cue {
    Press,
    Tick,
    Release,
    Page,
    Pulse,
}

#[derive(Clone, Copy)]
enum Source {
    Noise,
    Sine(f64, Option<f64>),
    Square(f64),
}

#[derive(Clone, Copy)]
enum Filter {
    Highpass(f64),
    Bandpass(f64, f64),
    Lowpass(f64, f64),
}

#[derive(Clone, Copy)]
struct Layer {
    source: Source,
    filter: Option<Filter>,
    attack: f64,
    decay: f64,
    release: f64,
    gain: f64,
}

const PRESS: [Layer; 2] = [
    Layer {
        source: Source::Noise,
        filter: Some(Filter::Highpass(2400.0)),
        attack: 0.0003,
        decay: 0.009,
        release: 0.0,
        gain: 0.13,
    },
    Layer {
        source: Source::Sine(680.0, None),
        filter: None,
        attack: 0.0006,
        decay: 0.022,
        release: 0.008,
        gain: 0.24,
    },
];
const TICK: [Layer; 1] = [Layer {
    source: Source::Square(2100.0),
    filter: Some(Filter::Bandpass(2600.0, 1.6)),
    attack: 0.0004,
    decay: 0.028,
    release: 0.0,
    gain: 0.24,
}];
const RELEASE: [Layer; 1] = [Layer {
    source: Source::Noise,
    filter: Some(Filter::Lowpass(1600.0, 0.9)),
    attack: 0.001,
    decay: 0.055,
    release: 0.0,
    gain: 0.32,
}];
const PAGE: [Layer; 1] = [Layer {
    source: Source::Sine(430.0, Some(640.0)),
    filter: None,
    attack: 0.002,
    decay: 0.11,
    release: 0.03,
    gain: 0.4,
}];
const PULSE: [Layer; 1] = [Layer {
    source: Source::Sine(330.0, None),
    filter: Some(Filter::Lowpass(2200.0, 1.0)),
    attack: 0.002,
    decay: 0.13,
    release: 0.04,
    gain: 0.5,
}];

pub(super) fn install(document: &Document, preferences: Rc<UiPreferences>) -> Result<(), JsValue> {
    if LISTENER.with(|slot| slot.borrow().is_some()) {
        return Ok(());
    }
    let window = document
        .default_view()
        .ok_or_else(|| JsValue::from_str("document has no default window"))?;
    let mut initialized = false;
    let click = Closure::wrap(Box::new(move |event: Event| {
        if preferences
            .snapshot()
            .is_ok_and(|snapshot| snapshot.sounds_enabled())
        {
            let _ = on_click(event, &mut initialized);
        }
    }) as Box<dyn FnMut(Event)>);
    let mut cleanup_pending = false;
    let cleanup_window = window.clone();
    let pagehide = Closure::wrap(Box::new(move |event: Event| {
        if event
            .dyn_ref::<PageTransitionEvent>()
            .is_some_and(PageTransitionEvent::persisted)
        {
            return;
        }
        if cleanup_pending {
            return;
        }
        cleanup_pending = true;
        let cleanup = Closure::once_into_js(|| {
            LISTENER.with(|slot| drop(slot.borrow_mut().take()));
        });
        cleanup_window.queue_microtask(cleanup.unchecked_ref::<js_sys::Function>());
    }) as Box<dyn FnMut(Event)>);
    document.add_event_listener_with_callback_and_bool(
        "click",
        click.as_ref().unchecked_ref(),
        true,
    )?;
    if let Err(error) =
        window.add_event_listener_with_callback("pagehide", pagehide.as_ref().unchecked_ref())
    {
        let _ = document.remove_event_listener_with_callback_and_bool(
            "click",
            click.as_ref().unchecked_ref(),
            true,
        );
        return Err(error);
    }
    LISTENER.with(|slot| {
        *slot.borrow_mut() = Some(Listener {
            document: document.clone(),
            window,
            click,
            pagehide,
        })
    });
    Ok(())
}

fn on_click(event: Event, initialized: &mut bool) -> Result<(), JsValue> {
    let Some(target) = event
        .target()
        .and_then(|target| target.dyn_into::<Element>().ok())
    else {
        return Ok(());
    };
    let Some(control) = target.closest(SELECTOR)? else {
        return Ok(());
    };
    if control.closest("[data-sound-silent]")?.is_some()
        || control.matches(":disabled, [aria-disabled='true']")?
    {
        return Ok(());
    }
    let cue = cue_for(&control);
    AUDIO.with(|audio| play(audio, cue, initialized))
}

fn cue_for(element: &Element) -> Cue {
    match element.get_attribute("data-sound").as_deref() {
        Some("press") => return Cue::Press,
        Some("tick") => return Cue::Tick,
        Some("release") => return Cue::Release,
        Some("page") => return Cue::Page,
        Some("pulse") => return Cue::Pulse,
        _ => {}
    }
    let label = format!(
        "{} {}",
        element.get_attribute("aria-label").unwrap_or_default(),
        element.text_content().unwrap_or_default()
    )
    .trim()
    .to_owned();
    if RegExp::new("close|dismiss|remove|delete|collapse|cancel|clear", "i").test(&label) {
        return Cue::Release;
    }
    if element.matches("input[type='checkbox'], input[type='radio'], select, [role='checkbox'], [role='radio'], [role='switch'], [role='tab'], [aria-pressed]").unwrap_or(false) { return Cue::Tick; }
    if element.matches("a[href]").unwrap_or(false) {
        return Cue::Page;
    }
    if RegExp::new("send|save|submit|calculate|create|add|upgrade|replay", "i").test(&label) {
        return Cue::Pulse;
    }
    if element.matches("input, textarea").unwrap_or(false) {
        return Cue::Tick;
    }
    Cue::Press
}

fn play(audio: &RefCell<AudioState>, cue: Cue, initialized: &mut bool) -> Result<(), JsValue> {
    let mut audio = audio.borrow_mut();
    let initialize = !*initialized;
    if initialize {
        // ensureReady() is intentionally not awaited by the reference caller.
        if let Ok(context) = get_context(&mut audio) {
            if context.state() == AudioContextState::Suspended {
                let _ = context.resume();
            }
        }
        get_master_bus(&mut audio)?.gain().set_value(0.32);
        *initialized = true;
    }
    // defineSound() and getDestination() each call getContext() in the source.
    let context = get_context(&mut audio)?;
    let destination = get_destination(&mut audio)?;
    let layers: &[Layer] = match cue {
        Cue::Press => &PRESS,
        Cue::Tick => &TICK,
        Cue::Release => &RELEASE,
        Cue::Page => &PAGE,
        Cue::Pulse => &PULSE,
    };
    let start = context.current_time();
    for layer in layers {
        play_layer(&context, &destination, *layer, start)?;
    }
    Ok(())
}

fn get_context(audio: &mut AudioState) -> Result<AudioContext, JsValue> {
    let context = match audio.context.as_ref() {
        Some(context) if context.state() != AudioContextState::Closed => context.clone(),
        _ => {
            let context = AudioContext::new()?;
            audio.master = None;
            context
        }
    };
    audio.context = Some(context.clone());
    if context.state() == AudioContextState::Suspended {
        let _ = context.resume()?;
    }
    Ok(context)
}

fn get_master_bus(audio: &mut AudioState) -> Result<GainNode, JsValue> {
    let context = get_context(audio)?;
    let master_matches = audio.master.as_ref().is_some_and(|master| {
        let context: web_sys::BaseAudioContext = context.clone().unchecked_into();
        master.context() == context
    });
    if !master_matches {
        let master = context.create_gain()?;
        audio.master = Some(master.clone());
        master.connect_with_audio_node(&context.destination())?;
    }
    Ok(audio
        .master
        .as_ref()
        .expect("master bus created above")
        .clone())
}

fn get_destination(audio: &mut AudioState) -> Result<AudioNode, JsValue> {
    let context = get_context(audio)?;
    let master_matches = audio.master.as_ref().is_some_and(|master| {
        let context: web_sys::BaseAudioContext = context.clone().unchecked_into();
        master.context() == context
    });
    Ok(if master_matches {
        audio
            .master
            .as_ref()
            .expect("matched master exists")
            .clone()
            .unchecked_into()
    } else {
        context.destination().unchecked_into()
    })
}

fn play_layer(
    context: &AudioContext,
    destination: &AudioNode,
    layer: Layer,
    start: f64,
) -> Result<(), JsValue> {
    let duration = layer.attack + layer.decay + layer.release;
    let end = start + duration;
    let envelope = context.create_gain()?;
    let gain = envelope.gain();
    gain.set_value_at_time(SILENCE as f32, start)?;
    if layer.attack > 0.0 {
        gain.linear_ramp_to_value_at_time(layer.gain as f32, start + layer.attack)?;
    } else {
        gain.set_value_at_time(layer.gain as f32, start)?;
    }
    gain.set_target_at_time(SILENCE as f32, start + layer.attack, layer.decay / 3.0)?;

    let (source, scheduled): (AudioNode, AudioNode) = match layer.source {
        Source::Noise => {
            let buffer = white_noise(context, duration + 0.1)?;
            let node = context.create_buffer_source()?;
            node.set_buffer(Some(&buffer));
            node.start_with_when(start)?;
            node.unchecked_ref::<AudioScheduledSourceNode>()
                .stop_with_when(end + 0.1)?;
            (node.clone().unchecked_into(), node.unchecked_into())
        }
        Source::Sine(frequency, sweep) => {
            let node = context.create_oscillator()?;
            node.set_type(OscillatorType::Sine);
            node.frequency()
                .set_value_at_time(frequency as f32, start)?;
            if let Some(end_frequency) = sweep {
                node.frequency()
                    .exponential_ramp_to_value_at_time(end_frequency.max(1.0) as f32, end)?;
            }
            node.start_with_when(start)?;
            node.unchecked_ref::<AudioScheduledSourceNode>()
                .stop_with_when(end + 0.1)?;
            (node.clone().unchecked_into(), node.unchecked_into())
        }
        Source::Square(frequency) => {
            let node = context.create_oscillator()?;
            node.set_type(OscillatorType::Square);
            node.frequency()
                .set_value_at_time(frequency as f32, start)?;
            node.start_with_when(start)?;
            node.unchecked_ref::<AudioScheduledSourceNode>()
                .stop_with_when(end + 0.1)?;
            (node.clone().unchecked_into(), node.unchecked_into())
        }
    };
    let mut tail = source.clone();
    let filter = if let Some(filter) = layer.filter {
        let node = context.create_biquad_filter()?;
        match filter {
            Filter::Highpass(frequency) => {
                node.set_type(BiquadFilterType::Highpass);
                set_filter(&node, frequency, 1.0, start)?;
            }
            Filter::Bandpass(frequency, resonance) => {
                node.set_type(BiquadFilterType::Bandpass);
                set_filter(&node, frequency, resonance, start)?;
            }
            Filter::Lowpass(frequency, resonance) => {
                node.set_type(BiquadFilterType::Lowpass);
                set_filter(&node, frequency, resonance, start)?;
            }
        }
        tail.connect_with_audio_node(node.as_ref())?;
        tail = node.clone().unchecked_into();
        Some(node)
    } else {
        None
    };
    tail.connect_with_audio_node(envelope.as_ref())?;
    envelope.connect_with_audio_node(destination)?;

    let scheduled: AudioScheduledSourceNode = scheduled.unchecked_into();
    let scheduled_for_cleanup = scheduled.clone();
    let disconnect_source = source.clone();
    let disconnect_envelope: AudioNode = envelope.clone().unchecked_into();
    let disconnect_filter = filter.map(|node| node.unchecked_into::<AudioNode>());
    let ended = Closure::once_into_js(move || {
        scheduled_for_cleanup.set_onended(None);
        let _ = disconnect_source.disconnect();
        let _ = disconnect_envelope.disconnect();
        if let Some(filter) = disconnect_filter {
            let _ = filter.disconnect();
        }
    });
    if let Some(function) = ended.dyn_ref::<js_sys::Function>() {
        scheduled.set_onended(Some(function));
    }
    Ok(())
}

fn set_filter(
    node: &BiquadFilterNode,
    frequency: f64,
    resonance: f64,
    start: f64,
) -> Result<(), JsValue> {
    node.frequency()
        .set_value_at_time(frequency as f32, start)?;
    node.q().set_value(resonance as f32);
    Ok(())
}

fn white_noise(context: &AudioContext, duration: f64) -> Result<AudioBuffer, JsValue> {
    let length = (context.sample_rate() as f64 * duration) as u32;
    let buffer = context.create_buffer(1, length, context.sample_rate())?;
    let samples = (0..length)
        .map(|_| (Math::random() * 2.0 - 1.0) as f32)
        .collect::<Vec<_>>();
    buffer.copy_to_channel(&samples, 0)?;
    Ok(buffer)
}
