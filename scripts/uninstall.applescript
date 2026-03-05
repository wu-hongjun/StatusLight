-- Uninstall OpenSlicky
-- Double-clickable uninstaller for macOS

on run
	set dialogResult to display dialog "This will uninstall OpenSlicky and remove:" & linefeed & linefeed & ¬
		"  • CLI tools (/usr/local/bin/slicky, slickyd)" & linefeed & ¬
		"  • Launch daemon" & linefeed & ¬
		"  • OpenSlicky.app from Applications" & linefeed & linefeed & ¬
		"Your configuration at ~/.config/openslicky/ will be preserved." & linefeed & linefeed & ¬
		"Continue?" with title "Uninstall OpenSlicky" buttons {"Cancel", "Uninstall"} default button "Cancel" with icon caution

	if button returned of dialogResult is "Uninstall" then
		doUninstall()

		set purgeResult to display dialog "Remove configuration files too?" & linefeed & linefeed & ¬
			"This deletes ~/.config/openslicky/ (settings, Slack token, custom presets)." with title "Uninstall OpenSlicky" buttons {"Keep Config", "Remove Config"} default button "Keep Config"

		if button returned of purgeResult is "Remove Config" then
			do shell script "rm -rf ~/.config/openslicky/"
		end if

		display dialog "OpenSlicky has been uninstalled." with title "Uninstall OpenSlicky" buttons {"OK"} default button "OK" with icon note
	end if
end run

on doUninstall()
	-- 1. Quit the running app if open
	try
		tell application "OpenSlicky" to quit
	end try
	-- Wait for app to exit (up to 5 seconds), then force kill
	try
		do shell script "for i in 1 2 3 4 5 6 7 8 9 10; do pgrep -x OpenSlicky >/dev/null 2>&1 || exit 0; sleep 0.5; done; killall -9 OpenSlicky 2>/dev/null; exit 0"
	end try

	-- 2. Unload LaunchAgent
	set plistPath to (POSIX path of (path to home folder)) & "Library/LaunchAgents/com.openslicky.daemon.plist"
	try
		do shell script "launchctl unload -w " & quoted form of plistPath
	end try
	try
		do shell script "rm -f " & quoted form of plistPath
	end try

	-- 3. Kill slickyd and slicky processes, wait for confirmed exit
	try
		do shell script "killall slickyd 2>/dev/null; for i in 1 2 3 4 5 6 7 8 9 10; do pgrep -x slickyd >/dev/null 2>&1 || break; sleep 0.5; done; killall -9 slickyd 2>/dev/null; killall slicky 2>/dev/null; for i in 1 2 3 4 5 6 7 8 9 10; do pgrep -x slicky >/dev/null 2>&1 || break; sleep 0.5; done; killall -9 slicky 2>/dev/null; exit 0"
	end try

	-- 4. Turn off the light (now no other process holds the HID handle)
	try
		do shell script "/usr/local/bin/slicky off"
	on error
		try
			do shell script "/Applications/OpenSlicky.app/Contents/MacOS/slicky off"
		end try
	end try

	-- 5. Remove symlinks (requires admin) and app bundle
	try
		do shell script "rm -f /usr/local/bin/slicky /usr/local/bin/slickyd && rm -rf /Applications/OpenSlicky.app" with administrator privileges
	on error
		display dialog "Could not remove CLI tools (admin access denied)." & linefeed & "You can remove them manually:" & linefeed & linefeed & "  sudo rm -f /usr/local/bin/slicky /usr/local/bin/slickyd" with title "Uninstall OpenSlicky" buttons {"OK"} default button "OK" with icon caution
	end try

	-- 6. Remove install markers (so reinstall works correctly)
	try
		do shell script "rm -f ~/.config/openslicky/.installed-*"
	end try
end doUninstall
