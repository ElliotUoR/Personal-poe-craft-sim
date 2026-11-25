-- Movement speed with the current boots vs a candidate item, incl. action speed (Tailwind),
-- with "killed recently" off/on. cwd = PathOfBuilding/src.
-- Usage: luajit compare_boots_ms.lua <build.xml> <item.txt> [slot]
local xmlPath, itemPath, slotName = assert(arg[1]), assert(arg[2]), arg[3] or "Boots"
local here = debug.getinfo(1, "S").source:sub(2):match("(.*/)") or "./"
dofile(here .. "pob_env.lua")
local f = assert(io.open(xmlPath, "r")); local xmlText = f:read("*a"); f:close()
f = assert(io.open(itemPath, "r")); local itemText = f:read("*a"); f:close()
loadBuildFromXML(xmlText, "imported")
local it, ct, cs = build.itemsTab, build.configTab, build.calcsTab

local db = cs.mainEnv.player.modDB
print("ONSLAUGHT_SOURCES")
for _, m in ipairs(db:Tabulate("FLAG", nil, "Condition:Onslaught") or {}) do print("   ", tostring(m.mod.source)) end
print("FLASKS_ACTIVE_CONFIG", tostring(ct.input.conditionUsingFlask), "| Onslaught condition:", tostring(db.conditions.Onslaught))

local function report(label)
  for _, killed in ipairs({ false, true }) do
    ct.input.conditionKilledRecently = killed
    ct:BuildModList(); cs:BuildOutput()
    local o = cs.mainOutput
    print(string.format("%-14s killedRecently=%-5s MovementSpeedMod %.3f  ActionSpeedMod %.3f  Effective %.3f",
      label, tostring(killed), o.MovementSpeedMod, o.ActionSpeedMod or 1,
      o.EffectiveMovementSpeedMod or (o.MovementSpeedMod * (o.ActionSpeedMod or 1))))
  end
end
local current = it.items[it.slots[slotName].selItemId]
report(current and current.name:sub(1, 14) or "current")
local item = new("Item"):Item(itemText)
it:AddItem(item, true); it.slots[slotName]:SetSelItemId(item.id); it:PopulateSlots()
report(item.name:sub(1, 14))
