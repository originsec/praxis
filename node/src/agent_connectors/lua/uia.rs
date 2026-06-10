use anyhow::Result;
#[cfg(windows)]
use anyhow::anyhow;
use mlua::{Lua, Table};
#[cfg(windows)]
use once_cell::sync::Lazy;
#[cfg(windows)]
use std::collections::HashMap;
#[cfg(windows)]
use std::sync::Mutex;

#[cfg(windows)]
use crate::utils::LockExt;
#[cfg(windows)]
use uiautomation::UIElement;
#[cfg(windows)]
use uiautomation::controls::ControlType;
#[cfg(windows)]
use uiautomation::core::UIAutomation;
#[cfg(windows)]
use uiautomation::patterns::{
    UIExpandCollapsePattern, UIInvokePattern, UITogglePattern, UIWindowPattern,
};
#[cfg(windows)]
use uiautomation::types::{TreeScope, UIProperty};
#[cfg(windows)]
use uiautomation::variants::Variant;

#[cfg(windows)]
struct UiaState {
    automation: UIAutomation,
    elements: HashMap<String, UIElement>,
}

#[cfg(windows)]
unsafe impl Send for UiaState {}

#[cfg(windows)]
static UIA_STATE: Lazy<Mutex<Option<UiaState>>> = Lazy::new(|| Mutex::new(None));

#[cfg(windows)]
fn ensure_init() -> Result<()> {
    let mut state = UIA_STATE.lock_safe();
    if state.is_none() {
        let automation = UIAutomation::new().map_err(|e| anyhow!("UIA init failed: {}", e))?;
        *state = Some(UiaState {
            automation,
            elements: HashMap::new(),
        });
    }
    Ok(())
}

#[cfg(windows)]
fn store_element(element: UIElement) -> String {
    let id = uuid::Uuid::new_v4().to_string();
    let mut state = UIA_STATE.lock_safe();
    if let Some(ref mut s) = *state {
        s.elements.insert(id.clone(), element);
    }
    id
}

#[cfg(windows)]
fn with_element<F, R>(id: &str, f: F) -> Result<R>
where
    F: FnOnce(&UIElement, &UIAutomation) -> Result<R>,
{
    let state = UIA_STATE.lock_safe();
    let s = state
        .as_ref()
        .ok_or_else(|| anyhow!("UIA not initialized"))?;
    let el = s
        .elements
        .get(id)
        .ok_or_else(|| anyhow!("UIA element not found: {}", id))?;
    f(el, &s.automation)
}

#[cfg(windows)]
fn parse_tree_scope(scope: Option<String>) -> TreeScope {
    match scope.as_deref() {
        Some("children") => TreeScope::Children,
        Some("subtree") => TreeScope::Subtree,
        _ => TreeScope::Descendants,
    }
}

#[cfg(windows)]
fn parse_control_type(name: &str) -> Option<ControlType> {
    match name.to_lowercase().as_str() {
        "window" => Some(ControlType::Window),
        "menubar" => Some(ControlType::MenuBar),
        "menuitem" => Some(ControlType::MenuItem),
        "menu" => Some(ControlType::Menu),
        "button" => Some(ControlType::Button),
        "edit" => Some(ControlType::Edit),
        "text" => Some(ControlType::Text),
        "pane" => Some(ControlType::Pane),
        "toolbar" => Some(ControlType::ToolBar),
        "tab" => Some(ControlType::Tab),
        "tabitem" => Some(ControlType::TabItem),
        "treeitem" => Some(ControlType::TreeItem),
        "tree" => Some(ControlType::Tree),
        "list" => Some(ControlType::List),
        "listitem" => Some(ControlType::ListItem),
        "document" => Some(ControlType::Document),
        "group" => Some(ControlType::Group),
        "combobox" => Some(ControlType::ComboBox),
        "checkbox" => Some(ControlType::CheckBox),
        "radiobutton" => Some(ControlType::RadioButton),
        "hyperlink" => Some(ControlType::Hyperlink),
        "custom" => Some(ControlType::Custom),
        _ => None,
    }
}

