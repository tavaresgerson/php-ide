//! Tabs, tree rows, list rows and menu items.

use eframe::egui::{self, Color32, CornerRadius, Response, Sense, Stroke};
use phpide_icons::IconId;

use super::{
    body_font, focus_ring, galley, galley_truncated, paint_icon, small_font, state_of, theme, State,
};
use crate::design::tokens::{radius, space};

pub struct TabResponse {
    pub response: Response,
    pub close_clicked: bool,
}

/// Editor tab: icon, label (`*` before it when modified, RF-TAB-09),
/// close button and an accent underline on the active tab.
pub struct Tab {
    label: String,
    icon: Option<(IconId, Color32)>,
    active: bool,
    modified: bool,
    closable: bool,
    pinned: bool,
    preview: bool,
    color: Option<Color32>,
    /// Close button on the left (RF-TAB-11).
    close_left: bool,
    forced: Option<State>,
}

impl Tab {
    pub fn new(label: impl Into<String>) -> Self {
        Tab {
            label: label.into(),
            icon: None,
            active: false,
            modified: false,
            closable: true,
            pinned: false,
            preview: false,
            color: None,
            close_left: false,
            forced: None,
        }
    }

    /// Puts the close button before the name.
    pub fn close_left(mut self, left: bool) -> Self {
        self.close_left = left;
        self
    }

    pub fn icon(mut self, icon: IconId, color: Color32) -> Self {
        self.icon = Some((icon, color));
        self
    }

    /// Label color (a file's Git status, RF-TAB-12).
    pub fn label_color(mut self, color: Option<Color32>) -> Self {
        self.color = color;
        self
    }

    /// Pinned tabs show a pin instead of the close button.
    pub fn pinned(mut self, pinned: bool) -> Self {
        self.pinned = pinned;
        self
    }

    /// Preview tabs (replaced by the next file opened) use an italic label.
    pub fn preview(mut self, preview: bool) -> Self {
        self.preview = preview;
        self
    }

    pub fn active(mut self, active: bool) -> Self {
        self.active = active;
        self
    }

    pub fn modified(mut self, modified: bool) -> Self {
        self.modified = modified;
        self
    }

    pub fn closable(mut self, closable: bool) -> Self {
        self.closable = closable;
        self
    }

    pub fn force(mut self, state: State) -> Self {
        self.forced = Some(state);
        self
    }

