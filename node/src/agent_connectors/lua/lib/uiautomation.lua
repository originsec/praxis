local M = {}

--
-- Windows UI Automation library. Wraps the native praxis.uia_* functions
-- into a higher-level API for finding windows, navigating element trees,
-- and invoking UI controls.
--
-- All element IDs are opaque strings managed by the Rust side. Call
-- release() when done with an element to free memory.
--

--
-- Get the desktop root element.
--

function M.root()
  return praxis.uia_root()
end

--
-- Find a top-level window by name prefix and/or PID.
-- Returns element ID or nil.
--
-- Usage:
--   local win = uia.find_window({ name = "Claude" })
--   local win = uia.find_window({ pid = 1234 })
--   local win = uia.find_window({ name = "Claude", pid = 1234 })
--

function M.find_window(opts)
  return praxis.uia_find_window(opts)
end

--
-- Wait for a window to appear. Retries up to `retries` times with
-- `delay_ms` between attempts. Returns element ID or nil.
--

function M.wait_for_window(opts, retries, delay_ms)
  retries = retries or 10
  delay_ms = delay_ms or 1000
  for _ = 1, retries do
    local win = praxis.uia_find_window(opts)
    if win then
      return win
    end
    praxis.sleep_ms(delay_ms)
  end
  return nil
end

--
-- Find the first child/descendant matching the given criteria.
-- Returns element ID or nil.
--
-- opts fields:
--   name          (string?)  Exact element name
--   control_type  (string?)  E.g. "MenuBar", "MenuItem", "Button"
--   classname     (string?)  CSS/Win32 class name
--   scope         (string?)  "children", "descendants" (default), "subtree"
--

function M.find(parent_id, opts)
  return praxis.uia_find(parent_id, opts)
end

--
-- Find all children/descendants matching the given criteria.
-- Returns a list of element IDs.
--

function M.find_all(parent_id, opts)
  return praxis.uia_find_all(parent_id, opts)
end

--
-- Breadth-first search using Children scope at each level. Avoids the hang
-- that find_first(Descendants) causes on large Electron UIA trees.
-- max_depth defaults to 10.
--

function M.find_bfs(parent_id, opts, max_depth)
  return praxis.uia_find_bfs(parent_id, opts, max_depth)
end

--
-- Wait for an element to appear under a parent. Returns element ID or nil.
-- Uses BFS by default to avoid Descendants traversal hangs on Electron apps.
--

function M.wait_for(parent_id, opts, retries, delay_ms)
  retries = retries or 10
  delay_ms = delay_ms or 500
  local desc = opts.name or opts.control_type or "element"
  for i = 1, retries do
    praxis.log_debug("uia.wait_for: attempt " .. i .. "/" .. retries .. " for '" .. desc .. "'")
    local el = praxis.uia_find_bfs(parent_id, opts)
    if el then
      praxis.log_debug("uia.wait_for: found '" .. desc .. "' on attempt " .. i)
      return el
    end
    praxis.sleep_ms(delay_ms)
  end
  praxis.log_warn("uia.wait_for: '" .. desc .. "' not found after " .. retries .. " attempts")
  return nil
end

--
-- Get the name of an element.
--

function M.name(element_id)
  return praxis.uia_name(element_id)
end

--
-- Get the class name of an element.
--

function M.classname(element_id)
  return praxis.uia_classname(element_id)
end

--
-- Invoke an element (click/activate). Uses the UIA InvokePattern.
--

function M.invoke(element_id)
  praxis.uia_invoke(element_id)
end

--
-- Expand an element (e.g. open a menu). Uses the UIA ExpandCollapsePattern.
--

function M.expand(element_id)
  praxis.uia_expand(element_id)
end

--
-- Set keyboard focus to an element.
--

function M.focus(element_id)
  praxis.uia_focus(element_id)
end

--
-- Send keystrokes to an element (sets focus first).
-- Uses the uiautomation crate's key syntax: {alt}, {enter}, {ctrl}, etc.
--