//
// Build a UICondition from a config table: { name, control_type, classname }.
// Multiple properties are combined with AND logic.
//

#[cfg(windows)]
fn build_condition(
    automation: &UIAutomation,
    config: &Table,
) -> Result<uiautomation::core::UICondition> {
    let name: Option<String> = config.get("name").unwrap_or(None);
    let control_type: Option<String> = config.get("control_type").unwrap_or(None);
    let classname: Option<String> = config.get("classname").unwrap_or(None);

    let mut conditions: Vec<uiautomation::core::UICondition> = Vec::new();

    if let Some(ref n) = name {
        let cond = automation
            .create_property_condition(UIProperty::Name, Variant::from(n.as_str()), None)
            .map_err(|e| anyhow!("create name condition: {}", e))?;
        conditions.push(cond);
    }

    if let Some(ref ct) = control_type {
        if let Some(ct_val) = parse_control_type(ct) {
            let cond = automation
                .create_property_condition(
                    UIProperty::ControlType,
                    Variant::from(ct_val as i32),
                    None,
                )
                .map_err(|e| anyhow!("create control_type condition: {}", e))?;
            conditions.push(cond);
        } else {
            return Err(anyhow!("unknown control_type: {}", ct));
        }
    }

    if let Some(ref cn) = classname {
        let cond = automation
            .create_property_condition(UIProperty::ClassName, Variant::from(cn.as_str()), None)
            .map_err(|e| anyhow!("create classname condition: {}", e))?;
        conditions.push(cond);
    }

    if conditions.is_empty() {
        return automation
            .create_true_condition()
            .map_err(|e| anyhow!("create true condition: {}", e));
    }

    let mut combined = conditions.remove(0);
    for cond in conditions {
        combined = automation
            .create_and_condition(combined, cond)
            .map_err(|e| anyhow!("create and condition: {}", e))?;
    }

    Ok(combined)
}

#[cfg(windows)]
fn lua_err(e: impl std::fmt::Display) -> mlua::Error {
    mlua::Error::RuntimeError(e.to_string())
}

//
// praxis.uia_find_window({ name = "...", pid = 123 })
// Finds a top-level window by name prefix and/or PID.
//

#[cfg(windows)]
fn uia_find_window(config: &Table) -> Result<Option<String>> {
    ensure_init()?;
    let name: Option<String> = config.get("name").unwrap_or(None);
    let pid: Option<u32> = config.get("pid").unwrap_or(None);

    let state = UIA_STATE.lock_safe();
    let s = state
        .as_ref()
        .ok_or_else(|| anyhow!("UIA not initialized"))?;

    let root = s
        .automation
        .get_root_element()
        .map_err(|e| anyhow!("get root: {}", e))?;

    let window_condition = s
        .automation
        .create_property_condition(
            UIProperty::ControlType,
            Variant::from(ControlType::Window as i32),
            None,
        )
        .map_err(|e| anyhow!("create window condition: {}", e))?;

    let windows = root
        .find_all(TreeScope::Children, &window_condition)
        .map_err(|e| anyhow!("find windows: {}", e))?;

    for w in windows {
        let w_name = w.get_name().unwrap_or_default();
        let w_pid = w.get_process_id().unwrap_or_default() as u32;

        let name_match = name
            .as_ref()
            .map(|n| w_name.starts_with(n.as_str()))
            .unwrap_or(true);
        let pid_match = pid.map(|p| w_pid == p).unwrap_or(true);

        if name_match && pid_match {
            drop(state);
            return Ok(Some(store_element(w)));
        }
    }

    Ok(None)
}

//
// praxis.uia_find(parent_id, { name, control_type, classname, scope })
// Finds the first matching child/descendant.
//

#[cfg(windows)]
fn uia_find(parent_id: &str, config: &Table) -> Result<Option<String>> {
    let scope: Option<String> = config.get("scope").unwrap_or(None);
    let tree_scope = parse_tree_scope(scope);

    with_element(parent_id, |el, automation| {
        let condition = build_condition(automation, config)?;
        match el.find_first(tree_scope, &condition) {
            Ok(found) => Ok(Some(store_element(found))),
            Err(_) => Ok(None),
        }
    })
}

