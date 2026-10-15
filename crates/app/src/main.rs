mod client;
mod hid;
mod sensors;
mod settings;
mod tray;
mod view;

use std::collections::VecDeque;
use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use dell5530_proto::Fan;
use iced::{time, window, Subscription, Task, Theme};

use sensors::{Reader, Sample};
use settings::{Accent, Brightness, LedMode, PowerMode, Settings};
use tray::TrayEvent;

pub const ICON_PNG: &[u8] = include_bytes!("../../../assets/images/alien-square.png");
const HISTORY: usize = 120;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Page {
    Lighting,
    Performance,
    Settings,
}

#[derive(Clone, Debug)]
pub enum Helper {
    Checking,
    Ready(String),
    Unsupported(String),
    Unavailable(String),
}

impl Helper {
    pub fn is_ready(&self) -> bool {
        matches!(self, Helper::Ready(_))
    }
}

#[derive(Clone, Debug)]
pub enum Message {
    Page(Page),
    LedMode(LedMode),
    Rgb(usize, u8),
    Preset([u8; 3]),
    Brightness(Brightness),
    Duration(u16),
    ApplyLed,
    LedDone(Result<(), String>),
    TrayLedDone(Result<(), String>, bool),
    Power(PowerMode),
    PowerDone(Result<String, String>),
    FanChanged(Fan, u8),
    FanReleased(Fan),
    FanDone(Fan, Result<String, String>),
    LiveFan(bool),
    Probe,
    Probed(Result<client::Probe, String>),
    LedVersion(Result<String, String>),
    SensorTick(Instant),
    Sampled(Sample),
    AutoRefresh(bool),
    Accent(Accent),
    CloseToTray(bool),
    TrayPoll(Instant),
    WindowId(Option<window::Id>),
    CloseRequested(window::Id),
}

/// The LED fields that can be edited before being applied.
#[derive(Clone, PartialEq)]
pub struct LedFields {
    mode: LedMode,
    rgb: [u8; 3],
    duration: u16,
    brightness: Brightness,
}

impl LedFields {
    fn of(s: &Settings) -> Self {
        Self { mode: s.led_mode, rgb: s.rgb, duration: s.duration, brightness: s.brightness }
    }
    /// Fields that don't affect the chosen mode are ignored when checking for changes.
    fn relevant(&self) -> (LedMode, Option<[u8; 3]>, Option<u16>, Option<Brightness>) {
        match self.mode {
            LedMode::Static => (self.mode, Some(self.rgb), None, Some(self.brightness)),
            LedMode::Morph => (self.mode, None, Some(self.duration), Some(self.brightness)),
            LedMode::Off => (self.mode, None, None, None),
        }
    }
}

pub struct App {
    pub settings: Settings,
    pub applied: LedFields,
    pub theme: Theme,
    pub page: Page,
    pub helper: Helper,
    pub led_firmware: String,
    pub led_status: String,
    pub power_status: String,
    pub last_acpi: String,
    pub sample: Option<Sample>,
    pub history: [VecDeque<f32>; 4],
    pub tray_ok: bool,
    pub visible: bool,
    window: Option<window::Id>,
    reader: Arc<Mutex<Option<Reader>>>,
    sampling: bool,
    fan_busy: [bool; 2],
    fan_sent: [u8; 2],
    fan_released: [bool; 2],
    tray_rx: Option<Receiver<TrayEvent>>,
    _tray: Option<ksni::blocking::Handle<tray::Tray>>,
}

fn fan_idx(f: Fan) -> usize {
    usize::from(f == Fan::Gpu)
}

async fn blocking<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> T {
    tokio::task::spawn_blocking(f).await.expect("blocking task panicked")
}

pub fn make_theme(accent: Accent) -> Theme {
    let [r, g, b] = accent.rgb();
    Theme::custom(
        "Gaming",
        iced::theme::Palette {
            background: iced::Color::from_rgb8(0x0a, 0x0c, 0x12),
            text: iced::Color::from_rgb8(0xe6, 0xea, 0xf2),
            primary: iced::Color::from_rgb8(r, g, b),
            success: iced::Color::from_rgb8(0x3d, 0xdc, 0x84),
            warning: iced::Color::from_rgb8(0xff, 0xb4, 0x4d),
            danger: iced::Color::from_rgb8(0xff, 0x4d, 0x5e),
        },
    )
}

