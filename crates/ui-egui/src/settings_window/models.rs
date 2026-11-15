//! The AI models tab: saved models, their providers and keys.

use super::{divider, group, note, row, section};
use crate::icons::Icon;
use crate::theme::Tokens;
use crate::ui_command::{Commands, Sent};
use crate::widgets;
use egui::{
    Align, Layout, Rect, RichText, ScrollArea, Sense, Stroke, TextEdit, Ui, Vec2, pos2, vec2,
};
use mareader_assist::{AppleState, Board, Job, KeyRow, KeyState, Load, lanes};
use mareader_engine::command::Command;
use mareader_settings::{AiSettings, ModelPreset, PageTask, PresetId, Provider};
use mareader_study::Failure;

const FIELD_W: f32 = 260.0;
const PICK_W: f32 = 240.0;

/// What the running service adds to the settings.
pub struct Live<'a> {
    pub board: &'a Board,
    /// Tasks a saved model runs now.
    pub in_use: &'a dyn Fn(PresetId) -> usize,
}

pub fn models_tab(ui: &mut Ui, ai: &mut AiSettings, live: Option<Live<'_>>, out: &mut Commands) {
    let board = live.as_ref().map(|l| l.board);
    super::ai::probe(board, out);
    if let Some(board) = board {
        fetch_models(ui, ai, board, out);
    }
    models(ui, ai, live.as_ref(), out);
    providers(ui, ai, board, out);
}

/// The providers saved models use, each once, in the menu's order.
fn used_servers(ai: &AiSettings) -> Vec<Provider> {
    let used = |p: &Provider| ai.presets.iter().any(|m| m.provider == *p);
    Provider::SERVERS.into_iter().filter(used).collect()
}

/// Local servers list their models afresh each time the tab opens.
fn fetch_models(ui: &Ui, ai: &AiSettings, board: &Board, out: &mut Commands) {
    // The tab was hidden if it wasn't drawn the frame before.
    let seen = egui::Id::new("ai-tab-frame");
    let frame = ui.ctx().cumulative_frame_nr();
    let last: Option<u64> = ui.data(|d| d.get_temp(seen));
    ui.data_mut(|d| d.insert_temp(seen, frame));
    let opened = last.is_none_or(|l| l + 1 < frame);
    for p in used_servers(ai) {
        let idle = *board.models(p) == Load::Idle;
        if p.local() && !board.fetching(p) && (opened || idle) {
            out.push(Sent::Engine(Command::Ai(Job::Models(p))));
        }
    }
}

/// The open model's id, kept while the window lives.
fn open_id() -> egui::Id {
    egui::Id::new("ai-open-model")
}

/// Whether a provider's section is open; unset follows its standing.
fn provider_open_id(p: Provider) -> egui::Id {
    egui::Id::new(("ai-provider-open", p.scope()))
}

/// What an open model's buttons ask for.
enum Act {
    Delete,
    Duplicate,
    Dictionary,
}

fn models(ui: &mut Ui, ai: &mut AiSettings, live: Option<&Live<'_>>, out: &mut Commands) {
    let board = live.map(|l| l.board);
    let mut open = ui.data(|d| d.get_temp::<Option<u32>>(open_id()).flatten());
    section(ui, "Models");
    let mut act = None;
    let mut made = None;
    group(ui, |ui| {
        for i in 0..ai.presets.len() {
            let Some(p) = ai.presets.get(i) else {
                break;
            };
            let id = p.id;
            let busy = live.map_or(0, |l| (l.in_use)(id));
            let (line, warn) = status_line(p, board, busy);
            let pills = uses(ai, id);
            let is_open = open == Some(id.0);
            let look = Head {
                provider: p.provider,
                title: &p.title(),
                line: &line,
                warn,
                pills: &pills,
                open: is_open,
            };
            if head(ui, &look).clicked() {
                open = (!is_open).then_some(id.0);
            }
            let explains = ai.tasks.explain.preset == Some(id);
            if is_open
                && let Some(p) = ai.presets.get_mut(i)
                && let Some(a) = editor(ui, p, (board, explains), out)
            {
                act = Some((id, a));
            }
            divider(ui);
        }
        made = add_model(ui, ai);
    });
    match act {
        Some((id, Act::Delete)) => {
            ai.remove_preset(id);
            open = None;
        }
        Some((id, Act::Duplicate)) => made = duplicate(ai, id).or(made),
        Some((id, Act::Dictionary)) => ai.tasks.explain.preset = Some(id),
        None => {}
    }
    if let Some(id) = made {
        open = Some(id.0);
        // Its provider opens too, so a key can go in at once.
        if let Some(p) = ai.preset(id).map(|p| p.provider) {
            ui.data_mut(|d| d.insert_temp(provider_open_id(p), true));
        }
    }
    ui.data_mut(|d| d.insert_temp(open_id(), open));
    note(
        ui,
        "Each task asks the model chosen for it in the AI tab. Duplicate a \
         model with another “At once” to give a task its own room.",
    );
}

