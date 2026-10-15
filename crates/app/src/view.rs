use std::collections::VecDeque;

use dell5530_proto::Fan;
use iced::widget::{button, canvas, column, container, row, scrollable, slider, text, toggler, Space};
use iced::{
    gradient, mouse, Alignment, Background, Border, Color, Element, Length, Point, Radians, Rectangle,
    Renderer, Shadow, Theme, Vector,
};

use crate::settings::{Accent, Brightness, LedMode, PowerMode};
use crate::tuning::PowerMizer;
use crate::{App, Helper, Message, Page, PerfTab};
use dell5530_proto::Governor;

const DIM: Color = Color::from_rgb(0.54, 0.58, 0.65);
const PRESETS: [[u8; 3]; 9] = [
    [255, 0, 0],
    [255, 128, 0],
    [255, 220, 0],
    [0, 255, 80],
    [0, 220, 255],
    [0, 100, 255],
    [140, 60, 255],
    [255, 60, 200],
    [255, 255, 255],
];

fn rgb(c: [u8; 3]) -> Color {
    Color::from_rgb8(c[0], c[1], c[2])
}

fn card(_: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(Color::from_rgb8(0x13, 0x17, 0x21))),
        border: Border { color: Color::from_rgb8(0x24, 0x2b, 0x3b), width: 1.0, radius: 14.0.into() },
        ..Default::default()
    }
}

fn sidebar_style(_: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(Color::from_rgb8(0x0d, 0x10, 0x17))),
        border: Border { color: Color::from_rgb8(0x1b, 0x21, 0x2e), width: 1.0, radius: 0.0.into() },
        ..Default::default()
    }
}

fn root_style(theme: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(theme.palette().background)),
        ..Default::default()
    }
}

fn chip(selected: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |theme, status| {
        let primary = theme.palette().primary;
        let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
        let (bg, border, fg) = if selected {
            (Color { a: 0.16, ..primary }, primary, primary)
        } else if hovered {
            (Color::from_rgb8(0x1d, 0x23, 0x32), Color { a: 0.7, ..primary }, theme.palette().text)
        } else {
            (Color::from_rgb8(0x18, 0x1d, 0x29), Color::from_rgb8(0x2a, 0x32, 0x45), theme.palette().text)
        };
        let fade = if matches!(status, button::Status::Disabled) { 0.4 } else { 1.0 };
        button::Style {
            background: Some(Background::Color(Color { a: bg.a * fade, ..bg })),
            text_color: Color { a: fg.a * fade, ..fg },
            border: Border { color: border, width: 1.0, radius: 10.0.into() },
            ..Default::default()
        }
    }
}

fn swatch(color: Color, selected: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |theme, status| {
        let hovered = matches!(status, button::Status::Hovered);
        let border = if selected {
            Color::WHITE
        } else if hovered {
            theme.palette().primary
        } else {
            Color::from_rgb8(0x2a, 0x32, 0x45)
        };
        button::Style {
            background: Some(Background::Color(color)),
            border: Border { color: border, width: if selected { 2.5 } else { 1.0 }, radius: 14.0.into() },
            ..Default::default()
        }
    }
}

fn heading<'a>(s: &'a str) -> Element<'a, Message> {
    text(s).size(13).color(DIM).into()
}

fn segmented<'a, T: Copy + PartialEq + 'a>(
    items: &[T],
    selected: T,
    label: fn(T) -> &'static str,
    msg: fn(T) -> Message,
) -> Element<'a, Message> {
    row(items.iter().map(|&it| {
        button(text(label(it)).size(14))
            .padding([8, 16])
            .style(chip(it == selected))
            .on_press(msg(it))
            .into()
    }))
    .spacing(8)
    .into()
}

fn panel<'a>(content: impl Into<Element<'a, Message>>) -> container::Container<'a, Message> {
    container(content).padding(20).width(Length::Fill).style(card)
}

pub fn view(app: &App) -> Element<'_, Message> {
    let page = match app.page {
        Page::Lighting => lighting(app),
        Page::Performance => performance(app),
        Page::Settings => settings(app),
    };
    container(row![sidebar(app), container(page).padding(24).width(Length::Fill).height(Length::Fill)])
        .style(root_style)
        .into()
}