impl App {
    fn boot() -> (Self, Task<Message>) {
        let settings = Settings::load();
        let (tray_handle, tray_rx) = match tray::spawn() {
            Some((h, rx)) => (Some(h), Some(rx)),
            None => (None, None),
        };
        let app = App {
            applied: LedFields::of(&settings),
            theme: make_theme(settings.accent),
            fan_sent: [settings.fan_cpu, settings.fan_gpu],
            settings,
            page: Page::Lighting,
            helper: Helper::Checking,
            led_firmware: "Unknown".into(),
            led_status: "Idle".into(),
            power_status: "Select a power mode or adjust fan boost.".into(),
            last_acpi: "None".into(),
            sample: None,
            history: Default::default(),
            tray_ok: tray_handle.is_some(),
            visible: true,
            window: None,
            reader: Arc::new(Mutex::new(None)),
            sampling: false,
            fan_busy: [false; 2],
            fan_released: [false; 2],
            tray_rx,
            _tray: tray_handle,
        };
        let tasks = Task::batch([
            window::oldest().map(Message::WindowId),
            Task::perform(blocking(client::probe), Message::Probed),
            Task::perform(blocking(hid::version), Message::LedVersion),
        ]);
        (app, tasks)
    }

    fn led_fields(&self) -> LedFields {
        LedFields::of(&self.settings)
    }

    pub fn led_dirty(&self) -> bool {
        self.led_fields().relevant() != self.applied.relevant()
    }

    fn persist(&self) {
        let mut s = self.settings.clone();
        s.led_mode = self.applied.mode;
        s.rgb = self.applied.rgb;
        s.duration = self.applied.duration;
        s.brightness = self.applied.brightness;
        s.save();
    }

    fn send_fan(&mut self, fan: Fan) -> Task<Message> {
        let i = fan_idx(fan);
        let value = if i == 0 { self.settings.fan_cpu } else { self.settings.fan_gpu };
        if self.fan_busy[i] || self.fan_sent[i] == value {
            return Task::none();
        }
        self.fan_busy[i] = true;
        self.fan_sent[i] = value;
        Task::perform(blocking(move || client::set_fan_boost(fan, value)), move |r| Message::FanDone(fan, r))
    }

    fn show_window(&mut self) -> Task<Message> {
        self.visible = true;
        match self.window {
            Some(id) => Task::batch([window::set_mode(id, window::Mode::Windowed), window::gain_focus(id)]),
            None => Task::none(),
        }
    }