/// The tasks asking `id`, fallbacks included, as the row's pills.
fn uses(ai: &AiSettings, id: PresetId) -> Vec<&'static str> {
    let is = |p: Option<&ModelPreset>| p.is_some_and(|p| p.id == id);
    let mut names = Vec::new();
    if is(ai.explain_preset()) {
        names.push("AI Dictionary");
    }
    if is(ai.organize_preset()) {
        names.push("Library");
    }
    for task in PageTask::ALL {
        if is(ai.page_preset(task)) {
            names.push(task.label());
        }
    }
    names
}

/// The row's small print; true when it asks for a hand.
fn status_line(p: &ModelPreset, board: Option<&Board>, busy: usize) -> (String, bool) {
    let server = p.provider.server();
    if let Some(b) = board
        && server
        && missing(&p.model, b.models(p.provider))
    {
        return (format!("{} isn't on the server now", p.model.trim()), true);
    }
    if server {
        let (said, warn) = standing(p.provider, board);
        if warn {
            return (said, true);
        }
    }
    // The title already reads "Provider · model" unless renamed.
    let name = p.name.trim();
    let named = !name.is_empty() && name != ModelPreset::suggested_name(p.provider, &p.model);
    let mut parts = Vec::new();
    if named {
        parts.push(p.provider.label().to_owned());
    }
    if p.provider == Provider::Apple {
        parts.push(apple_status(board).to_owned());
    } else if named || p.model.trim().is_empty() {
        parts.push(match p.model() {
            "" => "the server's first model".to_owned(),
            model => model.to_owned(),
        });
    }
    let room = board.map_or(u32::from(p.at_once), |b| b.capacity(p));
    parts.push(format!("{room} at once"));
    if busy > 0 {
        parts.push(format!("{busy} running"));
    }
    let warn = p.provider == Provider::Apple
        && board.is_some_and(|b| !matches!(b.apple.ready(), Some(AppleState::Ready(_)) | None));
    (parts.join(" · "), warn)
}

/// How a provider stands, in a few words; true when it needs a hand.
fn standing(p: Provider, board: Option<&Board>) -> (String, bool) {
    let Some(b) = board else {
        return ("Shows once the AI service runs".to_owned(), false);
    };
    if let Some(f) = b.problems.get(&p) {
        return (f.why.message().to_owned(), true);
    }
    if p.takes_keys() {
        return match b.keys(p) {
            Load::Ready(rows) => {
                let set_aside = |r: &&KeyRow| matches!(r.state, KeyState::Banned { .. });
                let ready = rows.len() - rows.iter().filter(set_aside).count();
                match (rows.len(), ready) {
                    (0, _) if p.needs_key() => ("No API key yet".to_owned(), true),
                    (0, _) => ("No key; fine if the server asks for none".to_owned(), false),
                    (_, 0) => ("Every key is set aside".to_owned(), true),
                    (_, 1) => ("1 key ready".to_owned(), false),
                    (_, n) => (format!("{n} keys ready"), false),
                }
            }
            Load::Failed(f) => (f.why.message().to_owned(), true),
            Load::Idle | Load::Busy => ("Checking keys…".to_owned(), false),
        };
    }
    match b.models(p) {
        Load::Ready(list) if list.is_empty() => ("Running, with no models yet".to_owned(), true),
        Load::Ready(list) if list.len() == 1 => ("1 model on this computer".to_owned(), false),
        Load::Ready(list) => (format!("{} models on this computer", list.len()), false),
        Load::Failed(_) => (format!("Not reachable; is {} running?", p.label()), true),
        Load::Idle | Load::Busy => ("Looking for it…".to_owned(), false),
    }
}

fn apple_status(board: Option<&Board>) -> &'static str {
    match board.and_then(|b| b.apple.ready()) {
        Some(AppleState::Ready(_)) => "ready",
        Some(AppleState::NotReady(_)) => "downloading",
        Some(AppleState::Off(_)) => "off in System Settings",
        Some(AppleState::Missing(_)) => "not on this Mac",
        None => "on this Mac",
    }
}

/// Each provider's own logo; a custom server has none, so a server.
fn logo(p: Provider) -> Icon {
    match p {
        Provider::OpenAi => Icon::OpenAi,
        Provider::Gemini => Icon::Gemini,
        Provider::OpenRouter => Icon::OpenRouter,
        Provider::Ollama => Icon::Ollama,
        Provider::LmStudio => Icon::LmStudio,
        Provider::Custom => Icon::Server,
        Provider::Apple => Icon::Apple,
    }
}

/// The logo in ink on a quiet tile, readable in every look.
fn badge(ui: &Ui, rect: Rect, p: Provider) {
    let t = Tokens::get(ui.ctx());
    let painter = ui.painter();
    painter.rect_filled(rect, 7.0, t.surface);
    let edge = Stroke::new(1.0, t.line);
    painter.rect_stroke(rect, 7.0, edge, egui::StrokeKind::Inside);
    let side = (rect.height() * 0.6).round();
    let inner = Rect::from_center_size(rect.center(), Vec2::splat(side));
    logo(p).image(ui, side).tint(t.ink).paint_at(ui, inner);
}