fn sidebar(app: &App) -> Element<'_, Message> {
    let nav = |label: &'static str, page: Page| {
        button(text(label).size(15))
            .width(Length::Fill)
            .padding([11, 14])
            .style(chip(app.page == page))
            .on_press(Message::Page(page))
    };
    let (dot, status) = match &app.helper {
        Helper::Checking => (Color::from_rgb8(0xff, 0xb4, 0x4d), "Checking helper...".to_string()),
        Helper::Ready(m) => (Color::from_rgb8(0x3d, 0xdc, 0x84), m.clone()),
        Helper::Unsupported(m) => (Color::from_rgb8(0xff, 0xb4, 0x4d), m.clone()),
        Helper::Unavailable(_) => (Color::from_rgb8(0xff, 0x4d, 0x5e), "Helper offline".to_string()),
    };
    let status_row = row![
        container(Space::new()).width(8).height(8).style(move |_| container::Style {
            background: Some(Background::Color(dot)),
            border: Border { radius: 4.0.into(), ..Default::default() },
            ..Default::default()
        }),
        text(status).size(12).color(DIM),
    ]
    .spacing(8)
    .align_y(Alignment::Center);

    container(
        column![
            text("DELL G15").size(24),
            text("5530 CONTROLLER").size(11).color(DIM),
            Space::new().height(24),
            nav("Lighting", Page::Lighting),
            nav("Performance", Page::Performance),
            nav("Settings", Page::Settings),
            Space::new().height(Length::Fill),
            status_row,
        ]
        .spacing(8),
    )
    .padding(20)
    .width(220)
    .height(Length::Fill)
    .style(sidebar_style)
    .into()
}

fn lighting(app: &App) -> Element<'_, Message> {
    let s = &app.settings;
    let mut controls = column![
        heading("MODE"),
        segmented(&LedMode::ALL, s.led_mode, LedMode::label, Message::LedMode)
    ]
    .spacing(10);

    match s.led_mode {
        LedMode::Static => {
            controls = controls.push(heading("COLOR"));
            for (i, name) in ["R", "G", "B"].into_iter().enumerate() {
                controls = controls.push(
                    row![
                        text(name).size(14).width(16),
                        slider(0..=255u8, s.rgb[i], move |v| Message::Rgb(i, v)),
                        text(s.rgb[i].to_string()).size(14).width(34),
                    ]
                    .spacing(12)
                    .align_y(Alignment::Center),
                );
            }
            controls = controls.push(
                row(PRESETS.iter().map(|&c| {
                    button(Space::new())
                        .width(28)
                        .height(28)
                        .style(swatch(rgb(c), c == s.rgb))
                        .on_press(Message::Preset(c))
                        .into()
                }))
                .spacing(8),
            );
        }
        LedMode::Morph => {
            controls = controls.push(heading(match s.duration {
                0..=255 => "SPEED: VERY FAST",
                256..=1023 => "SPEED: FAST",
                1024..=2047 => "SPEED: BALANCED",
                2048..=3071 => "SPEED: SLOW",
                _ => "SPEED: VERY SLOW",
            }));
            controls = controls.push(slider(4..=0xfffu16, s.duration, Message::Duration));
        }
        LedMode::Off => {
            controls = controls.push(text("Animations are removed and the keyboard goes dark.").color(DIM));
        }
    }

    if s.led_mode != LedMode::Off {
        controls = controls.push(heading("BRIGHTNESS")).push(segmented(
            &Brightness::ALL,
            s.brightness,
            Brightness::label,
            Message::Brightness,
        ));
    }

    let apply = button(text("Apply").size(15).center())
        .padding([10, 28])
        .style(button::primary)
        .on_press_maybe(app.led_dirty().then_some(Message::ApplyLed));
    controls = controls
        .push(Space::new().height(6))
        .push(row![apply, text(&app.led_status).size(13).color(DIM)].spacing(14).align_y(Alignment::Center));

    let preview_style = {
        let mode = s.led_mode;
        let c = rgb(s.rgb);
        move |_: &Theme| {
            let (bg, glow) = match mode {
                LedMode::Static => (Background::Color(c), c),
                LedMode::Morph => (
                    Background::Gradient(
                        gradient::Linear::new(Radians(0.9))
                            .add_stop(0.0, Color::from_rgb(1.0, 0.1, 0.1))
                            .add_stop(0.5, Color::from_rgb(0.1, 1.0, 0.2))
                            .add_stop(1.0, Color::from_rgb(0.1, 0.3, 1.0))
                            .into(),
                    ),
                    Color::from_rgb(0.4, 0.4, 0.8),
                ),
                LedMode::Off => (Background::Color(Color::from_rgb8(0x1a, 0x1e, 0x29)), Color::TRANSPARENT),
            };
            container::Style {
                background: Some(bg),
                border: Border { color: Color { a: 0.35, ..Color::WHITE }, width: 2.0, radius: 60.0.into() },
                shadow: Shadow { color: Color { a: 0.55, ..glow }, offset: Vector::ZERO, blur_radius: 40.0 },
                ..Default::default()
            }
        }
    };
    let hex = match s.led_mode {
        LedMode::Static => format!("#{:02X}{:02X}{:02X}", s.rgb[0], s.rgb[1], s.rgb[2]),
        LedMode::Morph => "Color cycle".into(),
        LedMode::Off => "Off".into(),
    };
    let preview = panel(
        column![
            Space::new().height(18),
            container(Space::new()).width(120).height(120).style(preview_style),
            Space::new().height(22),
            text(hex).size(18),
            text("PREVIEW").size(11).color(DIM),
        ]
        .spacing(4)
        .align_x(Alignment::Center)
        .width(Length::Fill),
    )
    .width(260)
    .height(Length::Fill);

    column![
        text("Keyboard Lighting").size(26),
        row![panel(controls).height(Length::Fill), preview].spacing(18).height(Length::Fill),
    ]
    .spacing(16)
    .into()
}

