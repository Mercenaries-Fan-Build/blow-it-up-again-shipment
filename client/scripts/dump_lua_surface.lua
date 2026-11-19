-- Run through the Lua bridge on the running game, after lines that set SKIP_LINES, PAGE_FROM and
-- PAGE_COUNT (the bridge returns at most 16 KiB, so the rows come a page at a time):
--   { printf 'SKIP_LINES = "Net\\tDLCGetMountPoint"\nPAGE_FROM = 1\nPAGE_COUNT = 300\n';
--     cat scripts/dump_lua_surface.lua; } | mercs2_repl
-- Returns the total row count on the first line, then rows PAGE_FROM .. PAGE_FROM+PAGE_COUNT-1, one per
-- `table<TAB>field`: every field of every global table, and of each table-valued field one level below
-- (as `Table.Sub<TAB>field`), leaving out the `table<TAB>field` lines SKIP_LINES names (the names this ASI
-- adds, src/registry.rs).
local skip = {}
for line in string.gmatch(SKIP_LINES, "[^\n]+") do
  skip[line] = true
end
local out = {}
local function add(t, f)
  local row = t .. "\t" .. f
  if not skip[row] then
    table.insert(out, row)
  end
end
for name, value in pairs(_G) do
  if type(name) == "string" and type(value) == "table" and name ~= "_G" and name ~= "_MODULES" then
    for field, inner in pairs(value) do
      if type(field) == "string" then
        add(name, field)
        if type(inner) == "table" and inner ~= value then
          for f2 in pairs(inner) do
            if type(f2) == "string" then
              add(name .. "." .. field, f2)
            end
          end
        end
      end
    end
  end
end
table.sort(out)
local page = {table.getn(out)}
local last = PAGE_FROM + PAGE_COUNT - 1
if last > table.getn(out) then
  last = table.getn(out)
end
for i = PAGE_FROM, last do
  table.insert(page, out[i])
end
return table.concat(page, "\n")
