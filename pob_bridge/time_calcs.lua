-- Time PoB calculations on a loaded build: full output rebuild vs. the item-comparison
-- calculator (GetMiscCalculator with a slot override, what PoB's tooltips use).
-- Usage (cwd = PathOfBuilding/src): luajit time_calcs.lua <build.xml> [slot]
local xmlPath = assert(arg[1])
local slotName = arg[2] or "Body Armour"
local here = debug.getinfo(1, "S").source:sub(2):match("(.*/)") or "./"
dofile(here .. "pob_env.lua")
local f = assert(io.open(xmlPath, "r")); local xmlText = f:read("*a"); f:close()
loadBuildFromXML(xmlText, "imported")

local slot = build.itemsTab.slots[slotName]
local item = slot and build.itemsTab.items[slot.selItemId]
print("SLOT", slotName)
if item then print("ITEM\n" .. item.raw) end

local clock = os.clock
local t0 = clock()
local N = 10
for _ = 1, N do build.calcsTab:BuildOutput() end
print(string.format("FULL_REBUILD_MS\t%.1f", (clock() - t0) / N * 1000))

local calcFunc, baseOut = build.calcsTab:GetMiscCalculator()
t0 = clock()
local out
for _ = 1, N do out = calcFunc({ repSlotName = slotName, repItem = item }) end
print(string.format("ITEM_SWAP_CALC_MS\t%.1f", (clock() - t0) / N * 1000))
print("CHECK_TotalDPS", baseOut.TotalDPS, out.TotalDPS)
