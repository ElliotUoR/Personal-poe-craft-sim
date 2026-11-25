-- Key stats with the current item in a slot vs a candidate. cwd = PathOfBuilding/src.
local xmlPath, itemPath, slotName = assert(arg[1]), assert(arg[2]), arg[3] or "Boots"
local here = debug.getinfo(1, "S").source:sub(2):match("(.*/)") or "./"
dofile(here .. "pob_env.lua")
local f = assert(io.open(xmlPath, "r")); local xmlText = f:read("*a"); f:close()
f = assert(io.open(itemPath, "r")); local itemText = f:read("*a"); f:close()
loadBuildFromXML(xmlText, "imported")
local it, cs = build.itemsTab, build.calcsTab
local keys = { "CombinedDPS", "TotalDPS", "Life", "EnergyShield", "TotalEHP", "PhysicalMaximumHitTaken", "ChaosMaximumHitTaken",
  "FireResist", "ColdResist", "LightningResist", "ChaosResist", "ManaUnreserved", "LifeUnreserved", "EffectiveMovementSpeedMod",
  "Str", "Dex", "Int" }
local function snap() cs:BuildOutput(); local o, r = cs.mainOutput, {}; for _, k in ipairs(keys) do r[k] = o[k] end; return r end
local a = snap()
local item = new("Item"):Item(itemText)
it:AddItem(item, true); it.slots[slotName]:SetSelItemId(item.id); it:PopulateSlots()
local b = snap()
print(string.format("%-26s %16s %16s %9s", "stat", "current", "candidate", "change"))
for _, k in ipairs(keys) do
  local x, y = a[k], b[k]
  if x ~= nil and y ~= nil then
    local d = (x ~= 0) and string.format("%+.1f%%", (y - x) / math.abs(x) * 100) or ""
    print(string.format("%-26s %16.2f %16.2f %9s", k, x, y, d))
  end
end
for _, w in ipairs(cs.mainEnv.itemWarnings and {} or {}) do print(w) end
