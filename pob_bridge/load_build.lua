-- Headless Path of Building: load a build XML and print its main stats.
-- Usage (cwd must be PathOfBuilding/src):
--   luajit /path/to/load_build.lua /path/to/build.xml
local xmlPath = assert(arg[1], "usage: luajit load_build.lua <build.xml>")  -- PoB replaces `arg` while booting
local here = debug.getinfo(1, "S").source:sub(2):match("(.*/)") or "./"
dofile(here .. "pob_env.lua")

local f = assert(io.open(xmlPath, "r"))
local xmlText = f:read("*a")
f:close()
loadBuildFromXML(xmlText, "imported")

local out = build.calcsTab.mainOutput
local keys = { "TotalDPS", "CombinedDPS", "AverageHit", "Speed", "CritChance", "CritMultiplier",
  "Life", "EnergyShield", "Mana", "Armour", "Evasion", "TotalEHP", "PhysicalMaximumHitTaken",
  "FireMaximumHitTaken", "ColdMaximumHitTaken", "LightningMaximumHitTaken", "ChaosMaximumHitTaken",
  "FireResist", "ColdResist", "LightningResist", "ChaosResist", "MovementSpeedMod" }
print("BUILD", build.buildName, "level", build.characterLevel, "class", build.spec.curClassName,
  "ascendancy", build.spec.curAscendClassName)
local env = build.calcsTab.mainEnv
if env and env.player and env.player.mainSkill then
  print("MAINSKILL", env.player.mainSkill.activeEffect.grantedEffect.name)
end
for _, k in ipairs(keys) do
  if out[k] ~= nil then print("STAT", k, out[k]) end
end
