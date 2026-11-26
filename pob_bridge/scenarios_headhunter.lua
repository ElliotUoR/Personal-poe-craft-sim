-- Headhunter scenarios built from PoEDB Archnemesis mod texts (https://poedb.tw/us/Monster_Modifiers, fetched 2026-11-26)
local weaver = function(e) return "50% increased " .. e .. " Damage\nGain 50% of Physical Damage as Extra " .. e .. " Damage\n25% of Physical Damage Converted to " .. e .. " Damage\n" end
local strider = function(e) return "30% increased " .. e .. " Damage\nGain 25% of Physical Damage as Extra " .. e .. " Damage\n50% of Physical Damage Converted to " .. e .. " Damage\n" end
return {
  { name = "no HH buffs", mods = "" },
  { name = "Flameweaver", mods = weaver("Fire") },
  { name = "Frost Strider", mods = strider("Cold") },
  { name = "3 weavers", mods = weaver("Fire") .. weaver("Cold") .. weaver("Lightning") },
  { name = "XtraFire+Exposure", mods = "Gain 50% of Physical Damage as Extra Fire Damage\nInflict Fire Exposure on Hit\n" },
}
