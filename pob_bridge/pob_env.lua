-- Shared setup for running Path of Building headless under stock LuaJIT.
-- Must be dofile'd with cwd = PathOfBuilding/src.
--  * module paths for PoB's bundled Lua libs, the luautf8 C module and lua-zlib
--  * real Inflate/Deflate (HeadlessWrapper's _SimpleGraphic.def.lua stubs them to "",
--    which silently breaks timeless-jewel lookup tables, e.g. "Missing LUT: Lethal Pride")
package.path = "../runtime/lua/?.lua;../runtime/lua/?/init.lua;" .. package.path
package.cpath = (os.getenv("POB_CPATH") or "/home/user/pob/deps/luautf8/?.so") .. ";"
  .. "/usr/lib/x86_64-linux-gnu/lua/5.1/?.so;" .. package.cpath

local zlib = require("zlib")

local function installZlib()
  function Inflate(data)
    if not data or data == "" then return nil, "no data" end
    local ok, out = pcall(function() return zlib.inflate(15 + 32)(data) end) -- auto-detect zlib/gzip header
    if ok then return out end
    return nil, tostring(out)
  end
  function Deflate(data, isGzip)
    local ok, out = pcall(function() return zlib.deflate(9, isGzip and 31 or 15)(data, "finish") end)
    if ok then return out end
    return nil, tostring(out)
  end
end

-- HeadlessWrapper loads _SimpleGraphic.def.lua (which defines the stubs) and then Launch.lua
-- inside the same dofile, so patch right after the stub file has run.
local origDofile = dofile
function dofile(path, ...)
  local results = { origDofile(path, ...) }
  if path == "_SimpleGraphic.def.lua" then installZlib() end
  return unpack(results)
end

dofile("HeadlessWrapper.lua")
dofile = origDofile
