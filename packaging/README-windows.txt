LiNa SM2 - Mod Launcher
=======================

A mod launcher for Warhammer 40,000: Space Marine 2.


HOW TO RUN IT
-------------

There is no installer. Put this folder wherever you like -- your
Documents folder is fine -- and double-click lina-sm2.exe.

Nothing is written to the Windows registry and nothing outside your user
account is touched. To uninstall, delete this folder.

Your settings, profiles and savegame backups live separately, under:

    %APPDATA%\lina-sm2
    %LOCALAPPDATA%\lina-sm2

Deleting the folder leaves those alone. Delete them by hand if you want
them gone as well.


TWO THINGS YOU WILL NOTICE
--------------------------

1. Windows shows "Windows protected your PC" the first time.

   This program is not signed with a code-signing certificate. Click
   "More info", then "Run anyway". That warning is about the missing
   signature, not about anything found in the file.

2. A black console window opens alongside the launcher.

   The same file is both the graphical launcher and a command line tool,
   and on Windows a program has to choose one of the two at build time.
   It currently chooses the command line, which is why the console
   appears. It can be closed only by closing the launcher.


COMMAND LINE
------------

The same executable works from PowerShell or cmd:

    lina-sm2.exe list          list every mod in load order
    lina-sm2.exe paths         show what was detected where
    lina-sm2.exe save backup   back up the savegames
    lina-sm2.exe --help        everything else

English and German are both built in: lina-sm2.exe lang de


THIS IS A TEST BUILD
--------------------

The Windows support has never run on a machine with Space Marine 2
actually installed. It compiles and its logic is covered by tests, but
game detection, the savegame paths and starting the game are unverified
here. Please report what happens -- especially if a path is not found.

The savegame functions take and verify a backup of their own before they
restore anything, on every platform. That safety net is not the part in
doubt; where the files are looked for is.


LICENSE
-------

MIT -- see the LICENSE file next to this one.
