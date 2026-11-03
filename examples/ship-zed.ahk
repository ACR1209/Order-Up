#Requires AutoHotkey v2.0
#SingleInstance Force
SetTitleMatchMode 2

; ---- settings ---------------------------------------------------------------
REQUIRE_ACTIVE := true         ; true: only send if a Zed window is the active window
                               ; false: if Zed isn't active, use the most recently used Zed window
FOCUS_KEYS := "^!a"            ; Zed keybinding that focuses the agent input
COUNTDOWN  := 5                ; seconds you have to cancel with Esc
SEND_ENTER := true             ; set false while testing: types the prompt but doesn't submit
PROMPT := "Ship the changes we've been working on: run the tests, commit on a new branch, push, and open a PR. Never push to main."
; -----------------------------------------------------------------------------

cancelled := false

CancelShip(*) {
    global cancelled
    cancelled := true
}

; Small always-on-top toast that never steals focus
g := Gui("+AlwaysOnTop -Caption +ToolWindow +E0x08000000")
g.BackColor := "202020"
g.SetFont("s12 cWhite", "Segoe UI")
msg := g.Add("Text", "w340 Center", "")
g.Show("NoActivate x" (A_ScreenWidth - 380) " y40")

Say(text) {
    msg.Value := text
}

Fail(text) {
    Say(text)
    Sleep 3000
    ExitApp
}

; 1) Cancel window
Hotkey "Esc", CancelShip, "On"
Loop COUNTDOWN * 10 {
    if cancelled
        break
    if Mod(A_Index - 1, 10) = 0
        Say("Shipping in " (COUNTDOWN - (A_Index - 1) // 10) "s - press Esc to cancel")
    Sleep 100
}
Hotkey "Esc", "Off"
if cancelled
    Fail("Cancelled")

; 2) Use the active Zed window, or abort without typing anything
hwnd := 0
if WinActive("ahk_exe zed.exe")
    hwnd := WinExist("A")
else if REQUIRE_ACTIVE
    Fail("Zed isn't the active window. Nothing sent.")
else
    hwnd := WinExist("ahk_exe zed.exe")
if !hwnd
    Fail("No Zed window found. Nothing sent.")
WinActivate hwnd
if !WinWaitActive("ahk_id " hwnd, , 2)
    Fail("Could not focus Zed. Nothing sent.")

; 3) Focus the agent input, type the prompt, submit
Say("Sending to Claude Code...")
Send FOCUS_KEYS
Sleep 500
SendText PROMPT
Sleep 200
if SEND_ENTER
    Send "{Enter}"
Sleep 800
ExitApp