    pub fn show(self, ui: &mut egui::Ui) -> TabResponse {
        let theme = theme(ui);
        let p = theme.palette;
        let m = theme.metrics;
        let fg = match self.color {
            Some(color) => color,
            None if self.active => p.text_primary,
            None => p.text_secondary,
        };
        let shown_label = if self.modified {
            format!("*{}", self.label)
        } else {
            self.label.clone()
        };
        let text = {
            let mut job = egui::text::LayoutJob::default();
            job.append(
                &shown_label,
                0.0,
                egui::TextFormat {
                    font_id: body_font(),
                    color: fg,
                    italics: self.preview,
                    ..Default::default()
                },
            );
            ui.ctx().fonts_mut(|fonts| fonts.layout_job(job))
        };
        let icon_space = if self.icon.is_some() {
            m.icon + space::S3
        } else {
            0.0
        };
        let close_side = m.icon;
        let close_space = if self.closable {
            close_side + space::S3
        } else {
            0.0
        };
        let width = space::S5 + icon_space + text.size().x + close_space + space::S4;
        let (rect, response) =
            ui.allocate_exact_size(egui::vec2(width, m.tab_strip), Sense::click());
        let (label, active) = (self.label.clone(), self.active);
        response.widget_info(|| {
            egui::WidgetInfo::selected(egui::WidgetType::Button, true, active, &label)
        });
        let state = state_of(&response, true, self.forced);
        let close_x = if self.close_left {
            rect.left() + space::S4 + close_side / 2.0
        } else {
            rect.right() - space::S4 - close_side / 2.0
        };
        let close_rect = egui::Rect::from_center_size(
            egui::pos2(close_x, rect.center().y),
            egui::vec2(close_side, close_side),
        );
        let mut close_clicked = false;
        if self.closable && !self.pinned {
            let close = ui.interact(close_rect, response.id.with("close"), Sense::click());
            let close_label = format!("Close {}", self.label);
            close.widget_info(|| {
                egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &close_label)
            });
            close_clicked = close.clicked() || response.middle_clicked();
            if close.hovered() {
                ui.painter().rect_filled(
                    close_rect.expand(2.0),
                    CornerRadius::same(radius::CONTROL),
                    p.bg_pressed,
                );
            }
        }
        if ui.is_rect_visible(rect) {
            let hovered = matches!(state, State::Hovered | State::Pressed);
            let fill = if self.active {
                p.bg_editor
            } else if hovered {
                p.bg_hover
            } else {
                Color32::TRANSPARENT
            };
            ui.painter().rect_filled(rect, CornerRadius::ZERO, fill);
            if self.active {
                let underline = egui::Rect::from_min_max(
                    egui::pos2(rect.left() + space::S1, rect.bottom() - 2.0),
                    egui::pos2(rect.right() - space::S1, rect.bottom()),
                );
                ui.painter()
                    .rect_filled(underline, CornerRadius::same(1), p.accent);
            }
            let mut x = rect.left() + space::S5;
            if self.close_left {
                x = rect.left() + space::S4 + close_space;
            }
            if let Some((icon, color)) = self.icon {
                let icon_rect = egui::Rect::from_center_size(
                    egui::pos2(x + m.icon / 2.0, rect.center().y),
                    egui::vec2(m.icon, m.icon),
                );
                paint_icon(ui.painter(), icon_rect, icon, color);
                x += icon_space;
            }
            ui.painter().galley(
                egui::pos2(x, rect.center().y - text.size().y / 2.0),
                text,
                fg,
            );
            if self.pinned {
                paint_icon(
                    ui.painter(),
                    close_rect.shrink(2.0),
                    IconId::Pin,
                    p.text_muted,
                );
            } else if self.closable && (hovered || self.active) {
                paint_icon(
                    ui.painter(),
                    close_rect.shrink(2.0),
                    IconId::Close,
                    p.text_muted,
                );
            }
            if state == State::Focused {
                focus_ring(ui.painter(), rect.shrink(2.0), radius::CONTROL, &p);
            }
        }
        TabResponse {
            response,
            close_clicked,
        }
    }
}

pub struct TreeResponse {
    pub response: Response,
    /// The disclosure arrow was clicked.
    pub toggled: bool,
}

/// Row of a tree: indentation, disclosure arrow, icon, label and detail.
pub struct TreeRow {
    label: String,
    depth: usize,
    expanded: Option<bool>,
    icon: Option<(IconId, Color32)>,
    label_color: Option<Color32>,
    detail: Option<String>,
    selected: bool,
    focused: bool,
    forced: Option<State>,
    highlight: Option<(usize, usize)>,
    /// Vertical guides for each level of indentation (RF-NAV-13).
    guides: bool,
}

impl TreeRow {
    pub fn guides(mut self, guides: bool) -> Self {
        self.guides = guides;
        self
    }

    pub fn new(label: impl Into<String>) -> Self {
        TreeRow {
            label: label.into(),
            depth: 0,
            expanded: None,
            icon: None,
            label_color: None,
            detail: None,
            selected: false,
            focused: false,
            forced: None,
            highlight: None,
            guides: false,
        }
    }

    /// Chars `[start, end)` of the label matched by a speed search.
    pub fn highlight(mut self, range: Option<(usize, usize)>) -> Self {
        self.highlight = range;
        self
    }

    pub fn depth(mut self, depth: usize) -> Self {
        self.depth = depth;
        self
    }

    /// `Some(open)` for expandable rows, `None` for leaves.
    pub fn expanded(mut self, expanded: Option<bool>) -> Self {
        self.expanded = expanded;
        self
    }