//
// praxis.uia_find_bfs(parent_id, { name, control_type, classname }, max_depth)
// Breadth-first search using Children scope at each level. Avoids the hang
// that find_first(Descendants) causes on large Electron UIA trees.
//

#[cfg(windows)]
fn uia_find_bfs(parent_id: &str, config: &Table, max_depth: u32) -> Result<Option<String>> {
    let state = UIA_STATE.lock_safe();
    let s = state
        .as_ref()
        .ok_or_else(|| anyhow!("UIA not initialized"))?;
    let root = s
        .elements
        .get(parent_id)
        .ok_or_else(|| anyhow!("UIA element not found: {}", parent_id))?;

    let target_condition = build_condition(&s.automation, config)?;
    let true_condition = s
        .automation
        .create_true_condition()
        .map_err(|e| anyhow!("create true condition: {}", e))?;

    let mut queue: Vec<UIElement> = vec![root.clone()];

    for _depth in 0..max_depth {
        let mut next_queue = Vec::new();

        for parent in &queue {
            //
            // Check children of this parent for a match.
            //

            match parent.find_first(TreeScope::Children, &target_condition) {
                Ok(found) => {
                    drop(state);
                    return Ok(Some(store_element(found)));
                }
                Err(_) => {}
            }

            //
            // Collect all children for the next level.
            //

            if let Ok(children) = parent.find_all(TreeScope::Children, &true_condition) {
                for child in children {
                    next_queue.push(child);
                }
            }
        }

        if next_queue.is_empty() {
            break;
        }
        queue = next_queue;
    }

    Ok(None)
}

//
// praxis.uia_find_all(parent_id, { name, control_type, classname, scope })
// Finds all matching children/descendants. Returns list of element IDs.
//

#[cfg(windows)]
fn uia_find_all(parent_id: &str, config: &Table) -> Result<Vec<String>> {
    let scope: Option<String> = config.get("scope").unwrap_or(None);
    let tree_scope = parse_tree_scope(scope);

    with_element(parent_id, |el, automation| {
        let condition = build_condition(automation, config)?;
        let found = el
            .find_all(tree_scope, &condition)
            .map_err(|e| anyhow!("find_all: {}", e))?;

        let mut ids = Vec::new();
        for f in found {
            ids.push(store_element(f));
        }
        Ok(ids)
    })
}

//
// praxis.uia_name(element_id) → string
//

#[cfg(windows)]
fn uia_name(element_id: &str) -> Result<String> {
    with_element(element_id, |el, _| {
        el.get_name().map_err(|e| anyhow!("get_name: {}", e))
    })
}

//
// praxis.uia_classname(element_id) → string
//

#[cfg(windows)]
fn uia_classname(element_id: &str) -> Result<String> {
    with_element(element_id, |el, _| {
        el.get_classname()
            .map_err(|e| anyhow!("get_classname: {}", e))
    })
}

//
// praxis.uia_invoke(element_id)
//

#[cfg(windows)]
fn uia_invoke(element_id: &str) -> Result<()> {
    with_element(element_id, |el, _| {
        let pattern: UIInvokePattern = el
            .get_pattern()
            .map_err(|e| anyhow!("get InvokePattern: {}", e))?;
        pattern.invoke().map_err(|e| anyhow!("invoke: {}", e))
    })
}

//
// praxis.uia_expand(element_id)
//

#[cfg(windows)]
fn uia_expand(element_id: &str) -> Result<()> {
    with_element(element_id, |el, _| {
        let pattern: UIExpandCollapsePattern = el
            .get_pattern()
            .map_err(|e| anyhow!("get ExpandCollapsePattern: {}", e))?;
        pattern.expand().map_err(|e| anyhow!("expand: {}", e))
    })
}

//
// praxis.uia_focus(element_id)
//

#[cfg(windows)]
fn uia_focus(element_id: &str) -> Result<()> {
    with_element(element_id, |el, _| {
        el.set_focus().map_err(|e| anyhow!("set_focus: {}", e))
    })
}

//
// praxis.uia_send_keys(element_id, keys, interval_ms)
//