/// One row's look: a badge, two lines, pills and a chevron.
struct Head<'a> {
    provider: Provider,
    title: &'a str,
    line: &'a str,
    warn: bool,
    pills: &'a [&'static str],
    open: bool,
}

/// A model's or provider's row; click to open it.
fn head(ui: &mut Ui, h: &Head<'_>) -> egui::Response {
    let t = Tokens::get(ui.ctx());
    let size = vec2(ui.available_width(), 56.0);
    let (rect, r) = ui.allocate_exact_size(size, Sense::click());
    let info =
        |label: &str| egui::WidgetInfo::selected(egui::WidgetType::Button, true, h.open, label);
    r.widget_info(|| info(h.title));
    let mid = rect.center().y;
    let mark = Rect::from_center_size(pos2(rect.left() + 15.0, mid), Vec2::splat(30.0));
    badge(ui, mark, h.provider);
    let chevron = if h.open {
        Icon::ChevronDown
    } else {
        Icon::ChevronRight
    };
    let at = Rect::from_center_size(pos2(rect.right() - 9.0, mid), Vec2::splat(16.0));
    chevron.image(ui, 16.0).tint(t.muted).paint_at(ui, at);
    let right = pills(ui, h.pills, pos2(rect.right() - 26.0, mid));
    let text = Rect::from_min_max(
        pos2(rect.left() + 42.0, rect.top()),
        pos2(right - 8.0, rect.bottom()),
    );
    let painter = ui.painter().with_clip_rect(text);
    let ink = if r.hovered() { t.accent } else { t.ink };
    let font = egui::FontId::proportional(t.text);
    let top = pos2(text.left(), mid - 9.0);
    painter.text(top, egui::Align2::LEFT_CENTER, h.title, font, ink);
    let small = egui::FontId::proportional(t.text_small);
    let mut below = pos2(text.left(), mid + 9.0);
    let color = if h.warn { t.danger } else { t.muted };
    if h.warn {
        let at = Rect::from_center_size(pos2(below.x + 6.0, below.y), Vec2::splat(12.0));
        Icon::Alert.image(ui, 12.0).tint(color).paint_at(ui, at);
        below.x += 16.0;
    }
    painter.text(below, egui::Align2::LEFT_CENTER, h.line, small, color);
    r
}

/// Pills laid leftward from `end`; the left edge they reach.
fn pills(ui: &Ui, names: &[&'static str], end: egui::Pos2) -> f32 {
    let t = Tokens::get(ui.ctx());
    // Two read at a glance; the rest fold into a count.
    let more = names.len().saturating_sub(2);
    let shown = names.iter().take(2).map(|n| (*n).to_owned());
    let mut labels: Vec<String> = shown.collect();
    if more > 0 {
        labels.push(format!("+{more}"));
    }
    let font = egui::FontId::proportional(t.text_small);
    let mut x = end.x;
    for label in labels.iter().rev() {
        let g = ui
            .painter()
            .layout_no_wrap(label.clone(), font.clone(), t.accent);
        let w = g.size().x + 12.0;
        let pill = Rect::from_min_size(pos2(x - w, end.y - 10.0), vec2(w, 20.0));
        ui.painter().rect_filled(pill, 10.0, t.accent_soft);
        let at = pos2(pill.left() + 6.0, end.y - g.size().y / 2.0);
        ui.painter().galley(at, g, t.accent);
        x = pill.left() - 4.0;
    }
    x
}

/// A thread down an open row's insides, tying them to its head.
fn thread(ui: &Ui, top: f32) {
    let t = Tokens::get(ui.ctx());
    let x = ui.min_rect().left() - 8.0;
    let span = egui::Rangef::new(top + 8.0, ui.cursor().top() - 8.0);
    ui.painter()
        .vline(x, span, Stroke::new(2.0, t.accent.gamma_multiply(0.5)));
}

/// Edits one model; what its buttons ask for, if anything.
fn editor(
    ui: &mut Ui,
    p: &mut ModelPreset,
    (board, explains): (Option<&Board>, bool),
    out: &mut Commands,
) -> Option<Act> {
    let top = ui.cursor().top();
    renaming(p, |p| fields(ui, p, board, out));
    room_note(ui, p, board);
    divider(ui);
    let act = actions(ui, p, (board, explains), out);
    thread(ui, top);
    act
}

/// Try, Use for the AI Dictionary, Duplicate, and a confirmed Delete.
fn actions(
    ui: &mut Ui,
    p: &ModelPreset,
    (board, explains): (Option<&Board>, bool),
    out: &mut Commands,
) -> Option<Act> {
    let t = Tokens::get(ui.ctx());
    let confirm = egui::Id::new(("ai-delete", p.id.0));
    let mut asked: bool = ui.data(|d| d.get_temp(confirm)).unwrap_or(false);
    let mut act = None;
    ui.add_space(8.0);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        if let Some(board) = board {
            let busy = matches!(board.tried(p.id), Load::Busy);
            let label = if busy { "Asking…" } else { "Try it" };
            let button = widgets::primary_button(ui, Some(Icon::Try), label, t.accent, &t);
            let r = ui.add_enabled(!busy, button);
            widgets::name(&r, "Try it");
            if r.clicked() {
                out.push(Sent::Engine(Command::Ai(Job::Try(p.clone()))));
            }
        }
        if !explains && ui.button("Use for AI Dictionary").clicked() {
            act = Some(Act::Dictionary);
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let label = if asked { "Delete for good" } else { "Delete" };
            let text = RichText::new(label).color(t.danger);
            if ui.button(text).clicked() {
                if asked {
                    act = Some(Act::Delete);
                }
                asked = !asked;
            }
            if asked && ui.button("Keep").clicked() {
                asked = false;
            }
            if !asked && widgets::small_icon_button(ui, Icon::Copy, "Duplicate").clicked() {
                act = Some(Act::Duplicate);
            }
        });
    });
    let deleting = matches!(act, Some(Act::Delete));
    ui.data_mut(|d| d.insert_temp(confirm, asked && !deleting));
    if let Some(board) = board {
        tried(ui, board.tried(p.id));
    }
    ui.add_space(8.0);
    act
}