fn perf_tab_label(t: PerfTab) -> &'static str {
    match t {
        PerfTab::Overview => "Overview",
        PerfTab::Nvidia => "Nvidia",
        PerfTab::CoreCtrl => "CoreCtrl",
    }
}

fn performance(app: &App) -> Element<'_, Message> {
    let tabs = segmented(
        &[PerfTab::Overview, PerfTab::Nvidia, PerfTab::CoreCtrl],
        app.perf_tab,
        perf_tab_label,
        Message::PerfTab,
    );
    let body = match app.perf_tab {
        PerfTab::Overview => overview(app),
        PerfTab::Nvidia => nvidia(app),
        PerfTab::CoreCtrl => corectrl(app),
    };
    column![text("Performance").size(26), tabs, body].spacing(16).into()
}

fn status_line(app: &App) -> Element<'_, Message> {
    text(&app.tuning_status).size(13).color(DIM).into()
}

fn option_button<'a>(label: &'static str, blurb: &'static str, selected: bool, msg: Message) -> Element<'a, Message> {
    button(column![text(label).size(15), text(blurb).size(11).color(DIM)].spacing(2))
        .width(Length::Fill)
        .padding([10, 14])
        .style(chip(selected))
        .on_press(msg)
        .into()
}

fn nvidia(app: &App) -> Element<'_, Message> {
    let t = &app.tuning;
    let current = app.settings.powermizer;
    let mizer = column![
        heading("POWERMIZER MODE"),
        row(PowerMizer::ALL.iter().map(|&m| option_button(
            m.label(),
            m.blurb(),
            current == Some(m),
            Message::SetPowerMizer(m)
        )))
        .spacing(10),
        text("Your choice is remembered and reapplied when the app starts.").size(13).color(DIM),
    ]
    .spacing(10);

    let powerd = match t.powerd {
        Some(on) => row![
            option_button("On", "Dynamic Boost running", on, Message::SetPowerd(true)),
            option_button("Off", "Service stopped", !on, Message::SetPowerd(false)),
        ]
        .spacing(10),
        None => row![text("nvidia-powerd service not found").size(13).color(DIM)],
    };
    let mut powerd_col = column![heading("NVIDIA-POWERD (DYNAMIC BOOST)"), powerd].spacing(10);
    if !app.helper.is_ready() {
        powerd_col = powerd_col.push(text("Changing the service needs the root helper (sudo ./install.sh).").size(13).color(DIM));
    }

    column![panel(mizer), panel(powerd_col), status_line(app)].spacing(16).into()
}

fn corectrl(app: &App) -> Element<'_, Message> {
    let t = &app.tuning;
    let current = match t.governor {
        Some(g) => g.name().to_string(),
        None => "unknown".into(),
    };
    let driver = t.scaling_driver.clone().unwrap_or_else(|| "unknown".into());
    let epp = t.epp.clone().unwrap_or_else(|| "n/a".into());
    let gov = column![
        heading("CPU FREQUENCY GOVERNOR"),
        row![
            option_button("Powersave", "Scale clocks with load", t.governor == Some(Governor::Powersave), Message::SetGovernor(Governor::Powersave)),
            option_button("Performance", "Favor high clocks", t.governor == Some(Governor::Performance), Message::SetGovernor(Governor::Performance)),
        ]
        .spacing(10),
        text(format!("Current: {current}   EPP: {epp}   Driver: {driver}")).size(13).color(DIM),
    ]
    .spacing(10);
    let mut page = column![panel(gov), status_line(app)].spacing(16);
    if !app.helper.is_ready() {
        page = page.push(text("Changing the governor needs the root helper (sudo ./install.sh).").size(13).color(DIM));
    }
    page.into()
}

