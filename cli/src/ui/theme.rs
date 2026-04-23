use ratatui::style::Color;

pub const BG: Color = Color::Rgb(25, 25, 30);
pub const ACCENT: Color = Color::Rgb(100, 180, 100);
pub const DIM: Color = Color::Rgb(80, 80, 80);
pub const MUTED: Color = Color::Rgb(120, 120, 120);
pub const TEXT: Color = Color::Rgb(180, 180, 180);

pub const PANEL_HIGHLIGHT_BG: Color = Color::Rgb(35, 35, 40);
pub const POPUP_HIGHLIGHT_BG: Color = Color::Rgb(35, 40, 35);
pub const SETTINGS_HIGHLIGHT_BG: Color = Color::Rgb(40, 50, 40);
pub const POPUP_BG: Color = Color::Rgb(30, 30, 35);

pub const INPUT_BORDER: Color = Color::Rgb(60, 70, 60);
pub const STATUS_RUNNING: Color = Color::Rgb(180, 160, 60);
pub const STATUS_DONE: Color = Color::Rgb(80, 160, 80);
pub const STATUS_FAIL: Color = Color::Rgb(160, 60, 60);
pub const STATUS_QUEUED: Color = Color::Rgb(100, 140, 180);

//
// Intercept view: HTTP status-code buckets and protocol markers.
//

pub const STATUS_2XX: Color = Color::Rgb(130, 200, 100);
pub const STATUS_3XX: Color = Color::Rgb(200, 180, 80);
pub const STATUS_4XX: Color = Color::Rgb(220, 130, 70);
pub const STATUS_5XX: Color = Color::Rgb(220, 80, 80);

pub const PROTO_WS: Color = Color::Rgb(100, 180, 210);
pub const PROTO_H2: Color = Color::Rgb(180, 120, 210);

//
// KQL keyword colour — warm amber so it stands apart from ACCENT green,
// which is reserved for table names and the active focus.
//
pub const KEYWORD: Color = Color::Rgb(220, 170, 90);

pub const CODE_FG: Color = Color::Rgb(120, 190, 120);
#[allow(dead_code)]
pub const CODE_BG: Color = Color::Rgb(35, 35, 40);
pub const JSON_KEY: Color = Color::Rgb(140, 200, 140);
pub const JSON_STRING: Color = Color::Rgb(200, 180, 120);
pub const JSON_NUMBER: Color = Color::Rgb(100, 160, 210);
pub const JSON_PUNCT: Color = Color::Rgb(130, 130, 130);