/// A copy under a fresh name, to give a task its own room.
fn duplicate(ai: &mut AiSettings, id: PresetId) -> Option<PresetId> {
    if ai.presets.len() >= AiSettings::PRESETS_MAX {
        return None;
    }
    let from = ai.preset(id)?.clone();
    let base = from.title();
    let taken = |name: &str| ai.presets.iter().any(|p| p.title() == name);
    let name = (2..=AiSettings::PRESETS_MAX + 1)
        .map(|n| format!("{base} ({n})"))
        .find(|n| !taken(n))?;
    let made = ai.add_preset(from.provider, &from.model);
    let p = ai.preset_mut(made)?;
    p.name = name;
    p.at_once = from.at_once;
    Some(made)
}

/// The last row: providers as tiles; picking one saves a model.
fn add_model(ui: &mut Ui, ai: &mut AiSettings) -> Option<PresetId> {
    let t = Tokens::get(ui.ctx());
    let id = egui::Id::new("ai-adding");
    let first = ai.presets.is_empty();
    let full = ai.presets.len() >= AiSettings::PRESETS_MAX;
    let mut adding = first || ui.data(|d| d.get_temp::<bool>(id)).unwrap_or(false);
    let size = vec2(ui.available_width(), super::ROW_H);
    ui.allocate_ui_with_layout(size, Layout::left_to_right(Align::Center), |ui| {
        if first {
            let text = RichText::new("Pick a provider for your first model").color(t.muted);
            ui.label(text);
            return;
        }
        let r = ui.add_enabled(!full, egui::Button::selectable(adding, "+  Add a model"));
        if r.clicked() {
            adding = !adding;
        }
        if full {
            r.on_disabled_hover_text(format!("{} models is the most", AiSettings::PRESETS_MAX));
        }
    });
    let made = if adding && !full { tiles(ui) } else { None };
    ui.data_mut(|d| d.insert_temp(id, adding && made.is_none()));
    let id = ai.add_preset(made?, "");
    // The first model looks words up straight away.
    if ai.tasks.explain.preset.is_none() {
        ai.tasks.explain.preset = Some(id);
    }
    Some(id)
}

/// Every provider as a tile: badge, name and what it needs.
fn tiles(ui: &mut Ui) -> Option<Provider> {
    let t = Tokens::get(ui.ctx());
    let mac = cfg!(target_os = "macos");
    let shown: Vec<Provider> = Provider::ALL
        .into_iter()
        .filter(|p| mac || *p != Provider::Apple)
        .collect();
    let gap = 8.0;
    let w = ((ui.available_width() - 2.0 * gap) / 3.0).floor();
    let mut picked = None;
    for chunk in shown.chunks(3) {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = gap;
            for p in chunk {
                let (rect, r) = ui.allocate_exact_size(vec2(w, 58.0), Sense::click());
                let label = p.label();
                r.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, label));
                widgets::tip(&r, p.detail(), None);
                let edge = if r.hovered() { t.accent } else { t.line };
                let fill = if r.hovered() { t.accent_soft } else { t.card };
                ui.painter().rect_filled(rect, 8.0, fill);
                let stroke = Stroke::new(1.0, edge);
                ui.painter()
                    .rect_stroke(rect, 8.0, stroke, egui::StrokeKind::Inside);
                let mid = rect.center().y;
                let mark = Rect::from_center_size(pos2(rect.left() + 24.0, mid), Vec2::splat(28.0));
                badge(ui, mark, *p);
                let text = rect.with_min_x(rect.left() + 46.0).shrink2(vec2(0.0, 2.0));
                let painter = ui
                    .painter()
                    .with_clip_rect(text.with_max_x(rect.right() - 6.0));
                let font = egui::FontId::proportional(t.text);
                let top = pos2(text.left(), mid - 8.0);
                painter.text(top, egui::Align2::LEFT_CENTER, label, font, t.ink);
                let small = egui::FontId::proportional(t.text_small);
                let below = pos2(text.left(), mid + 9.0);
                painter.text(below, egui::Align2::LEFT_CENTER, blurb(*p), small, t.muted);
                if r.clicked() {
                    picked = Some(*p);
                }
            }
        });
        ui.add_space(gap);
    }
    picked
}