    pub fn icon(mut self, icon: IconId, color: Color32) -> Self {
        self.icon = Some((icon, color));
        self
    }

    /// Label color (for example a Git status color).
    pub fn label_color(mut self, color: Color32) -> Self {
        self.label_color = Some(color);
        self
    }

    pub fn detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = Some(detail.into());
        self
    }

    pub fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }

    /// Whether the tree has keyboard focus (stronger selection color).
    pub fn focused(mut self, focused: bool) -> Self {
        self.focused = focused;
        self
    }

    pub fn force(mut self, state: State) -> Self {
        self.forced = Some(state);
        self
    }

    pub fn show(self, ui: &mut egui::Ui) -> TreeResponse {
        let theme = theme(ui);
        let p = theme.palette;
        let m = theme.metrics;
        let indent = ui.spacing().indent;
        let width = ui.available_width();
        let (rect, response) = ui.allocate_exact_size(egui::vec2(width, m.row), Sense::click());
        let (label, selected) = (self.label.clone(), self.selected);
        let detail = self.detail.clone();
        response.widget_info(|| {
            let mut info = egui::WidgetInfo::selected(
                egui::WidgetType::SelectableLabel,
                true,
                selected,
                &label,
            );
            // The detail ("75% lines covered") is read out as the value.
            info.current_text_value = detail.clone();
            info
        });
        let state = state_of(&response, true, self.forced);
        if self.guides && ui.is_rect_visible(rect) {
            for level in 0..self.depth {
                let x =
                    (rect.left() + space::S2 + level as f32 * indent + m.icon / 2.0).round() + 0.5;
                ui.painter().line_segment(
                    [egui::pos2(x, rect.top()), egui::pos2(x, rect.bottom())],
                    egui::Stroke::new(1.0, p.border_subtle),
                );
            }
        }
        let arrow_rect = egui::Rect::from_center_size(
            egui::pos2(
                rect.left() + space::S2 + self.depth as f32 * indent + m.icon / 2.0,
                rect.center().y,
            ),
            egui::vec2(m.icon, m.icon),
        );
        let mut toggled = false;
        if self.expanded.is_some() {
            let arrow = ui.interact(arrow_rect, response.id.with("arrow"), Sense::click());
            toggled = arrow.clicked();
        }
        if ui.is_rect_visible(rect) {
            let fill = if self.selected && (self.focused || state == State::Focused) {
                p.bg_selected_focused
            } else if self.selected {
                p.bg_selected
            } else if matches!(state, State::Hovered | State::Pressed) {
                p.bg_hover
            } else {
                Color32::TRANSPARENT
            };
            ui.painter()
                .rect_filled(rect, CornerRadius::same(radius::CONTROL), fill);
            if let Some(open) = self.expanded {
                let chevron = if open {
                    IconId::ChevronDown
                } else {
                    IconId::ChevronRight
                };
                paint_icon(ui.painter(), arrow_rect.shrink(2.0), chevron, p.text_muted);
            }
            let mut x = arrow_rect.right() + space::S2;
            if let Some((icon, color)) = self.icon {
                let icon_rect = egui::Rect::from_center_size(
                    egui::pos2(x + m.icon / 2.0, rect.center().y),
                    egui::vec2(m.icon, m.icon),
                );
                paint_icon(ui.painter(), icon_rect, icon, color);
                x += m.icon + space::S3;
            }
            let fg = self.label_color.unwrap_or(p.text_primary);
            let available = rect.right() - x - space::S4;
            let text = galley_truncated(ui, self.label.clone(), body_font(), fg, available);
            let text_width = text.size().x;
            let text_pos = egui::pos2(x, rect.center().y - text.size().y / 2.0);
            if let Some((start, end)) = self.highlight {
                let from = text.pos_from_cursor(egui::text::CCursor::new(start));
                let to = text.pos_from_cursor(egui::text::CCursor::new(end));
                let mark = egui::Rect::from_min_max(
                    text_pos + egui::vec2(from.left(), from.top()),
                    text_pos + egui::vec2(to.left(), to.bottom()),
                )
                .expand2(egui::vec2(1.0, 0.0));
                ui.painter().rect_filled(
                    mark,
                    CornerRadius::same(radius::CONTROL / 2),
                    p.search_match_bg,
                );
            }
            ui.painter().galley(text_pos, text, fg);
            if let Some(detail) = self.detail {
                let detail_x = x + text_width + space::S4;
                let detail = galley_truncated(
                    ui,
                    detail,
                    body_font(),
                    p.text_muted,
                    rect.right() - detail_x - space::S4,
                );
                ui.painter().galley(
                    egui::pos2(detail_x, rect.center().y - detail.size().y / 2.0),
                    detail,
                    p.text_muted,
                );
            }
            if state == State::Focused && !self.selected {
                focus_ring(ui.painter(), rect.shrink(1.0), radius::CONTROL, &p);
            }
        }
        TreeResponse { response, toggled }
    }
}

