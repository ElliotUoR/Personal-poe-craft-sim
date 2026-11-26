-- Score sets of modifier lines (e.g. Headhunter-stolen rare monster mods) on a build:
-- each set is added as a PoB custom-modifier block, DPS/EHP recorded, unparsed lines flagged.
-- Usage (cwd = PathOfBuilding/src): luajit score_mod_sets.lua <build.xml> <sets.json> > out.tsv
-- sets.json: [{"name": "...", "lines": ["...", ...]}, ...]
local XMLPATH, SETS = assert(arg[1]), assert(arg[2])
local here = debug.getinfo(1, "S").source:sub(2):match("(.*/)") or "./"
dofile(here .. "pob_env.lua")
local dkjson = require("dkjson")
local f = assert(io.open(XMLPATH, "r")); local x = f:read("*a"); f:close()
f = assert(io.open(SETS, "r")); local sets = dkjson.decode(f:read("*a")); f:close()
if sets.mods then sets = sets.mods end
loadBuildFromXML(x, "imported")
local ct, cs = build.configTab, build.calcsTab
local cl = ct.configSets[ct.activeConfigSetId].customModsList
local function run(text)
  for i = #cl, 1, -1 do if cl[i].title == "score_mod_sets" then table.remove(cl, i) end end
  if text ~= "" then cl[#cl + 1] = { title = "score_mod_sets", enabled = true, text = text } end
  ct:BuildModList(); cs:BuildOutput()
  local o = cs.mainOutput
  return o.CombinedDPS or 0, o.TotalEHP or 0, o.Life or 0, o.EffectiveMovementSpeedMod or 0
end
local d0, e0 = run("")
print(table.concat({ "name", "dps_change_pct", "ehp_change_pct", "unparsed_lines", "lines" }, "\t"))
for _, s in ipairs(sets) do
  local unparsed = {}
  for _, line in ipairs(s.lines) do
    local mods, extra = modLib.parseMod(line)
    if not mods or extra then unparsed[#unparsed + 1] = line end
  end
  local d, e = run(table.concat(s.lines, "\n"))
  print(table.concat({ s.name, string.format("%.1f", (d / d0 - 1) * 100), string.format("%.1f", (e / e0 - 1) * 100),
    table.concat(unparsed, " | "), table.concat(s.lines, " | ") }, "\t"))
end