/// A tile's few words; the tip holds the full line.
fn blurb(p: Provider) -> &'static str {
    match p {
        Provider::OpenAi | Provider::Gemini => "API key",
        Provider::OpenRouter => "Many models",
        Provider::Ollama | Provider::LmStudio => "On this computer",
        Provider::Custom => "Any /v1 address",
        Provider::Apple => "On this Mac",
    }
}

/// The providers saved models use: each a row that opens on its keys.
fn providers(ui: &mut Ui, ai: &mut AiSettings, board: Option<&Board>, out: &mut Commands) {
    let used = used_servers(ai);
    if used.is_empty() {
        return;
    }
    section(ui, "Providers");
    let mut keyed = false;
    group(ui, |ui| {
        for (i, p) in used.into_iter().enumerate() {
            if i > 0 {
                divider(ui);
            }
            keyed |= provider(ui, ai, p, board, out);
        }
    });
    if keyed {
        note(
            ui,
            "Keys are encrypted on this computer. With several, requests take \
             turns, and a key the provider turns away is set aside. A custom \
             server needs one only if it asks.",
        );
    }
}

/// One provider's address and keys; true when its keys show.
fn provider(
    ui: &mut Ui,
    ai: &mut AiSettings,
    p: Provider,
    board: Option<&Board>,
    out: &mut Commands,
) -> bool {
    let Some(e) = ai.endpoint_mut(p) else {
        return false;
    };
    let (said, warn) = standing(p, board);
    let key = provider_open_id(p);
    let mut open = ui.data(|d| d.get_temp::<bool>(key)).unwrap_or(warn);
    let look = Head {
        provider: p,
        title: p.label(),
        line: &said,
        warn,
        pills: &[],
        open,
    };
    if head(ui, &look).clicked() {
        open = !open;
    }
    ui.data_mut(|d| d.insert_temp(key, open));
    if !open {
        return false;
    }
    let top = ui.cursor().top();
    divider(ui);
    let tip = if p.needs_key() {
        "Blank uses the provider's own; change it only for a proxy"
    } else {
        "Up to and including /v1"
    };
    row(ui, "Address", tip, |ui| {
        let hint = match p.default_url() {
            "" => "https://host/v1/",
            url => url,
        };
        field(ui, TextEdit::singleline(&mut e.url).hint_text(hint));
    });
    if p.takes_keys() {
        key_rows(ui, p, board, out);
    }
    thread(ui, top);
    p.takes_keys()
}

/// The provider's keys, one a row, then a field for another.
fn key_rows(ui: &mut Ui, p: Provider, board: Option<&Board>, out: &mut Commands) {
    let Some(board) = board else {
        divider(ui);
        let quiet = "Shows once the AI service runs";
        row(ui, "API keys", quiet, |ui| {
            let t = Tokens::get(ui.ctx());
            ui.label(RichText::new(quiet).size(t.text_small).color(t.muted));
        });
        return;
    };
    if *board.keys(p) == Load::Idle {
        out.push(Sent::Engine(Command::Ai(Job::Keys(p))));
    }
    if let Load::Ready(rows) = board.keys(p) {
        for r in rows {
            divider(ui);
            key_row(ui, p, r, out);
        }
    }
    divider(ui);
    add_key(ui, p, out);
}

/// Runs `edit`; a name never typed follows what the model runs.
fn renaming(p: &mut ModelPreset, edit: impl FnOnce(&mut ModelPreset)) {
    let named = p.name == ModelPreset::suggested_name(p.provider, &p.model);
    edit(p);
    if named {
        p.name = ModelPreset::suggested_name(p.provider, &p.model);
    }
}

fn fields(ui: &mut Ui, p: &mut ModelPreset, board: Option<&Board>, out: &mut Commands) {
    divider(ui);
    row(ui, "Name", "How tasks list this model", |ui| {
        let hint = ModelPreset::suggested_name(p.provider, &p.model);
        let edit = TextEdit::singleline(&mut p.name).hint_text(hint);
        field(ui, edit.char_limit(ModelPreset::NAME_MAX));
    });
    divider(ui);
    row(ui, "Provider", "", |ui| provider_picker(ui, p));
    if p.provider.server() {
        divider(ui);
        model_row(ui, p, board, out);
    }
    divider(ui);
    let tip = "Tasks this model may run at the same time";
    row(ui, "At once", tip, |ui| {
        let mut n = f32::from(p.at_once);
        let max = f32::from(ModelPreset::AT_ONCE_MAX);
        widgets::stepper(ui, &mut n, 1.0..=max, 1.0, |v| format!("{v:.0}"), "at once");
        p.at_once = (n.round() as u8).clamp(1, ModelPreset::AT_ONCE_MAX);
    });
}

