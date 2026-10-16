use gtk::prelude::*;
use std::{cell::Cell, rc::Rc};

#[derive(Clone)]
pub enum Action {
    Search(String),
    Clipboard,
    Theme,
    Finish,
}

struct Step {
    title: &'static str,
    description: &'static str,
    example: String,
    tip: &'static str,
    button: &'static str,
    action: Option<Action>,
}

pub fn panel(
    app: String,
    initial: usize,
    changed: impl Fn(usize) + 'static,
    activate: impl Fn(Action) + 'static,
) -> gtk::Box {
    let steps = Rc::new(vec![
        Step {
            title: "Find and open an app",
            description: "Open Spotlight and type an app’s name. Matching apps appear first. Move your pointer over a result, or use ↑ and ↓, then press Enter to open it.",
            example: app.clone(),
            tip: "Meta+Space opens Spotlight on KDE when its shortcut is installed. You can also launch it from your app menu. Double-clicking a result opens it too.",
            button: "Try app search",
            action: Some(Action::Search(app)),
        },
        Step {
            title: "Find files and folders",
            description: "Type a filename or folder name in the same search bar. The full path appears below each result, so you can choose the right one.",
            example: "recent files\nrecent folders".into(),
            tip: "Recent files appear even outside the file index. Hidden files and hidden folders such as .config stay out of results.",
            button: "Show recent files",
            action: Some(Action::Search("recent files".into())),
        },
        Step {
            title: "Get comfortable with shortcuts",
            description: "The highlighted result is the one Spotlight will use. You can switch between the mouse and keyboard whenever you like.",
            example: "↑ / ↓         Select a result\nEnter         Open or run\nCtrl+K        Show actions\nCtrl+F        Toggle favorite\nEscape        Go back or hide\nF1            Open this guide".into(),
            tip: "Favorites and apps you use often get a ranking boost. Press Ctrl+Enter in the main search to search Google for what you typed.",
            button: "",
            action: None,
        },
        Step {
            title: "Calculate without opening another app",
            description: "Type a calculation or unit conversion into the main search. The answer appears at the top. Press Enter to copy it.",
            example: "2+3*4       → 14\n10 km to mi → 6.2137 mi".into(),
            tip: "The Try button fills in an example. It leaves you in control of copying or opening anything.",
            button: "Try a calculation",
            action: Some(Action::Search("2+3*4".into())),
        },
        Step {
            title: "Reuse clipboard items and capture a note",
            description: "Search for Clipboard History to find copied text and images. Select an item and press Enter to copy it again, then paste with Ctrl+V in your other app.",
            example: "Clipboard History\nnote Buy milk".into(),
            tip: "Clipboard history uses KDE Klipper. To save a quick note, type note followed by your text, then press Enter. Your note destination is shown in Settings → General.",
            button: "Open Clipboard History",
            action: Some(Action::Clipboard),
        },
        Step {
            title: "Make Spotlight yours",
            description: "Open Settings → Theme to choose Dark, Light, or Midnight. You can also pick your own background, text, accent, and selection colors.",
            example: "Settings → Theme → Apply theme".into(),
            tip: "The preview shows your changes before you apply them. Your saved theme is used next time Spotlight opens. Reopen this guide with F1 or by searching walkthrough.",
            button: "Open Theme settings",
            action: Some(Action::Theme),
        },
    ]);
    let index = Rc::new(Cell::new(initial.min(steps.len() - 1)));
    let activate: Rc<dyn Fn(Action)> = Rc::new(activate);
    let panel = gtk::Box::new(gtk::Orientation::Vertical, 18);
    panel.add_css_class("editor");
    let progress = gtk::Label::new(None);
    progress.set_xalign(0.);
    progress.add_css_class("subtitle");
    progress.set_widget_name("guide-progress");
    panel.append(&progress);
    let title = gtk::Label::new(None);
    title.set_xalign(0.);
    title.set_wrap(true);
    title.add_css_class("guide-title");
    title.set_widget_name("guide-title");
    panel.append(&title);
    let description = gtk::Label::new(None);
    description.set_xalign(0.);
    description.set_wrap(true);
    description.set_max_width_chars(55);
    panel.append(&description);
    let example = gtk::Label::new(None);
    example.set_xalign(0.);
    example.set_selectable(true);
    example.set_wrap(true);
    example.add_css_class("guide-example");
    panel.append(&example);
    let tip = gtk::Label::new(None);
    tip.set_xalign(0.);
    tip.set_wrap(true);
    tip.set_max_width_chars(55);
    tip.add_css_class("subtitle");
    panel.append(&tip);
    let try_it = gtk::Button::new();
    try_it.set_halign(gtk::Align::Start);
    try_it.set_widget_name("guide-try");
    panel.append(&try_it);
    let spacer = gtk::Box::new(gtk::Orientation::Vertical, 0);
    spacer.set_vexpand(true);
    panel.append(&spacer);
    let navigation = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let back = gtk::Button::with_label("Back");
    back.set_widget_name("guide-back");
    navigation.append(&back);
    let gap = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    gap.set_hexpand(true);
    navigation.append(&gap);
    let next = gtk::Button::with_label("Next");
    next.add_css_class("accent");
    next.set_widget_name("guide-next");
    navigation.append(&next);
    panel.append(&navigation);
    let render: Rc<dyn Fn()> = {
        let steps = Rc::clone(&steps);
        let index = Rc::clone(&index);
        let back = back.downgrade();
        let next = next.downgrade();
        let try_it = try_it.downgrade();
        Rc::new(move || {
            let i = index.get();
            let step = &steps[i];
            progress.set_text(&format!("Getting started · {} of {}", i + 1, steps.len()));
            title.set_text(step.title);
            description.set_text(step.description);
            example.set_text(&step.example);
            tip.set_text(step.tip);
            if let Some(back) = back.upgrade() {
                back.set_sensitive(i > 0);
            }
            if let Some(next) = next.upgrade() {
                next.set_label(if i + 1 == steps.len() {
                    "Start searching"
                } else {
                    "Next"
                });
            }
            if let Some(try_it) = try_it.upgrade() {
                try_it.set_visible(step.action.is_some());
                try_it.set_label(step.button);
            }
            changed(i);
        })
    };
    let step = Rc::clone(&index);
    let examples = Rc::clone(&steps);
    let action = Rc::clone(&activate);
    try_it.connect_clicked(move |_| {
        if let Some(action_value) = examples[step.get()].action.clone() {
            action(action_value);
        }
    });
    let step = Rc::clone(&index);
    let update = Rc::clone(&render);
    back.connect_clicked(move |_| {
        step.set(step.get().saturating_sub(1));
        update();
    });
    let step = index;
    let update = Rc::clone(&render);
    next.connect_clicked(move |_| {
        if step.get() + 1 == steps.len() {
            activate(Action::Finish);
        } else {
            step.set(step.get() + 1);
            update();
        }
    });
    render();
    panel
}
