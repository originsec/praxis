use ratatui::style::Color;

//
// Layered neutral ramp. BG is the page background, BG_PANEL is the
// slightly-lighter container tint that distinguishes panels without
// drawing borders, BG_ELEMENT is the input/element fill (one step up
// again), and BG_MENU is the popup/menu background.
//

pub const BG: Color = Color::Rgb(18, 19, 22);
pub const BG_PANEL: Color = Color::Rgb(24, 26, 30);
pub const BG_ELEMENT: Color = Color::Rgb(32, 35, 40);
pub const BG_MENU: Color = Color::Rgb(28, 31, 36);
pub const BG_SELECTED: Color = Color::Rgb(45, 60, 45);

//
// Borders / dividers. The "subtle" tone is for resting separators,
// "border" is the default panel-bar, and "active" / ACCENT are used to
// mark focus.
//

pub const BORDER_SUBTLE: Color = Color::Rgb(38, 42, 48);
pub const BORDER: Color = Color::Rgb(60, 66, 74);

//
// Type. TEXT_BRIGHT is high-contrast emphasis (titles, key glyphs).
// TEXT is the default body fill. MUTED handles labels and inline meta;
// DIM is the lowest-signal tier (separators, ghosts, placeholder).
//

pub const TEXT_BRIGHT: Color = Color::Rgb(225, 228, 232);
pub const TEXT: Color = Color::Rgb(180, 185, 195);
pub const MUTED: Color = Color::Rgb(125, 130, 138);
pub const DIM: Color = Color::Rgb(75, 80, 88);

//
// Semantic accents. ACCENT is the brand green; it is reserved for
// focus, active state, brand chrome, and the prompt bar. SECONDARY
// (warm amber) and TERTIARY (cyan) are used for highlight/identity.
//

pub const ACCENT: Color = Color::Rgb(120, 200, 120);
pub const SECONDARY: Color = Color::Rgb(220, 170, 90);
pub const TERTIARY: Color = Color::Rgb(120, 180, 210);

//
// Status. STATUS_RUNNING/QUEUED/DONE/FAIL are kept as the canonical
// state palette. ERROR/WARN/INFO/OK alias them so semantic call sites
// read naturally.
//

pub const STATUS_RUNNING: Color = Color::Rgb(220, 175, 80);
pub const STATUS_DONE: Color = Color::Rgb(120, 200, 120);
pub const STATUS_FAIL: Color = Color::Rgb(220, 95, 95);
pub const STATUS_QUEUED: Color = Color::Rgb(120, 160, 210);

pub const OK: Color = STATUS_DONE;
pub const WARN: Color = STATUS_RUNNING;
pub const ERROR: Color = STATUS_FAIL;

//
// Legacy aliases retained for any straggling call sites in the
// markdown renderer or external consumers. They mirror the new
// tokens.
//

pub const INPUT_BORDER: Color = BORDER;

//
// Intercept view: HTTP status-code buckets and protocol markers.
//

pub const STATUS_2XX: Color = Color::Rgb(140, 205, 110);
pub const STATUS_3XX: Color = Color::Rgb(220, 190, 90);
pub const STATUS_4XX: Color = Color::Rgb(225, 145, 85);
pub const STATUS_5XX: Color = Color::Rgb(225, 95, 95);

pub const PROTO_WS: Color = Color::Rgb(120, 195, 220);
pub const PROTO_H2: Color = Color::Rgb(190, 130, 215);

//
// KQL keyword colour — warm amber so it stands apart from ACCENT green,
// which is reserved for table names and the active focus.
//

pub const KEYWORD: Color = SECONDARY;

pub const JSON_KEY: Color = Color::Rgb(150, 205, 150);
pub const JSON_STRING: Color = Color::Rgb(210, 185, 130);
pub const JSON_NUMBER: Color = Color::Rgb(120, 170, 215);
pub const JSON_PUNCT: Color = Color::Rgb(135, 140, 145);
