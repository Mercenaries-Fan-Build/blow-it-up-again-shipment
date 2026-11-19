-- Blow It Up Again in the shell's Extras > DLC page, in the front end's Lua VM. qm loads this library at
-- shell.boot; it subscribes to shell.open.
--
-- SHELL.gfx (sha256 ca06957d...) shows its DLC button only while the movie's DLCVisibleVar is true when the
-- Extras page is built (the _extrastrans read). DLCAvailable(setting) sets it, and initializeSettings clears
-- it, both when the movie starts and from fadeToBlack (resetShell, restartShell and the join-game restarts).
-- Opening the DLC page empties DLCArray and then sends FSCommand onlineListDLCMaps (requestDLCMaps), so the
-- entry is added from that command's handler. Choosing an entry sends FSCommand onlineSelectDLC with the
-- entry's internal name (DLCSelectMap). The native FSCommand switch hands every command on to the Lua handlers
-- registered on the shell's Flash widget (v1.1 FUN_0061c170).

local OWNER = "blow-it-up-again"
local LEVEL = "dlc01"

-- addDLC(internalName, year, month, day, mapName, missionName, screenshot, missionDetails, isGood), the
-- parameter order of SHELL.gfx's DefineFunction2. The page draws mapName, month/day/year, missionName and
-- missionDetails; nothing in the movie reads screenshot or isGood.
-- missionName and missionDetails: the DLC's English stringdb (src/level/stringdb/0x9FE1E7D8.0.ucfx, UTF-16LE
-- at 0x15aa and 0x15d6). Date: the final DLC build's archive date (docs/dlc_loader_cross_reference.md:433).
local ENTRY = {
  LEVEL,
  2008,
  11,
  25,
  "Blow It Up Again",
  "Blow It Up Again Pack",
  "",
  "Blow it up again with four new explosive missions!",
  true,
}

import("MrxGuiBase")
import("MrxGuiShellBootstrap")

local QM = _G._QM
local bLoadedSeen = false

local function fail(sWhere, err)
  print("[biua] FATAL at " .. sWhere .. ": " .. tostring(err))
  QM.fail(OWNER, sWhere, err)
end

-- Runs fn under pcall and routes a failure to qm's error policy. Flash, widget-update and timer callbacks are
-- called by the engine, outside qm's own pcall.
local function guarded(sWhere, fn, ...)
  local ok, err = pcall(fn, ...)
  if not ok then
    fail(sWhere, err)
  end
end

local function shellFlash()
  if type(_GuiInternal) ~= "table" then
    error("_GuiInternal is missing from the front end's Lua state")
  end
  if type(_GuiInternal.SetFlashCallback) ~= "function" then
    error("_GuiInternal.SetFlashCallback is missing: MrxGuiBase.FlashWidget:SetFlashEventHandler would register nothing")
  end
  if type(_GuiInternal.CallFlashScriptFunction) ~= "function" then
    error("_GuiInternal.CallFlashScriptFunction is missing: the shell movie cannot be called")
  end
  local oShell = MrxGuiBase.GetWidgetByName("Shell")
  if oShell == nil then
    error("the Shell widget does not exist")
  end
  local oFlash = oShell.CustomData.oFlash
  if oFlash == nil then
    error("the Shell widget has no Flash widget (CustomData.oFlash)")
  end
  return oFlash
end

local function addEntry(oFlash)
  local tArgs = {}
  for i = 1, #ENTRY do
    tArgs[i] = ENTRY[i]
  end
  oFlash:CallActionScriptCallback("addDLC", tArgs)
  print("[biua] addDLC " .. LEVEL .. " sent to the shell movie")
end

local function onListDLCMaps(oFlash, sArg)
  guarded("onlineListDLCMaps", function()
    print("[biua] onlineListDLCMaps received (" .. tostring(sArg) .. ")")
    Event.Create(Event.TimerRelative, {0.01}, function(oTimerFlash)
      guarded("onlineListDLCMaps addDLC", addEntry, oTimerFlash)
    end, {oFlash})
  end)
end

local function startLevel()
  for _, sName in ipairs({"SetLevelName", "SetMasterScriptName", "StartSingleplayer"}) do
    if type(Sys[sName]) ~= "function" then
      error("Sys." .. sName .. " is missing from the front end's Lua state")
    end
  end
  MrxGuiShellBootstrap.nPlayersSelected = 1
  Sys.SetLevelName(LEVEL)
  Sys.SetMasterScriptName(LEVEL)
  print("[biua] Sys.StartSingleplayer(" .. LEVEL .. ", " .. LEVEL .. ")")
  Sys.StartSingleplayer(LEVEL, LEVEL)
  MrxGuiShellBootstrap.bNeedsReloading = true
end

local function onSelectDLC(oFlash, sArg)
  guarded("onlineSelectDLC", function()
    print("[biua] onlineSelectDLC received (" .. tostring(sArg) .. ")")
    if sArg ~= LEVEL then
      error("onlineSelectDLC named " .. tostring(sArg) .. "; the only DLC entry this Shipment adds is " .. LEVEL)
    end
    startLevel()
  end)
end

local function register(oFlash)
  oFlash:SetFlashEventHandler("onlineListDLCMaps", onListDLCMaps, {})
  oFlash:SetFlashEventHandler("onlineSelectDLC", onSelectDLC, {})
end

-- The shell Flash widget's per-frame update (Widget:SetEventHandler("GuiUpdate"), mrxguibase.lua:515-517; the
-- fade movie's own update is set the same way, mrxgui.lua:160). Each frame the movie is loaded it sets
-- DLCVisibleVar again, so a reset is undone before the next frame. On each new load of the movie it registers
-- the command handlers again.
local function assertAvailable(oFlash)
  if oFlash.CustomData.bLoaded ~= true then
    bLoadedSeen = false
    return
  end
  if not bLoadedSeen then
    bLoadedSeen = true
    register(oFlash)
    print("[biua] shell movie loaded: DLC handlers registered, DLCAvailable(true) asserted each frame")
  end
  oFlash:CallActionScriptCallback("DLCAvailable", {true})
end

local function update(oFlash, nDeltaTime)
  guarded("shell DLC update", assertAvailable, oFlash)
end

QM.on("shell.open", function()
  local oFlash = shellFlash()
  register(oFlash)
  bLoadedSeen = false
  oFlash:SetEventHandler("GuiUpdate", update)
  print("[biua] shell.open: DLC handlers registered on the shell movie, per-frame DLCAvailable armed")
end, OWNER)

print("[biua] biua_extras_dlc: body ran")
