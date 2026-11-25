-- Movement speed from the equipped boots, including "increased bonuses from equipped boots"
-- scaling (Wickerman/Viscera items, passives, custom mods). cwd = PathOfBuilding/src.
local xmlPath = assert(arg[1])
local here = debug.getinfo(1, "S").source:sub(2):match("(.*/)") or "./"
dofile(here .. "pob_env.lua")
local f = assert(io.open(xmlPath, "r")); local xmlText = f:read("*a"); f:close()
loadBuildFromXML(xmlText, "imported")

local itemsTab, calcsTab = build.itemsTab, build.calcsTab
print("== equipped Viscera (Wickerman) items")
for _, slot in ipairs(itemsTab.orderedSlots) do
  local it = itemsTab.items[slot.selItemId]
  if it and it.type == "Viscera" then
    print(slot.slotName, it.name)
    for _, ml in ipairs(it.explicitModLines or {}) do print("   ", ml.line) end
  end
end

local env = calcsTab.mainEnv
local function sources(db, name)
  for _, m in ipairs(db:Tabulate("INC", nil, name) or {}) do
    print(string.format("   %s %+g%%  from %s", m.mod.type, m.value, tostring(m.mod.source)))
  end
end
print("== EffectOfBonusesFromBoots sources (player modDB)")
sources(env.player.modDB, "EffectOfBonusesFromBoots")
-- (the player modDB holds tree, config and Viscera mods, i.e. what CalcSetup scales boots by)
print("== total boot bonus multiplier:", calcLib.mod(env.player.modDB, nil, "EffectOfBonusesFromBoots"))

local out = calcsTab.mainOutput
print("== movement speed now: MovementSpeedMod", out.MovementSpeedMod)
for _, m in ipairs(env.player.modDB:Tabulate("OVERRIDE", nil, "MovementSpeed") or {}) do
  print(string.format("   OVERRIDE %g from %s", m.value, tostring(m.mod.source)))
end
local calcFunc = calcsTab:GetMiscCalculator()
local noBoots = calcFunc({ repSlotName = "Boots", repItem = new("Item"):Item("Rarity: NORMAL\nAssassin's Boots\nItem Level: 70\nQuality: 0\nImplicits: 0\n") })
print("== with plain Assassin's Boots instead: MovementSpeedMod", noBoots.MovementSpeedMod)
-- same boots without the Wickerman boot scaling: drop EffectOfBonusesFromBoots from the leg's mods
for _, slot in ipairs(itemsTab.orderedSlots) do
  local it = itemsTab.items[slot.selItemId]
  if it and it.type == "Viscera" then
    for i = #it.modList, 1, -1 do
      if it.modList[i].name == "EffectOfBonusesFromBoots" then table.remove(it.modList, i) end
    end
  end
end
calcsTab:BuildOutput()
print("== same boots, Wickerman boot scaling removed: MovementSpeedMod", calcsTab.mainOutput.MovementSpeedMod)
