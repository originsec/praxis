local M = {}

function M.starts_with(s, prefix)
  s = tostring(s or "")
  prefix = tostring(prefix or "")
  return string.sub(s, 1, #prefix) == prefix
end

function M.ends_with(s, suffix)
  s = tostring(s or "")
  suffix = tostring(suffix or "")
  if #suffix == 0 then
    return true
  end
  return string.sub(s, -#suffix) == suffix
end

function M.norm(path)
  return string.gsub(path or "", "\\", "/")
end

function M.parent_dir(path)
  return praxis.path_parent(path)
end

function M.expand_path(path, home)
  path = tostring(path or "")
  if home ~= nil and tostring(home) ~= "" then
    local h = tostring(home)
    local out = string.gsub(path, "${HOME}", h)
    out = string.gsub(out, "${USERPROFILE}", h)
    return out
  end
  return praxis.expand_path(path)
end

function M.dedup(list)
  local seen = {}
  local out = {}
  for _, item in ipairs(list or {}) do
    if item ~= nil and not seen[item] then
      seen[item] = true
      table.insert(out, item)
    end
  end
  return out
end

function M.sort_strings(list)
  table.sort(list, function(a, b)
    return tostring(a) < tostring(b)
  end)
end

function M.user_homes_with_dir(dir_name)
  dir_name = tostring(dir_name or "")
  if dir_name == "" then
    return {}
  end
  return M.for_each_user_home_coalesce(function(home)
    if praxis.path_is_dir(praxis.path_join({ home, dir_name })) then
      return home
    end
    return nil
  end)
end

function M.has_any_env_var(env_vars, homes)
  local vars = env_vars or {}
  local users = homes or {}

  if #users == 0 then
    for _, key in ipairs(vars) do
      if praxis.env_get_for_home(key, nil) ~= nil then
        return true
      end
    end
    return false
  end

  for _, key in ipairs(vars) do
    for _, home in ipairs(users) do
      if praxis.env_get_for_home(key, home) ~= nil then
        return true
      end
    end
  end
  return false
end

function M.parse_json(content)
  if content == nil then
    return nil
  end
  local ok, parsed = pcall(praxis.json_decode, content)
  if not ok or type(parsed) ~= "table" then
    return nil
  end
  return parsed
end

function M.for_each_user_home_coalesce(fn, opts)
  opts = opts or {}
  local dedup = opts.dedup
  if dedup == nil then
    dedup = true
  end
  local key_fn = opts.key_fn

  local out = {}
  local seen = {}

  local function add(item)
    if item == nil then
      return
    end

    if not dedup then
      table.insert(out, item)
      return
    end

    local key = nil
    if key_fn then
      key = key_fn(item)
    elseif type(item) ~= "table" then
      key = tostring(item)
    end

    if key == nil then
      table.insert(out, item)
      return
    end

    if not seen[key] then
      seen[key] = true
      table.insert(out, item)
    end
  end

  local homes = praxis.user_homes() or {}
  for _, home in ipairs(homes) do
    local ok, result = pcall(fn, home)
    if ok and result ~= nil then
      if type(result) == "table" then
        local is_list = (#result > 0)
        if is_list then
          for _, item in ipairs(result) do
            add(item)
          end
        else
          add(result)
        end
      else
        add(result)
      end
    end
  end

  return out
end

return M