/// Row of a flat list: icon, label, secondary detail and trailing text
/// (a shortcut, a count, a location).
pub struct ListRow {
    label: String,
    icon: Option<(IconId, Color32)>,
    detail: Option<String>,
    trailing: Option<String>,
    selected: bool,
    forced: Option<State>,
}

impl ListRow {
    pub fn new(label: impl Into<String>) -> Self {
        ListRow {
            label: label.into(),
            icon: None,
            detail: None,
            trailing: None,
            selected: false,
            forced: None,
        }
    }

    pub fn icon(mut self, icon: IconId, color: Color32) -> Self {
        self.icon = Some((icon, color));
        self
    }

    pub fn detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = Some(detail.into());
        self
    }

    pub fn trailing(mut self, trailing: impl Into<String>) -> Self {
        self.trailing = Some(trailing.into());
        self
    }

    pub fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }

    pub fn force(mut self, state: State) -> Self {
        self.forced = Some(state);
        self
    }

    pub fn show(self, ui: &mut egui::Ui) -> Response {
        let theme = theme(ui);
        let p = theme.palette;
        let m = theme.metrics;
        let width = ui.available_width();
        let (rect, response) = ui.allocate_exact_size(egui::vec2(width, m.row), Sense::click());
        let (label, selected) = (self.label.clone(), self.selected);
        response.widget_info(|| {
            egui::WidgetInfo::selected(egui::WidgetType::SelectableLabel, true, selected, &label)
        });
        let state = state_of(&response, true, self.forced);
        if ui.is_rect_visible(rect) {
            let fill = if self.selected {
                p.bg_selected_focused
            } else if matches!(state, State::Hovered | State::Pressed) {
                p.bg_hover
            } else {
                Color32::TRANSPARENT
            };
            ui.painter()
                .rect_filled(rect, CornerRadius::same(radius::CONTROL), fill);
            let mut x = rect.left() + space::S4;
            if let Some((icon, color)) = self.icon {
                let icon_rect = egui::Rect::from_center_size(
                    egui::pos2(x + m.icon / 2.0, rect.center().y),
                    egui::vec2(m.icon, m.icon),
                );
                paint_icon(ui.painter(), icon_rect, icon, color);
                x += m.icon + space::S3;
            }
            let mut right = rect.right() - space::S4;
            if let Some(trailing) = self.trailing {
                let text = galley(ui, trailing, small_font(), p.text_muted);
                right -= text.size().x;
                ui.painter().galley(
                    egui::pos2(right, rect.center().y - text.size().y / 2.0),
                    text,
                    p.text_muted,
                );
                right -= space::S4;
            }
            let text = galley_truncated(ui, self.label, body_font(), p.text_primary, right - x);
            let text_width = text.size().x;
            ui.painter().galley(
                egui::pos2(x, rect.center().y - text.size().y / 2.0),
                text,
                p.text_primary,
            );
            if let Some(detail) = self.detail {
                let detail_x = x + text_width + space::S3;
                let detail =
                    galley_truncated(ui, detail, body_font(), p.text_muted, right - detail_x);
                ui.painter().galley(
                    egui::pos2(detail_x, rect.center().y - detail.size().y / 2.0),
                    detail,
                    p.text_muted,
                );
            }
            if state == State::Focused {
                focus_ring(ui.painter(), rect.shrink(1.0), radius::CONTROL, &p);
            }
        }
        response
    }
}

