//! Logs Console Tab: dedicated filterable log stream with color-coded severity.

use iced::{
    widget::{button, column, container, row, scrollable, text, text_input, Space},
    Alignment, Element, Length,
};
use crate::ui::theme::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogLevel {
    All,
    Info,
    Warn,
    Error,
}

#[derive(Debug, Clone)]
pub struct LogEntry {
    pub level: LogLevel,
    pub timestamp: String,
    pub message: String,
}

pub fn view_logs<'a, Message>(
    entries: &'a [LogEntry],
    selected_level: LogLevel,
    search_query: &'a str,
    on_select_level: impl Fn(LogLevel) -> Message + 'a + Copy,
    on_search_change: impl Fn(String) -> Message + 'a + Copy,
    on_clear: Message,
) -> Element<'a, Message>
where
    Message: 'a + Clone,
{
    // 1. Controls Bar: Level filters, Search input, Clear
    let level_btn = |level: LogLevel, label: &'static str| {
        let active = selected_level == level;
        button(text(label).font(FONT_MEDIUM).size(11))
            .padding([4, 12])
            .on_press(on_select_level(level))
            .style(move |theme, status| style_tab_button(theme, status, active))
    };

    let level_bar = row![
        level_btn(LogLevel::All, "All"),
        level_btn(LogLevel::Info, "Info"),
        level_btn(LogLevel::Warn, "Warnings"),
        level_btn(LogLevel::Error, "Errors"),
    ]
    .spacing(4);

    let search_input = text_input("Filter log output...", search_query)
        .on_input(on_search_change)
        .padding(6)
        .size(12)
        .width(180)
        .style(style_text_input);

    let btn_clear = button(text("Clear").font(FONT_MEDIUM).size(11))
        .padding([4, 12])
        .on_press(on_clear)
        .style(style_button_secondary);

    let toolbar = row![
        level_bar,
        Space::new().width(Length::Fill),
        search_input,
        btn_clear,
    ]
    .spacing(8)
    .align_y(Alignment::Center);

    // 2. Filtered Log entries
    let filtered_lines: Vec<Element<'a, Message>> = entries
        .iter()
        .filter(|e| {
            if selected_level != LogLevel::All && e.level != selected_level {
                return false;
            }
            if !search_query.is_empty() && !e.message.to_lowercase().contains(&search_query.to_lowercase()) {
                return false;
            }
            true
        })
        .map(|e| {
            let color = match e.level {
                LogLevel::Error => ACCENT_ROSE,
                LogLevel::Warn => ACCENT_AMBER,
                _ => TEXT_SECONDARY,
            };

            row![
                text(&e.timestamp).font(FONT_MONO).size(11).color(TEXT_MUTED).width(75),
                text(&e.message).font(FONT_MONO).size(11).color(color),
            ]
            .spacing(8)
            .into()
        })
        .collect();

    let logs_container = container(
        scrollable(
            column(filtered_lines).spacing(4)
        )
        .height(Length::Fill)
    )
    .padding(12)
    .height(Length::Fill)
    .style(|_| container::Style {
        background: Some(BG_SURFACE.into()),
        border: iced::Border {
            color: BORDER_SUBTLE,
            width: 1.0,
            radius: iced::border::Radius::from(8.0),
        },
        ..Default::default()
    });

    column![toolbar, logs_container]
        .spacing(10)
        .height(Length::Fill)
        .into()
}