function M.send_keys(element_id, keys, interval_ms)
  praxis.uia_send_keys(element_id, keys, interval_ms)
end

--
-- Toggle a checkbox/toggle element. Uses the UIA TogglePattern.
--

function M.toggle(element_id)
  praxis.uia_toggle(element_id)
end

--
-- Get the toggle state of an element: "on", "off", or "indeterminate".
--

function M.toggle_state(element_id)
  return praxis.uia_toggle_state(element_id)
end

--
-- Get the bounding rectangle of an element.
-- Returns { x, y, width, height }.
--

function M.bounding_rect(element_id)
  return praxis.uia_bounding_rect(element_id)
end

--
-- Close a window via WindowPattern.
--

function M.window_close(element_id)
  praxis.uia_window_close(element_id)
end

--
-- Bring a window to the foreground.
--

function M.set_foreground(element_id)
  praxis.uia_set_foreground(element_id)
end

--
-- Click at screen coordinates.
--

function M.click_at(x, y)
  praxis.uia_click_at(x, y)
end

--
-- Click the center of an element using screen coordinates.
--

function M.click_element(element_id)
  local rect = M.bounding_rect(element_id)
  local cx = math.floor(rect.x + rect.width / 2)
  local cy = math.floor(rect.y + rect.height / 2)
  M.click_at(cx, cy)
end

--
-- Release an element handle to free memory.
--

function M.release(element_id)
  praxis.uia_release(element_id)
end

--
-- Release a list of element handles.
--

function M.release_all(ids)
  for _, id in ipairs(ids or {}) do
    praxis.uia_release(id)
  end
end

--
-- Dismiss popup dialogs matching a window name. Finds all top-level windows
-- with the given name and closes them via WindowPattern.Close().
--

function M.dismiss_dialogs(dialog_name, retries, delay_ms)
  retries = retries or 2
  delay_ms = delay_ms or 1000

  for attempt = 1, retries do
    local root = M.root()
    local dialogs = M.find_all(root, {
      name = dialog_name,
      control_type = "Window",
      scope = "children",
    })
    M.release(root)

    if #dialogs == 0 and attempt < retries then
      praxis.sleep_ms(delay_ms)
    end

    for _, dialog in ipairs(dialogs) do
      pcall(M.window_close, dialog)
      M.release(dialog)
    end

    if #dialogs > 0 then
      return #dialogs
    end
  end

  return 0
end

--
-- Open a menu by invoking a named button, then navigate through submenu
-- items. For apps with web-based menus (not native Win32 menu bars).
--
-- Steps:
--   1) Find and invoke the trigger button (e.g. "Menu")
--   2) For each intermediate path entry, find and expand it
--   3) Return the final menu item element
--

function M.open_app_menu(window_id, trigger_name, path)
  praxis.log_debug("uia.open_app_menu: looking for trigger '" .. trigger_name .. "'")
  local trigger = M.wait_for(window_id, { name = trigger_name }, 10, 500)
  if not trigger then
    error("menu trigger not found: " .. trigger_name)
  end

  praxis.log_debug("uia.open_app_menu: invoking trigger '" .. trigger_name .. "'")
  M.invoke(trigger)
  M.release(trigger)
  praxis.sleep_ms(1000)

  local current = nil
  for i, item_name in ipairs(path) do
    praxis.log_debug("uia.open_app_menu: looking for '" .. item_name .. "' (" .. i .. "/" .. #path .. ")")
    local item = M.wait_for(window_id, { name = item_name }, 10, 500)
    if not item then
      error("menu item not found: " .. item_name)
    end

    if i < #path then
      praxis.log_debug("uia.open_app_menu: expanding '" .. item_name .. "'")
      local ok = pcall(M.expand, item)
      if not ok then
        praxis.log_debug("uia.open_app_menu: expand failed, trying invoke")
        pcall(M.invoke, item)
      end
      praxis.sleep_ms(800)
    end

    current = item
  end

  return current
end

return M