/// Apple only where it runs; a switch forgets the old provider's model.
fn provider_picker(ui: &mut Ui, p: &mut ModelPreset) {
    let mac = cfg!(target_os = "macos");
    let before = p.provider;
    widgets::combo(("ai-provider", p.id.0))
        .width(PICK_W)
        .selected_text(p.provider.label())
        .show_ui(ui, |ui| {
            let shown = Provider::ALL
                .into_iter()
                .filter(|q| mac || *q != Provider::Apple || *q == before);
            for q in shown {
                ui.selectable_value(&mut p.provider, q, q.label())
                    .on_hover_text(q.detail());
            }
        });
    if p.provider != before {
        p.model.clear();
    }
}

fn model_row(ui: &mut Ui, p: &mut ModelPreset, board: Option<&Board>, out: &mut Commands) {
    let t = Tokens::get(ui.ctx());
    let server = p.provider;
    let lost = board.is_some_and(|b| missing(&p.model, b.models(server)));
    let gone = format!(
        "{} isn't on the server now; choose another.",
        p.model.trim()
    );
    let tip = if lost {
        gone.as_str()
    } else {
        "Blank uses the default"
    };
    row(ui, "Model", tip, |ui| {
        if let Some(b) = board {
            refresh(ui, server, b, out);
            model_menu(ui, server, &mut p.model, b.models(server), out);
        }
        let hint = match server.default_model() {
            "" => "The server's first",
            m => m,
        };
        let mut edit = TextEdit::singleline(&mut p.model).hint_text(hint);
        if lost {
            edit = edit.text_color(t.muted);
        }
        let r = field(ui, edit);
        if lost {
            widgets::tip(&r, &gone, None);
        }
    });
    if lost {
        ui.label(RichText::new(&gone).size(t.text_small).color(t.danger));
        ui.add_space(8.0);
    }
}

/// How much room the model has, and the honest catch with keys.
fn room_note(ui: &mut Ui, p: &ModelPreset, board: Option<&Board>) {
    if !p.provider.takes_keys() {
        return;
    }
    let keys = board.and_then(|b| b.keys(p.provider).ready());
    let Some(keys) = keys.filter(|k| !k.is_empty()) else {
        return;
    };
    let n = lanes(keys);
    let total = u32::from(p.at_once).saturating_mul(n);
    let keys = if n == 1 {
        "1 usable key"
    } else {
        "usable keys"
    };
    let said = match n {
        1 => format!("With {keys}, up to {total} at once."),
        _ => format!(
            "With {n} {keys}, up to {total} at once; requests take turns. \
             Keys from one account share its rate limits."
        ),
    };
    let t = Tokens::get(ui.ctx());
    ui.label(RichText::new(said).size(t.text_small).color(t.muted));
    ui.add_space(8.0);
}

fn tried(ui: &mut Ui, tried: &Load<mareader_assist::Tried>) {
    let t = Tokens::get(ui.ctx());
    let muted = |s: String| RichText::new(s).size(t.text_small).color(t.muted);
    match tried {
        Load::Idle => {}
        Load::Busy => {
            ui.add_space(4.0);
            ui.label(muted("Asking…".to_owned()));
        }
        Load::Ready(tried) => {
            let secs = tried.millis as f64 / 1000.0;
            ui.add_space(4.0);
            ui.label(muted(format!("Works, in {secs:.1} s: “{}”", tried.meaning)));
        }
        Load::Failed(f) => {
            ui.add_space(4.0);
            failure(ui, f);
        }
    }
}

/// Whether the server's list lacks `model`; Ollama may add ":latest".
fn missing(model: &str, models: &Load<Vec<String>>) -> bool {
    let model = model.trim();
    let Load::Ready(list) = models else {
        return false;
    };
    !model.is_empty() && !list.iter().any(|m| same(model, m))
}

/// Whether `model` names `listed`; Ollama reads "x" as "x:latest".
fn same(model: &str, listed: &str) -> bool {
    let model = model.trim();
    listed == model || listed.strip_suffix(":latest") == Some(model)
}

/// Asks the server for its models again; a spinner while it answers.
fn refresh(ui: &mut Ui, p: Provider, board: &Board, out: &mut Commands) {
    if board.fetching(p) {
        let r = ui.add_sized(vec2(24.0, 24.0), egui::Spinner::new().size(12.0));
        widgets::name(&r, "Refreshing models");
    } else if widgets::small_icon_button(ui, Icon::Rescan, "Refresh models").clicked() {
        out.push(Sent::Engine(Command::Ai(Job::Models(p))));
    }
}