/// Frame of a popover menu (for menus drawn inline, like the gallery).
pub fn menu_frame(ui: &egui::Ui) -> egui::Frame {
    let theme = theme(ui);
    let p = theme.palette;
    egui::Frame::new()
        .fill(p.bg_elevated)
        .stroke(Stroke::new(1.0, p.border_subtle))
        .corner_radius(CornerRadius::same(radius::CARD))
        .inner_margin(egui::Margin::same(theme.metrics.popover_padding as i8))
        .shadow(ui.visuals().popup_shadow)
}

/// Menu row: optional icon, label and right-aligned shortcut.
///
/// The icon column is always reserved, so rows with and without an icon
/// line up (as in the IntelliJ menus).
pub fn menu_item(
    ui: &mut egui::Ui,
    icon: Option<IconId>,
    label: &str,
    shortcut: &str,
    enabled: bool,
) -> Response {
    MenuRow::new(icon, label)
        .shortcut(shortcut)
        .enabled(enabled)
        .show(ui)
}

/// Menu row that turns something on or off (a tool window, a view option).
/// Checked, its icon sits in a highlighted box; without an icon, a check
/// mark takes the icon column. Read as a checkbox by screen readers.
pub fn menu_check_item(
    ui: &mut egui::Ui,
    icon: Option<IconId>,
    label: &str,
    shortcut: &str,
    checked: bool,
) -> Response {
    MenuRow::new(icon, label)
        .shortcut(shortcut)
        .checked(Some(checked), egui::WidgetType::Checkbox)
        .show(ui)
}

/// One choice of a list (encoding, line separator, scope): a check mark in
/// the icon column on the chosen one. Read as a radio button.
pub fn menu_radio_item(ui: &mut egui::Ui, label: &str, selected: bool, enabled: bool) -> Response {
    MenuRow::new(None, label)
        .enabled(enabled)
        .checked(Some(selected), egui::WidgetType::RadioButton)
        .show(ui)
}

/// Menu row that opens a submenu on hover or click, with the same icon
/// column as [`menu_item`] and a chevron on the right. Returns what
/// `content` returned while the submenu is open.
pub fn menu_submenu<R>(
    ui: &mut egui::Ui,
    icon: Option<IconId>,
    label: &str,
    enabled: bool,
    content: impl FnOnce(&mut egui::Ui) -> R,
) -> Option<R> {
    use egui::containers::menu::{MenuState, SubMenu};
    // The row takes the next auto id; the submenu state is keyed by it.
    let id = ui.next_auto_id();
    let open = MenuState::from_ui(ui, |state, _| {
        state.open_item == Some(SubMenu::id_from_widget_id(id))
    });
    let mut row = MenuRow::new(icon, label).enabled(enabled);
    row.submenu = true;
    row.open = open;
    let response = row.show(ui);
    if !enabled {
        return None;
    }
    SubMenu::new()
        .show(ui, &response, content)
        .map(|inner| inner.inner)
}

struct MenuRow<'a> {
    icon: Option<IconId>,
    label: &'a str,
    shortcut: &'a str,
    enabled: bool,
    submenu: bool,
    open: bool,
    checked: Option<bool>,
    widget_type: egui::WidgetType,
}

impl<'a> MenuRow<'a> {
    fn new(icon: Option<IconId>, label: &'a str) -> Self {
        MenuRow {
            icon,
            label,
            shortcut: "",
            enabled: true,
            submenu: false,
            open: false,
            checked: None,
            widget_type: egui::WidgetType::Button,
        }
    }

    fn shortcut(mut self, shortcut: &'a str) -> Self {
        self.shortcut = shortcut;
        self
    }

    fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    fn checked(mut self, checked: Option<bool>, widget_type: egui::WidgetType) -> Self {
        self.checked = checked;
        self.widget_type = widget_type;
        self
    }