fn overview(app: &App) -> Element<'_, Message> {
    let s = &app.settings;
    let mut page = column![].spacing(16);

    match &app.helper {
        Helper::Ready(_) => {
            let chips = |modes: &[PowerMode]| {
                row(modes.iter().map(|&m| {
                    button(
                        column![text(m.label()).size(15), text(m.blurb()).size(11).color(DIM)].spacing(2),
                    )
                    .width(Length::Fill)
                    .padding([10, 14])
                    .style(chip(s.power == m))
                    .on_press(Message::Power(m))
                    .into()
                }))
                .spacing(10)
            };
            let mut power = column![
                heading("POWER MODE"),
                chips(&PowerMode::ALL[..3]),
                chips(&PowerMode::ALL[3..]),
            ]
            .spacing(10);

            if s.power == PowerMode::Manual {
                let fan = |name: &'static str, fan: Fan, v: u8| {
                    row![
                        text(name).size(14).width(96),
                        slider(0..=255u8, v, move |x| Message::FanChanged(fan, x))
                            .on_release(Message::FanReleased(fan)),
                        text(format!("{}%", (v as f32 / 255.0 * 100.0).round() as u32)).size(14).width(44),
                    ]
                    .spacing(12)
                    .align_y(Alignment::Center)
                };
                power = power
                    .push(Space::new().height(4))
                    .push(heading("FAN BOOST"))
                    .push(fan("CPU fan", Fan::Cpu, s.fan_cpu))
                    .push(fan("GPU fan", Fan::Gpu, s.fan_gpu))
                    .push(toggler(s.live_fan).label("Apply while dragging").on_toggle(Message::LiveFan));
            }
            power = power.push(text(&app.power_status).size(13).color(DIM));
            page = page.push(panel(power));
        }
        Helper::Checking => page = page.push(panel(text("Checking the power helper...").color(DIM))),
        Helper::Unsupported(m) => {
            page = page.push(panel(
                text(format!("Power and fan controls are only available on the Dell G15 5530 ({m}).")).color(DIM),
            ))
        }
        Helper::Unavailable(e) => {
            page = page.push(panel(
                column![
                    text("Power helper is not running").size(16),
                    text(format!("{e}\nInstall it once with: sudo ./install.sh")).size(13).color(DIM),
                    button(text("Retry")).padding([8, 18]).style(chip(false)).on_press(Message::Probe),
                ]
                .spacing(10),
            ));
        }
    }

    let sample = app.sample.as_ref();
    let ready = app.helper.is_ready();
    let accent = app.theme.palette().primary;
    let temp = |v: Option<u32>| if ready { v.map_or("--".into(), |t| format!("{t} °C")) } else { "N/A".into() };
    let watts = |v: Option<f32>| v.map_or("N/A".into(), |w| format!("{w:.1} W"));
    let rpm = |v: Option<u32>| if ready { v.map_or("--".into(), |r| format!("{r} RPM")) } else { "N/A".into() };

    let pct = |v: Option<f32>| v.map_or("N/A".into(), |u| format!("{u:.0} %"));
    let mem = sample.map_or(("--".into(), "".into()), |x| {
        (format!("{:.0} %", x.mem_percent()), format!("{:.1} / {:.1} GB", x.mem_used_gb, x.mem_total_gb))
    });
    let pr = Some((0.0, 100.0));
    let third = || Space::new().width(Length::Fill);

    let grid = column![
        row![
            metric("CPU TEMP", temp(sample.map(|x| x.cpu_temp)), rpm(sample.map(|x| x.cpu_rpm)), &app.history[0], accent, None),
            metric("CPU USAGE", pct(sample.and_then(|x| x.cpu_usage)), "All cores".into(), &app.history[4], accent, pr),
            metric("CPU POWER", watts(sample.and_then(|x| x.cpu_watts)), "Package".into(), &app.history[2], accent, None),
        ]
        .spacing(16),
        row![
            metric("GPU TEMP", temp(sample.map(|x| x.gpu_temp)), rpm(sample.map(|x| x.gpu_rpm)), &app.history[1], accent, None),
            metric("GPU USAGE", pct(sample.and_then(|x| x.gpu_usage)), "Utilization".into(), &app.history[5], accent, pr),
            metric("GPU POWER", watts(sample.and_then(|x| x.gpu_watts)), "Board".into(), &app.history[3], accent, None),
        ]
        .spacing(16),
        row![metric("MEMORY", mem.0, mem.1, &app.history[6], accent, pr), third(), third()].spacing(16),
    ]
    .spacing(16);

    page = page.push(heading("LIVE SENSORS")).push(grid);
    scrollable(page.padding([0, 12])).height(Length::Fill).into()
}