/// The server's models, fetched when the menu first opens.
fn model_menu(
    ui: &mut Ui,
    p: Provider,
    model: &mut String,
    models: &Load<Vec<String>>,
    out: &mut Commands,
) {
    let menu = ui.menu_button("Choose", |ui| {
        ui.set_max_width(FIELD_W);
        match models {
            Load::Idle | Load::Busy => {
                ui.label("Loading models…");
            }
            Load::Failed(f) => {
                failure(ui, f);
                if ui.button("Try again").clicked() {
                    out.push(Sent::Engine(Command::Ai(Job::Models(p))));
                }
            }
            Load::Ready(list) if list.is_empty() => {
                ui.label("The server lists no models.");
            }
            Load::Ready(list) => {
                ScrollArea::vertical().max_height(260.0).show(ui, |ui| {
                    for id in list {
                        if ui.selectable_label(same(model, id), id).clicked() {
                            model.clone_from(id);
                            ui.close();
                        }
                    }
                });
            }
        }
    });
    if menu.response.clicked() && matches!(models, Load::Idle | Load::Failed(_)) {
        out.push(Sent::Engine(Command::Ai(Job::Models(p))));
    }
}

fn key_row(ui: &mut Ui, p: Provider, r: &KeyRow, out: &mut Commands) {
    let t = Tokens::get(ui.ctx());
    let (state, banned) = match &r.state {
        KeyState::Ready => ("Ready".to_owned(), false),
        KeyState::Resting { secs } => (format!("Resting, {secs} s"), false),
        KeyState::Banned { reason } => (format!("Set aside: {reason}"), true),
    };
    row(ui, &r.shown, &state, |ui| {
        let job = |id: &str| (p, id.to_owned());
        if widgets::small_icon_button(ui, Icon::Remove, "Remove key").clicked() {
            let (p, id) = job(&r.id);
            out.push(Sent::Engine(Command::Ai(Job::RemoveKey(p, id))));
        }
        let (icon, label) = match banned {
            true => (Icon::Restore, "Use again"),
            false => (Icon::Ban, "Set aside"),
        };
        if widgets::small_icon_button(ui, icon, label).clicked() {
            let (p, id) = job(&r.id);
            out.push(Sent::Engine(Command::Ai(match banned {
                true => Job::RecoverKey(p, id),
                false => Job::BanKey(p, id),
            })));
        }
        let color = if banned { t.danger } else { t.muted };
        ui.label(
            RichText::new(state.as_str())
                .size(t.text_small)
                .color(color),
        );
    });
}

/// A field for a new key; it empties once the key is sent.
fn add_key(ui: &mut Ui, p: Provider, out: &mut Commands) {
    let id = egui::Id::new(("ai-new-key", p.scope()));
    let mut text: String = ui.data_mut(|d| d.get_temp(id)).unwrap_or_default();
    ui.allocate_ui_with_layout(
        vec2(ui.available_width(), super::ROW_H),
        Layout::left_to_right(Align::Center),
        |ui| {
            let t = Tokens::get(ui.ctx());
            ui.add(Icon::Key.image(ui, 15.0).tint(t.muted));
            let edit = TextEdit::singleline(&mut text)
                .password(true)
                .hint_text("Paste an API key");
            let field = field(ui, edit);
            let entered = field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            let add = ui.add_enabled(!text.trim().is_empty(), egui::Button::new("Add"));
            if (add.clicked() || entered) && !text.trim().is_empty() {
                out.push(Sent::Engine(Command::Ai(Job::AddKey(
                    p,
                    std::mem::take(&mut text),
                ))));
            }
        },
    );
    ui.data_mut(|d| d.insert_temp(id, text));
}

/// A text field outlined like the appearance menu's.
fn field(ui: &mut Ui, edit: TextEdit<'_>) -> egui::Response {
    let t = Tokens::get(ui.ctx());
    let frame = egui::Frame::new()
        .stroke(Stroke::new(1.0, t.line))
        .corner_radius(6)
        .inner_margin(egui::Margin::symmetric(8, 4));
    ui.add(edit.frame(frame).desired_width(FIELD_W - 16.0))
}

fn failure(ui: &mut Ui, f: &Failure) {
    let t = Tokens::get(ui.ctx());
    ui.vertical(|ui| {
        ui.label(RichText::new(f.why.message()).color(t.danger));
        if let Some(detail) = &f.detail {
            ui.label(RichText::new(detail).size(t.text_small).color(t.muted));
        }
    });
}

#[cfg(test)]
mod tests {
    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable as _;
    use mareader_assist::Reply;

    use super::*;

    #[test]
    fn a_model_off_the_list_is_missing() {
        let list = Load::Ready(vec!["llama3.2:latest".to_owned()]);
        assert!(!missing("llama3.2", &list));
        assert!(!missing("llama3.2:latest", &list));
        assert!(!missing("", &list));
        assert!(missing("qwen3:4b", &list));
        // Unknown until the server answers.
        assert!(!missing("qwen3:4b", &Load::Busy));
    }

    struct State {
        ai: AiSettings,
        board: Board,
        out: Commands,
        shown: bool,
        fonts: bool,
    }