    fn show(self, ui: &mut egui::Ui) -> Response {
        let theme = theme(ui);
        let p = theme.palette;
        let m = theme.metrics;
        let enabled = self.enabled;
        let fg = if enabled {
            p.text_primary
        } else {
            p.text_disabled
        };
        let text = galley(ui, self.label, body_font(), fg);
        let keys = (!self.shortcut.is_empty())
            .then(|| galley(ui, self.shortcut, small_font(), p.text_muted));
        let chevron = m.icon * 0.75;
        let icon_column = m.icon + space::S3;
        let trailing = if self.submenu {
            chevron + space::S7
        } else {
            keys.as_ref().map_or(0.0, |k| k.size().x + space::S7)
        };
        let min_width = space::S4 + icon_column + text.size().x + trailing + space::S4;
        // In a menu's sizing pass the row asks only for what it needs; it
        // fills the menu's width afterwards. (Taking the available width
        // then would size every menu at egui's default area width.)
        let width = if ui.is_sizing_pass() {
            min_width
        } else {
            ui.available_width().max(min_width)
        };
        let sense = if enabled {
            Sense::click()
        } else {
            Sense::hover()
        };
        let (rect, response) = ui.allocate_exact_size(egui::vec2(width, m.row - 2.0), sense);
        let label = self.label.to_owned();
        let (widget_type, checked) = (self.widget_type, self.checked);
        response.widget_info(|| match checked {
            Some(checked) => egui::WidgetInfo::selected(widget_type, enabled, checked, &label),
            None => egui::WidgetInfo::labeled(widget_type, enabled, &label),
        });
        if ui.is_rect_visible(rect) {
            if enabled && (self.open || response.hovered() || response.has_focus()) {
                ui.painter().rect_filled(
                    rect,
                    CornerRadius::same(radius::CONTROL),
                    p.bg_selected_focused,
                );
            }
            let secondary = if enabled {
                p.text_secondary
            } else {
                p.text_disabled
            };
            let x = rect.left() + space::S4;
            let icon_rect = egui::Rect::from_center_size(
                egui::pos2(x + m.icon / 2.0, rect.center().y),
                egui::vec2(m.icon, m.icon),
            );
            let checked = self.checked == Some(true);
            match self.icon {
                Some(icon) => {
                    if checked {
                        ui.painter().rect(
                            icon_rect.expand(3.0),
                            CornerRadius::same(radius::CONTROL),
                            p.bg_selected,
                            Stroke::new(1.0, p.accent),
                            egui::StrokeKind::Inside,
                        );
                    }
                    paint_icon(ui.painter(), icon_rect, icon, secondary);
                }
                None if checked => {
                    paint_icon(ui.painter(), icon_rect, IconId::Check, p.accent);
                }
                None => {}
            }
            ui.painter().galley(
                egui::pos2(x + icon_column, rect.center().y - text.size().y / 2.0),
                text,
                fg,
            );
            if self.submenu {
                let chevron_rect = egui::Rect::from_center_size(
                    egui::pos2(rect.right() - space::S4 - chevron / 2.0, rect.center().y),
                    egui::vec2(chevron, chevron),
                );
                paint_icon(ui.painter(), chevron_rect, IconId::ChevronRight, secondary);
            } else if let Some(keys) = keys {
                ui.painter().galley(
                    egui::pos2(
                        rect.right() - space::S4 - keys.size().x,
                        rect.center().y - keys.size().y / 2.0,
                    ),
                    keys,
                    p.text_muted,
                );
            }
        }
        response
    }
}

pub fn menu_separator(ui: &mut egui::Ui) {
    let p = theme(ui).palette;
    let width = if ui.is_sizing_pass() {
        0.0
    } else {
        ui.available_width()
    };
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, space::S3 + 1.0), Sense::hover());
    ui.painter().hline(
        rect.x_range().shrink(space::S2),
        rect.center().y,
        Stroke::new(1.0, p.border_subtle),
    );
}