#[cfg(windows)]
fn uia_send_keys(element_id: &str, keys: &str, interval: u32) -> Result<()> {
    with_element(element_id, |el, _| {
        el.set_focus().map_err(|e| anyhow!("set_focus: {}", e))?;
        el.send_keys(keys, interval as u64)
            .map_err(|e| anyhow!("send_keys: {}", e))
    })
}

//
// praxis.uia_window_close(element_id) — close a window via WindowPattern.
//

#[cfg(windows)]
fn uia_window_close(element_id: &str) -> Result<()> {
    with_element(element_id, |el, _| {
        let pattern: UIWindowPattern = el
            .get_pattern()
            .map_err(|e| anyhow!("get WindowPattern: {}", e))?;
        pattern.close().map_err(|e| anyhow!("window close: {}", e))
    })
}

//
// praxis.uia_toggle(element_id)
//

#[cfg(windows)]
fn uia_toggle(element_id: &str) -> Result<()> {
    with_element(element_id, |el, _| {
        let pattern: UITogglePattern = el
            .get_pattern()
            .map_err(|e| anyhow!("get TogglePattern: {}", e))?;
        pattern.toggle().map_err(|e| anyhow!("toggle: {}", e))
    })
}

//
// praxis.uia_toggle_state(element_id) → "on" | "off" | "indeterminate"
//

#[cfg(windows)]
fn uia_toggle_state(element_id: &str) -> Result<String> {
    with_element(element_id, |el, _| {
        let pattern: UITogglePattern = el
            .get_pattern()
            .map_err(|e| anyhow!("get TogglePattern: {}", e))?;
        let state = pattern
            .get_toggle_state()
            .map_err(|e| anyhow!("get toggle state: {}", e))?;
        Ok(match state {
            uiautomation::types::ToggleState::Off => "off".to_string(),
            uiautomation::types::ToggleState::On => "on".to_string(),
            uiautomation::types::ToggleState::Indeterminate => "indeterminate".to_string(),
        })
    })
}

//
// praxis.uia_bounding_rect(element_id) → { x, y, width, height }
//

#[cfg(windows)]
fn uia_bounding_rect(element_id: &str) -> Result<(f64, f64, f64, f64)> {
    with_element(element_id, |el, _| {
        let rect = el
            .get_bounding_rectangle()
            .map_err(|e| anyhow!("get bounding rect: {}", e))?;
        Ok((
            rect.get_left() as f64,
            rect.get_top() as f64,
            rect.get_width() as f64,
            rect.get_height() as f64,
        ))
    })
}

//
// praxis.uia_set_foreground(element_id) — bring window to front.
//

#[cfg(windows)]
fn uia_set_foreground(element_id: &str) -> Result<()> {
    with_element(element_id, |el, _| {
        let handle = el
            .get_native_window_handle()
            .map_err(|e| anyhow!("get native window handle: {}", e))?;
        let raw: isize = handle.into();
        unsafe {
            let _ = windows::Win32::UI::WindowsAndMessaging::SetForegroundWindow(
                windows::Win32::Foundation::HWND(raw as *mut _),
            );
        }
        Ok(())
    })
}

//
// praxis.uia_click_at(x, y) — move cursor and click at screen coordinates.
//

