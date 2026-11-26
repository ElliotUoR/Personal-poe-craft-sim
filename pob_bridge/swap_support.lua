-- Replace one support gem in the main skill's group with candidates and report DPS under
-- optional extra-modifier scenarios (e.g. Headhunter-stolen rare monster mods).
-- Usage (cwd = PathOfBuilding/src):
--   luajit swap_support.lua <build.xml> <support to replace> <scenarios.lua> <candidate> [<candidate>...]
-- scenarios.lua returns { {name="...", mods="line\nline"}, ... }
local XMLPATH, OLD, SCEN = assert(arg[1]), assert(arg[2]), assert(arg[3])
local CANDS = {}
for i = 4, #arg do CANDS[#CANDS + 1] = arg[i] end
local scenarios = dofile(SCEN)
local here = debug.getinfo(1, "S").source:sub(2):match("(.*/)") or "./"
dofile(here .. "pob_env.lua")
local f = assert(io.open(XMLPATH, "r")); local x = f:read("*a"); f:close()
loadBuildFromXML(x, "imported")
local st, ct, cs = build.skillsTab, build.configTab, build.calcsTab
local group = st.socketGroupList[build.mainSocketGroup]
local gem
for _, g in ipairs(group.gemList) do if g.nameSpec == OLD then gem = g end end
assert(gem, "support " .. OLD .. " not found in the main socket group")
local cl = ct.configSets[ct.activeConfigSetId].customModsList

local function setScenario(mods)
  for i = #cl, 1, -1 do if cl[i].title == "swap_support scenario" then table.remove(cl, i) end end
  if mods and mods ~= "" then cl[#cl + 1] = { title = "swap_support scenario", enabled = true, text = mods } end
  ct:BuildModList()
end
local function dps()
  cs:BuildOutput()
  return cs.mainOutput.CombinedDPS or 0
end
local origName, origLevel, origQuality = gem.nameSpec, gem.level, gem.quality
local rows = {}
local list = { OLD }
for _, c in ipairs(CANDS) do list[#list + 1] = c end
local origGemId, origSkillId = gem.gemId, gem.skillId
for _, cand in ipairs(list) do
  gem.nameSpec = cand
  -- PoB resolves gemId/skillId before nameSpec: clear them so the new name is used
  if cand == origName then gem.gemId, gem.skillId = origGemId, origSkillId else gem.gemId, gem.skillId = nil, nil end
  gem.level, gem.quality = (cand == origName) and origLevel or 20, (cand == origName) and origQuality or 20
  st:ProcessSocketGroup(group)
  local ok = gem.gemData ~= nil and (gem.gemData.name == cand or gem.gemData.nameSpec == cand or cand == origName)
  local row = { cand .. (ok and "" or " (NOT FOUND: " .. tostring(gem.errMsg or (gem.gemData and gem.gemData.name)) .. ")") }
  for _, sc in ipairs(scenarios) do setScenario(sc.mods); row[#row + 1] = dps() end
  rows[#rows + 1] = row
end
setScenario(nil)
io.write(string.format("%-34s", "support \\ scenario"))
for _, sc in ipairs(scenarios) do io.write(string.format("%22s", sc.name:sub(1, 21))) end
io.write("\n")
local base = rows[1]
for _, row in ipairs(rows) do
  io.write(string.format("%-34s", row[1]:sub(1, 33)))
  for i = 2, #row do io.write(string.format("%15.1fM %+5.0f%%", row[i] / 1e6, (row[i] / base[i] - 1) * 100)) end
  io.write("\n")
end
