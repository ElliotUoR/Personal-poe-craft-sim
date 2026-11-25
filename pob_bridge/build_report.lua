-- One-line-per-stat report of a build: DPS, EHP, resources, attributes vs requirements,
-- passive points vs what the character level allows, and PoB's warnings.
-- Usage (cwd = PathOfBuilding/src): luajit build_report.lua <build.xml>
local xmlPath = assert(arg[1])
local here = debug.getinfo(1, "S").source:sub(2):match("(.*/)") or "./"
dofile(here .. "pob_env.lua")
local f = assert(io.open(xmlPath, "r")); local xmlText = f:read("*a"); f:close()
loadBuildFromXML(xmlText, "imported")
local cs = build.calcsTab
cs:BuildOutput()
local o = cs.mainOutput
local env = cs.mainEnv
local function p(k, v) print(string.format("R\t%s\t%s", k, tostring(v))) end
p("level", build.characterLevel)
p("mainSkill", env.player.mainSkill and env.player.mainSkill.activeEffect.grantedEffect.name)
for _, k in ipairs({ "CombinedDPS", "TotalDPS", "AverageHit", "Speed", "CritChance", "CritMultiplier", "Life", "EnergyShield",
  "Mana", "ManaUnreserved", "LifeUnreserved", "Armour", "Evasion", "TotalEHP", "PhysicalMaximumHitTaken", "FireMaximumHitTaken",
  "ColdMaximumHitTaken", "LightningMaximumHitTaken", "ChaosMaximumHitTaken", "FireResist", "ColdResist", "LightningResist",
  "ChaosResist", "FireResistOverCap", "ColdResistOverCap", "LightningResistOverCap", "EffectiveMovementSpeedMod",
  "Str", "Dex", "Int", "ReqStr", "ReqDex", "ReqInt", "Omni" }) do
  if o[k] ~= nil then p(k, o[k]) end
end
for _, a in ipairs({ "Str", "Dex", "Int" }) do
  if o["Req" .. a .. "Item"] then p("Req" .. a .. "Source", tostring(o["Req" .. a .. "Item"])) end
end
local used, ascUsed = build.spec:CountAllocNodes()
local extra = o.ExtraPoints or 0
p("passivePointsUsed", used)
p("passivePointsAllowed", (build.characterLevel - 1) + 23 + extra)
p("ascendancyPointsUsed", ascUsed)
-- PoB's own warnings (attribute requirements, too many points, jewel limits, ...)
build:RefreshStatList()
for _, line in ipairs((build.controls.warnings and build.controls.warnings.lines) or {}) do p("warning", line) end