#[cfg(windows)]
fn uia_click_at(x: i32, y: i32) -> Result<()> {
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        INPUT, INPUT_0, INPUT_MOUSE, MOUSE_EVENT_FLAGS, MOUSEEVENTF_ABSOLUTE, MOUSEEVENTF_LEFTDOWN,
        MOUSEEVENTF_LEFTUP, MOUSEEVENTF_MOVE, MOUSEINPUT, SendInput,
    };

    unsafe {
        windows::Win32::UI::WindowsAndMessaging::SetCursorPos(x, y)
            .map_err(|e| anyhow!("SetCursorPos: {}", e))?;
    }

    std::thread::sleep(std::time::Duration::from_millis(50));

    let screen_w = unsafe {
        windows::Win32::UI::WindowsAndMessaging::GetSystemMetrics(
            windows::Win32::UI::WindowsAndMessaging::SM_CXSCREEN,
        )
    };
    let screen_h = unsafe {
        windows::Win32::UI::WindowsAndMessaging::GetSystemMetrics(
            windows::Win32::UI::WindowsAndMessaging::SM_CYSCREEN,
        )
    };

    let abs_x = (x as u32 * 65535 / screen_w as u32) as i32;
    let abs_y = (y as u32 * 65535 / screen_h as u32) as i32;

    let inputs = [
        INPUT {
            r#type: INPUT_MOUSE,
            Anonymous: INPUT_0 {
                mi: MOUSEINPUT {
                    dx: abs_x,
                    dy: abs_y,
                    mouseData: 0,
                    dwFlags: MOUSE_EVENT_FLAGS(
                        MOUSEEVENTF_ABSOLUTE.0 | MOUSEEVENTF_MOVE.0 | MOUSEEVENTF_LEFTDOWN.0,
                    ),
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        },
        INPUT {
            r#type: INPUT_MOUSE,
            Anonymous: INPUT_0 {
                mi: MOUSEINPUT {
                    dx: abs_x,
                    dy: abs_y,
                    mouseData: 0,
                    dwFlags: MOUSE_EVENT_FLAGS(
                        MOUSEEVENTF_ABSOLUTE.0 | MOUSEEVENTF_MOVE.0 | MOUSEEVENTF_LEFTUP.0,
                    ),
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        },
    ];

    unsafe {
        SendInput(&inputs, std::mem::size_of::<INPUT>() as i32);
    }

    Ok(())
}

//
// praxis.uia_release(element_id)
//

#[cfg(windows)]
fn uia_release(element_id: &str) {
    let mut state = UIA_STATE.lock_safe();
    if let Some(ref mut s) = *state {
        s.elements.remove(element_id);
    }
}

//
// praxis.uia_root() → element_id
//

#[cfg(windows)]
fn uia_root() -> Result<String> {
    ensure_init()?;
    let state = UIA_STATE.lock_safe();
    let s = state
        .as_ref()
        .ok_or_else(|| anyhow!("UIA not initialized"))?;
    let root = s
        .automation
        .get_root_element()
        .map_err(|e| anyhow!("get root: {}", e))?;
    drop(state);
    Ok(store_element(root))
}

pub fn install_uia_api(lua: &Lua, praxis: &Table) -> Result<()> {
    #[cfg(not(windows))]
    {
        let _ = (lua, praxis);
        return Ok(());
    }

    #[cfg(windows)]
    {
        praxis
            .set(
                "uia_root",
                lua.create_function(|_, ()| uia_root().map_err(lua_err))
                    .map_err(|e| anyhow!(e.to_string()))?,
            )
            .map_err(|e| anyhow!(e.to_string()))?;

        praxis
            .set(
                "uia_find_window",
                lua.create_function(|_, config: Table| uia_find_window(&config).map_err(lua_err))
                    .map_err(|e| anyhow!(e.to_string()))?,
            )
            .map_err(|e| anyhow!(e.to_string()))?;

        praxis
            .set(
                "uia_find",
                lua.create_function(|_, (parent_id, config): (String, Table)| {
                    uia_find(&parent_id, &config).map_err(lua_err)
                })
                .map_err(|e| anyhow!(e.to_string()))?,
            )
            .map_err(|e| anyhow!(e.to_string()))?;

        praxis
            .set(
                "uia_find_bfs",
                lua.create_function(
                    |_, (parent_id, config, max_depth): (String, Table, Option<u32>)| {
                        uia_find_bfs(&parent_id, &config, max_depth.unwrap_or(10)).map_err(lua_err)
                    },
                )
                .map_err(|e| anyhow!(e.to_string()))?,
            )
            .map_err(|e| anyhow!(e.to_string()))?;

        praxis
            .set(
                "uia_find_all",
                lua.create_function(|_, (parent_id, config): (String, Table)| {
                    uia_find_all(&parent_id, &config).map_err(lua_err)
                })
                .map_err(|e| anyhow!(e.to_string()))?,
            )
            .map_err(|e| anyhow!(e.to_string()))?;

        praxis
            .set(
                "uia_name",
                lua.create_function(|_, id: String| uia_name(&id).map_err(lua_err))
                    .map_err(|e| anyhow!(e.to_string()))?,
            )
            .map_err(|e| anyhow!(e.to_string()))?;

        praxis
            .set(
                "uia_classname",
                lua.create_function(|_, id: String| uia_classname(&id).map_err(lua_err))
                    .map_err(|e| anyhow!(e.to_string()))?,
            )
            .map_err(|e| anyhow!(e.to_string()))?;

        praxis
            .set(
                "uia_invoke",
                lua.create_function(|_, id: String| uia_invoke(&id).map_err(lua_err))
                    .map_err(|e| anyhow!(e.to_string()))?,
            )
            .map_err(|e| anyhow!(e.to_string()))?;

        praxis
            .set(
                "uia_expand",
                lua.create_function(|_, id: String| uia_expand(&id).map_err(lua_err))
                    .map_err(|e| anyhow!(e.to_string()))?,
            )
            .map_err(|e| anyhow!(e.to_string()))?;

        praxis
            .set(
                "uia_focus",
                lua.create_function(|_, id: String| uia_focus(&id).map_err(lua_err))
                    .map_err(|e| anyhow!(e.to_string()))?,
            )
            .map_err(|e| anyhow!(e.to_string()))?;

        praxis
            .set(
                "uia_send_keys",
                lua.create_function(|_, (id, keys, interval): (String, String, Option<u32>)| {
                    uia_send_keys(&id, &keys, interval.unwrap_or(10)).map_err(lua_err)
                })
                .map_err(|e| anyhow!(e.to_string()))?,
            )
            .map_err(|e| anyhow!(e.to_string()))?;

        praxis
            .set(
                "uia_window_close",
                lua.create_function(|_, id: String| uia_window_close(&id).map_err(lua_err))
                    .map_err(|e| anyhow!(e.to_string()))?,
            )
            .map_err(|e| anyhow!(e.to_string()))?;

        praxis
            .set(
                "uia_toggle",
                lua.create_function(|_, id: String| uia_toggle(&id).map_err(lua_err))
                    .map_err(|e| anyhow!(e.to_string()))?,
            )
            .map_err(|e| anyhow!(e.to_string()))?;

        praxis
            .set(
                "uia_toggle_state",
                lua.create_function(|_, id: String| uia_toggle_state(&id).map_err(lua_err))
                    .map_err(|e| anyhow!(e.to_string()))?,
            )
            .map_err(|e| anyhow!(e.to_string()))?;

        praxis
            .set(
                "uia_bounding_rect",
                lua.create_function(|lua, id: String| {
                    let (x, y, w, h) = uia_bounding_rect(&id).map_err(lua_err)?;
                    let tbl = lua.create_table().map_err(lua_err)?;
                    tbl.set("x", x).map_err(lua_err)?;
                    tbl.set("y", y).map_err(lua_err)?;
                    tbl.set("width", w).map_err(lua_err)?;
                    tbl.set("height", h).map_err(lua_err)?;
                    Ok(tbl)
                })
                .map_err(|e| anyhow!(e.to_string()))?,
            )
            .map_err(|e| anyhow!(e.to_string()))?;

        praxis
            .set(
                "uia_set_foreground",
                lua.create_function(|_, id: String| uia_set_foreground(&id).map_err(lua_err))
                    .map_err(|e| anyhow!(e.to_string()))?,
            )
            .map_err(|e| anyhow!(e.to_string()))?;

        praxis
            .set(
                "uia_click_at",
                lua.create_function(|_, (x, y): (i32, i32)| uia_click_at(x, y).map_err(lua_err))
                    .map_err(|e| anyhow!(e.to_string()))?,
            )
            .map_err(|e| anyhow!(e.to_string()))?;

        praxis
            .set(
                "uia_release",
                lua.create_function(|_, id: String| {
                    uia_release(&id);
                    Ok(())
                })
                .map_err(|e| anyhow!(e.to_string()))?,
            )
            .map_err(|e| anyhow!(e.to_string()))?;

        Ok(())
    }
}
