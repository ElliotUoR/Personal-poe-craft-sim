-- Equip an item (PoB item text) in a slot and print movement speed under buff scenarios.
-- Usage (cwd = PathOfBuilding/src): luajit try_boots.lua <build.xml> <item.txt> [slot]
local xmlPath, itemPath, slotName = assert(arg[1]), assert(arg[2]), arg[3] or "Boots"
local here = debug.getinfo(1, "S").source:sub(2):match("(.*/)") or "./"
dofile(here .. "pob_env.lua")
local f = assert(io.open(xmlPath, "r")); local xmlText = f:read("*a"); f:close()
f = assert(io.open(itemPath, "r")); local itemText = f:read("*a"); f:close()
loadBuildFromXML(xmlText, "imported")

local it, ct, cs = build.itemsTab, build.configTab, build.calcsTab
local item = new("Item"):Item(itemText)
print("PARSED", item.name, "| mods:")
for _, ml in ipairs(item.explicitModLines) do print("   ", ml.line, ml.extra and ("  <-- UNPARSED: " .. ml.extra) or "") end
it:AddItem(item, true)
it.slots[slotName]:SetSelItemId(item.id)
it:PopulateSlots()

local vars = { "buffOnslaught", "buffTailwind", "conditionCritRecently", "conditionKilledRecently" }
local saved = {}
for _, v in ipairs(vars) do saved[v] = ct.input[v] end
print("SAVED_CONFIG", "onslaught=" .. tostring(saved.buffOnslaught), "tailwind=" .. tostring(saved.buffTailwind),
  "critRecently=" .. tostring(saved.conditionCritRecently), "killedRecently=" .. tostring(saved.conditionKilledRecently))

local function run(label, set)
  for _, v in ipairs(vars) do ct.input[v] = saved[v] end
  for k, v in pairs(set) do ct.input[k] = v end
  ct:BuildModList()
  cs:BuildOutput()
  local o = cs.mainOutput
  local db = cs.mainEnv.player.modDB
  print(string.format("SCENARIO\t%-38s MovementSpeedMod %.4f  ActionSpeedMod %s  Onslaught=%s Tailwind=%s",
    label, o.MovementSpeedMod, tostring(o.ActionSpeedMod), tostring(db.conditions.Onslaught), tostring(db.conditions.Tailwind)))
  if os.getenv("DETAIL") then
    for _, t in ipairs({ "INC", "MORE", "OVERRIDE" }) do
      for _, m in ipairs(db:Tabulate(t, cs.mainEnv.player.mainSkill.skillCfg, "MovementSpeed") or {}) do
        print(string.format("      %s %+g  %s", t, m.value, tostring(m.mod.source)))
      end
    end
  end
end
run("as saved in the build", {})
run("no Onslaught, no Tailwind", { buffOnslaught = false, buffTailwind = false, conditionCritRecently = false, conditionKilledRecently = false })
run("crit recently (Tailwind from boots)", { buffOnslaught = false, buffTailwind = false, conditionCritRecently = true, conditionKilledRecently = false })
run("Onslaught only", { buffOnslaught = true, buffTailwind = false, conditionCritRecently = false })
run("Onslaught + crit recently (Tailwind)", { buffOnslaught = true, conditionCritRecently = true, conditionKilledRecently = true })