    fn asked(s: &State) -> usize {
        let models = |c: &&Sent| matches!(c, Sent::Engine(Command::Ai(Job::Models(_))));
        s.out.iter().filter(models).count()
    }

    fn harness(ai: AiSettings) -> Harness<'static, State> {
        let state = State {
            ai,
            board: Board::default(),
            out: Vec::new(),
            shown: true,
            fonts: false,
        };
        // Tall enough for every section without scrolling.
        Harness::builder()
            .with_size(egui::vec2(800.0, 2400.0))
            .build_ui_state(
                |ui, s: &mut State| {
                    // Named fonts work from the frame after they're installed.
                    if !s.fonts {
                        crate::theme::install_fonts(ui.ctx());
                        s.fonts = true;
                        ui.ctx().request_repaint();
                    } else if s.shown {
                        let live = Live {
                            board: &s.board,
                            in_use: &|_| 0,
                        };
                        models_tab(ui, &mut s.ai, Some(live), &mut s.out);
                    }
                },
                state,
            )
    }

    fn no_models() -> AiSettings {
        AiSettings {
            presets: Vec::new(),
            tasks: Default::default(),
            ..AiSettings::default()
        }
    }

    #[test]
    fn opening_the_tab_asks_ollama_for_its_models() {
        let mut ai = no_models();
        ai.add_preset(Provider::Ollama, "llama3:8b");
        let mut h = harness(ai);
        h.step();
        h.step();
        // This board never starts the job, so each frame asks again.
        assert!(asked(h.state()) > 0);
        let p = Provider::Ollama;
        let s = h.state_mut();
        s.board.start(&Job::Models(p));
        let list = vec!["qwen3:4b".to_owned()];
        s.board.finish(Reply::Models(p, Ok(list)));
        s.out.clear();
        h.step();
        h.step();
        assert_eq!(asked(h.state()), 0, "no refetch while the tab stays open");
        // Opened, the model says it's gone from the server.
        h.get_by_label("Ollama · llama3:8b").click();
        h.step();
        h.step();
        h.get_by_label("Refresh models");
        h.get_by_label("llama3:8b isn't on the server now; choose another.");
        h.state_mut().shown = false;
        h.step();
        h.state_mut().shown = true;
        h.step();
        assert_eq!(asked(h.state()), 1, "reopening fetches again");
    }

    #[test]
    fn a_named_model_keeps_its_name_and_an_unnamed_one_follows_its_model() {
        let mut ai = no_models();
        let id = ai.add_preset(Provider::OpenAi, "");
        let mut p = ai.preset(id).cloned().unwrap_or_default();
        renaming(&mut p, |p| p.model = "gpt-4.1-nano".into());
        assert_eq!(p.name, "OpenAI · gpt-4.1-nano");
        renaming(&mut p, |p| p.provider = Provider::OpenRouter);
        assert_eq!(p.name, "OpenRouter · gpt-4.1-nano");
        p.name = "Fast".into();
        renaming(&mut p, |p| p.model = "gpt-4.1".into());
        assert_eq!(p.name, "Fast");
    }

    #[test]
    fn the_first_model_added_explains_words() {
        let mut h = harness(no_models());
        h.step();
        h.step();
        // With none saved, the providers wait as tiles.
        h.get_by_label("Custom server").click();
        h.step();
        h.step();
        let ai = &h.state().ai;
        let made = ai.explain_preset().map(|p| p.provider);
        assert_eq!(made, Some(Provider::Custom));
        // Its provider's keys show below, and it opened.
        h.get_by_label("Add");
        h.get_by_label("Try it");
    }

    #[test]
    fn a_duplicate_takes_a_fresh_name_and_the_same_room() {
        let mut ai = no_models();
        let id = ai.add_preset(Provider::OpenAi, "gpt-4.1-mini");
        if let Some(p) = ai.preset_mut(id) {
            p.at_once = 3;
        }
        let one = duplicate(&mut ai, id).and_then(|c| ai.preset(c).cloned());
        let two = duplicate(&mut ai, id).and_then(|c| ai.preset(c).cloned());
        let names = [&one, &two].map(|p| p.as_ref().map(ModelPreset::title));
        let want = ["OpenAI · gpt-4.1-mini (2)", "OpenAI · gpt-4.1-mini (3)"];
        assert_eq!(names, want.map(|n| Some(n.to_owned())));
        assert_eq!(one.map(|p| p.at_once), Some(3));
    }

    #[test]
    fn the_pills_name_every_task_a_model_runs() {
        let mut ai = no_models();
        let first = ai.add_preset(Provider::OpenAi, "");
        let local = ai.add_preset(Provider::Ollama, "llama3.2");
        ai.tasks.explain.preset = Some(first);
        ai.tasks.summarize.preset = Some(local);
        // Tasks with none of their own fall back to the AI Dictionary's.
        let want = ["AI Dictionary", "Library", "Translate", "Simplify"];
        assert_eq!(uses(&ai, first), want);
        assert_eq!(uses(&ai, local), ["Summarize"]);
    }
}