    fn toggle_leds(&mut self) -> Task<Message> {
        if self.settings.led_on {
            return Task::perform(blocking(|| hid::set_dim(100)), |r| Message::TrayLedDone(r, false));
        }
        let mode = if self.settings.last_led_mode == LedMode::Off {
            LedMode::Static
        } else {
            self.settings.last_led_mode
        };
        let a = self.applied.clone();
        Task::perform(
            blocking(move || hid::apply(mode, a.rgb, a.duration, a.brightness)),
            |r| Message::TrayLedDone(r, true),
        )
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Page(p) => self.page = p,
            Message::LedMode(m) => self.settings.led_mode = m,
            Message::Rgb(i, v) => self.settings.rgb[i] = v,
            Message::Preset(c) => self.settings.rgb = c,
            Message::Brightness(b) => self.settings.brightness = b,
            Message::Duration(d) => self.settings.duration = d,
            Message::ApplyLed => {
                let f = self.led_fields();
                self.led_status = "Applying...".into();
                return Task::perform(
                    blocking(move || hid::apply(f.mode, f.rgb, f.duration, f.brightness)),
                    Message::LedDone,
                );
            }
            Message::LedDone(Ok(())) => {
                self.applied = self.led_fields();
                self.settings.led_on = self.applied.mode != LedMode::Off;
                if self.settings.led_on {
                    self.settings.last_led_mode = self.applied.mode;
                }
                self.led_status = "Applied successfully".into();
                self.persist();
            }
            Message::LedDone(Err(e)) => {
                if self.led_fields().mode == LedMode::Off && e.contains("No supported device") {
                    self.led_status = "LED device not found; treated as off".into();
                    self.settings.led_on = false;
                } else {
                    self.led_status = format!("Apply failed: {e}");
                }
            }
            Message::TrayLedDone(Ok(()), on) => {
                self.settings.led_on = on;
                self.led_status = if on { "Restored saved LED mode" } else { "LEDs turned off" }.into();
                self.persist();
            }
            Message::TrayLedDone(Err(e), _) => self.led_status = format!("Tray toggle failed: {e}"),
            Message::Power(mode) => {
                self.settings.power = mode;
                self.settings.fan_cpu = 0;
                self.settings.fan_gpu = 0;
                self.fan_sent = [0, 0];
                self.persist();
                return Task::perform(blocking(move || client::set_power_mode(mode)), Message::PowerDone);
            }
            Message::PowerDone(r) => match r {
                Ok(m) => {
                    self.last_acpi = m.clone();
                    self.power_status = m;
                }
                Err(e) => {
                    self.last_acpi = format!("error: {e}");
                    self.power_status = format!("Power mode change failed: {e}");
                }
            },
            Message::FanChanged(fan, v) => {
                if fan == Fan::Cpu {
                    self.settings.fan_cpu = v;
                } else {
                    self.settings.fan_gpu = v;
                }
                if self.settings.live_fan {
                    return self.send_fan(fan);
                }
            }
            Message::FanReleased(fan) => {
                self.fan_released[fan_idx(fan)] = true;
                return self.send_fan(fan);
            }
            Message::FanDone(fan, r) => {
                let i = fan_idx(fan);
                self.fan_busy[i] = false;
                match r {
                    Ok(m) => {
                        self.last_acpi = m.clone();
                        self.power_status = m;
                    }
                    Err(e) => {
                        self.last_acpi = format!("error: {e}");
                        self.power_status = format!("Fan boost failed: {e}");
                    }
                }
                self.persist();
                if self.settings.live_fan || self.fan_released[i] {
                    self.fan_released[i] = false;
                    return self.send_fan(fan);
                }
            }
            Message::LiveFan(on) => {
                self.settings.live_fan = on;
                self.persist();
            }
            Message::Probe => {
                self.helper = Helper::Checking;
                return Task::perform(blocking(client::probe), Message::Probed);
            }
            Message::Probed(r) => {
                self.helper = match r {
                    Ok(p) if p.supported => Helper::Ready(p.model),
                    Ok(p) => Helper::Unsupported(p.model),
                    Err(e) => Helper::Unavailable(e),
                };
            }
            Message::LedVersion(r) => self.led_firmware = r.unwrap_or_else(|_| "Unavailable".into()),
            Message::SensorTick(_) => {
                let wanted = self.visible
                    && self.settings.auto_refresh
                    && self.page == Page::Performance
                    && !self.sampling;
                if wanted {
                    self.sampling = true;
                    let reader = Arc::clone(&self.reader);
                    let ready = self.helper.is_ready();
                    return Task::perform(
                        blocking(move || {
                            let mut guard = reader.lock().unwrap_or_else(|e| e.into_inner());
                            guard.get_or_insert_with(Reader::new).sample(ready)
                        }),
                        Message::Sampled,
                    );
                }
            }
            Message::Sampled(s) => {
                self.sampling = false;
                if s.error.is_none() && self.helper.is_ready() {
                    push(&mut self.history[0], s.cpu_temp as f32);
                    push(&mut self.history[1], s.gpu_temp as f32);
                }
                if let Some(w) = s.cpu_watts {
                    push(&mut self.history[2], w);
                }
                if let Some(w) = s.gpu_watts {
                    push(&mut self.history[3], w);
                }
                if let Some(e) = &s.error {
                    self.last_acpi = format!("error: {e}");
                }
                self.sample = Some(s);
            }
            Message::AutoRefresh(on) => {
                self.settings.auto_refresh = on;
                self.persist();
            }
            Message::Accent(a) => {
                self.settings.accent = a;
                self.theme = make_theme(a);
                self.persist();
            }
            Message::CloseToTray(on) => {
                self.settings.close_to_tray = on;
                self.persist();
            }
            Message::TrayPoll(_) => {
                let mut events = Vec::new();
                if let Some(rx) = &self.tray_rx {
                    while let Ok(ev) = rx.try_recv() {
                        events.push(ev);
                    }
                }
                let mut tasks = Vec::new();
                if self.window.is_none() {
                    tasks.push(window::oldest().map(Message::WindowId));
                }
                for ev in events {
                    tasks.push(match ev {
                        TrayEvent::ToggleLeds => self.toggle_leds(),
                        TrayEvent::Show => self.show_window(),
                        TrayEvent::Quit => iced::exit(),
                    });
                }
                return Task::batch(tasks);
            }
            Message::WindowId(id) => self.window = id.or(self.window),
            Message::CloseRequested(id) => {
                if self.settings.close_to_tray && self.tray_ok {
                    self.visible = false;
                    return window::set_mode(id, window::Mode::Hidden);
                }
                return iced::exit();
            }
        }
        Task::none()
    }

    fn subscription(&self) -> Subscription<Message> {
        Subscription::batch([
            time::every(Duration::from_millis(500)).map(Message::SensorTick),
            time::every(Duration::from_millis(200)).map(Message::TrayPoll),
            window::close_requests().map(Message::CloseRequested),
        ])
    }
}

fn push(q: &mut VecDeque<f32>, v: f32) {
    if q.len() == HISTORY {
        q.pop_front();
    }
    q.push_back(v);
}

fn main() -> iced::Result {
    let icon = window::icon::from_file_data(ICON_PNG, None).ok();
    iced::application(App::boot, App::update, view::view)
        .title("Dell G15 5530 Controller")
        .theme(|app: &App| app.theme.clone())
        .subscription(App::subscription)
        .antialiasing(true)
        .exit_on_close_request(false)
        .window(window::Settings {
            size: iced::Size::new(980.0, 680.0),
            min_size: Some(iced::Size::new(860.0, 600.0)),
            icon,
            platform_specific: window::settings::PlatformSpecific {
                application_id: "dell5530-controller".into(),
                ..Default::default()
            },
            ..Default::default()
        })
        .run()
}
