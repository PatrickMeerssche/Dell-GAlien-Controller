//! StatusNotifierItem tray icon. Events are forwarded to the GUI through a channel.

use std::sync::mpsc::{self, Receiver, Sender};

use ksni::blocking::{Handle, TrayMethods};

#[derive(Clone, Copy, Debug)]
pub enum TrayEvent {
    ToggleLeds,
    Show,
    Quit,
}

pub struct Tray {
    tx: Sender<TrayEvent>,
    icon: Vec<ksni::Icon>,
}

impl ksni::Tray for Tray {
    fn id(&self) -> String {
        "dell5530-controller".into()
    }

    fn title(&self) -> String {
        "Dell G-Series Controller".into()
    }

    fn icon_pixmap(&self) -> Vec<ksni::Icon> {
        self.icon.clone()
    }

    fn activate(&mut self, _x: i32, _y: i32) {
        let _ = self.tx.send(TrayEvent::ToggleLeds);
    }

    fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
        use ksni::menu::StandardItem;
        vec![
            StandardItem {
                label: "Show window".into(),
                activate: Box::new(|t: &mut Self| {
                    let _ = t.tx.send(TrayEvent::Show);
                }),
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: "Toggle keyboard LEDs".into(),
                activate: Box::new(|t: &mut Self| {
                    let _ = t.tx.send(TrayEvent::ToggleLeds);
                }),
                ..Default::default()
            }
            .into(),
            ksni::MenuItem::Separator,
            StandardItem {
                label: "Quit".into(),
                activate: Box::new(|t: &mut Self| {
                    let _ = t.tx.send(TrayEvent::Quit);
                }),
                ..Default::default()
            }
            .into(),
        ]
    }
}

fn load_icon() -> Vec<ksni::Icon> {
    let Ok(img) = image::load_from_memory_with_format(crate::ICON_PNG, image::ImageFormat::Png) else {
        return Vec::new();
    };
    let img = img.into_rgba8();
    let (width, height) = img.dimensions();
    let mut data = img.into_vec();
    for px in data.chunks_exact_mut(4) {
        px.rotate_right(1); // RGBA -> ARGB
    }
    vec![ksni::Icon { width: width as i32, height: height as i32, data }]
}

/// Returns `None` when no StatusNotifier host is available on the session bus.
pub fn spawn() -> Option<(Handle<Tray>, Receiver<TrayEvent>)> {
    let (tx, rx) = mpsc::channel();
    let handle = Tray { tx, icon: load_icon() }.spawn().ok()?;
    Some((handle, rx))
}