fn metric<'a>(
    title: &'static str,
    value: String,
    sub: String,
    data: &'a VecDeque<f32>,
    color: Color,
    range: Option<(f32, f32)>,
) -> Element<'a, Message> {
    panel(
        column![
            heading(title),
            text(value).size(28),
            text(sub).size(12).color(DIM),
            canvas(Spark { data, color, range }).width(Length::Fill).height(54),
        ]
        .spacing(4),
    )
    .into()
}

struct Spark<'a> {
    data: &'a VecDeque<f32>,
    color: Color,
    range: Option<(f32, f32)>,
}

impl<Message> canvas::Program<Message> for Spark<'_> {
    type State = ();

    fn draw(
        &self,
        _state: &(),
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        let (w, h) = (bounds.width, bounds.height);
        let grid = canvas::Stroke::default().with_color(Color::from_rgb8(0x1f, 0x26, 0x35)).with_width(1.0);
        for i in 1..4 {
            let y = h * i as f32 / 4.0;
            frame.stroke(&canvas::Path::line(Point::new(0.0, y), Point::new(w, y)), grid);
        }
        if self.data.len() >= 2 {
            let min = self.data.iter().copied().fold(f32::MAX, f32::min);
            let max = self.data.iter().copied().fold(f32::MIN, f32::max);
            let (lo, span) = match self.range {
                Some((lo, hi)) => (lo, hi - lo),
                None => {
                    let span = (max - min).max(10.0);
                    (min - (span - (max - min)) / 2.0, span)
                }
            };
            let step = w / (super::HISTORY - 1) as f32;
            let x0 = w - step * (self.data.len() - 1) as f32;
            let pt = |i: usize, v: f32| Point::new(x0 + step * i as f32, h - 3.0 - (v - lo) / span * (h - 6.0));

            let line = canvas::Path::new(|b| {
                for (i, &v) in self.data.iter().enumerate() {
                    if i == 0 {
                        b.move_to(pt(i, v));
                    } else {
                        b.line_to(pt(i, v));
                    }
                }
            });
            let area = canvas::Path::new(|b| {
                b.move_to(Point::new(x0, h));
                for (i, &v) in self.data.iter().enumerate() {
                    b.line_to(pt(i, v));
                }
                b.line_to(Point::new(w, h));
                b.close();
            });
            frame.fill(&area, Color { a: 0.14, ..self.color });
            frame.stroke(&line, canvas::Stroke::default().with_color(self.color).with_width(2.0));
        }
        vec![frame.into_geometry()]
    }
}

fn settings(app: &App) -> Element<'_, Message> {
    let s = &app.settings;
    let accents = row(Accent::ALL.iter().map(|&a| {
        button(Space::new())
            .width(28)
            .height(28)
            .style(swatch(rgb(a.rgb()), a == s.accent))
            .on_press(Message::Accent(a))
            .into()
    }))
    .spacing(10);

    let close = toggler(s.close_to_tray)
        .label(if app.tray_ok { "Keep running in the tray when closed" } else { "Tray unavailable (no StatusNotifier host)" })
        .on_toggle_maybe(app.tray_ok.then_some(Message::CloseToTray));

    let line = |k: &'static str, v: String| {
        row![text(k).size(13).color(DIM).width(140), text(v).size(13)].spacing(10)
    };
    let helper = match &app.helper {
        Helper::Checking => "Checking".to_string(),
        Helper::Ready(_) => "Running".to_string(),
        Helper::Unsupported(_) => "Running (unsupported model)".to_string(),
        Helper::Unavailable(e) => format!("Offline ({e})"),
    };
    let model = match &app.helper {
        Helper::Ready(m) | Helper::Unsupported(m) => m.clone(),
        _ => "Unknown".into(),
    };

    column![
        text("Settings").size(26),
        panel(
            column![
                heading("ACCENT COLOR"),
                accents,
                Space::new().height(6),
                close,
                toggler(s.auto_refresh).label("Refresh sensors every 0.5 s").on_toggle(Message::AutoRefresh),
            ]
            .spacing(12)
        ),
        panel(
            column![
                heading("DIAGNOSTICS"),
                line("Power helper", helper),
                line("Model", model),
                line("LED firmware", app.led_firmware.clone()),
                line("Last LED action", app.led_status.clone()),
                line("Last ACPI result", app.last_acpi.clone()),
                line("Tray", if app.tray_ok { "Active" } else { "Unavailable" }.to_string()),
            ]
            .spacing(8)
        ),
    ]
    .spacing(16)
    .into()
}
