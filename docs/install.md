# How to start Mango

A step-by-step guide for Windows, macOS and Linux. No programming needed.

Mango is a program that runs on your computer and shows its pages in a window
(or in your web browser). Your library, reading progress and settings stay on
your computer.


## 1. Download the right file

Open the releases page:

    https://github.com/apixelsnet/mango/releases/latest

Under **Assets**, take the one file that matches your computer:

| Your computer                               | File to download                       |
| ------------------------------------------- | -------------------------------------- |
| Windows                                     | `mango-<version>-windows-x86_64.zip`   |
| Mac with an Apple chip (M1, M2, M3, M4...)  | `mango-<version>-macos-aarch64.tar.gz` |
| Mac with an Intel chip                      | `mango-<version>-macos-x86_64.tar.gz`  |
| Linux, most PCs and laptops                 | `mango-<version>-linux-x86_64.tar.gz`  |
| Linux on a Raspberry Pi or other ARM board  | `mango-<version>-linux-aarch64.tar.gz` |

`<version>` is a number such as `1.1.14`. Ignore `SHA256SUMS` and the
"Source code" files.

Not sure which Mac you have? Click the Apple menu, then **About This Mac**.
"Chip: Apple M..." means an Apple chip; "Processor: ... Intel" means Intel.


## 2. What is in the folder

| File                      | What it is                                                         |
| ------------------------- | ------------------------------------------------------------------ |
| `mango-desktop`           | **Start this one.** Mango in its own window, with a tray icon.     |
| `mango`                   | Mango itself, without a window. `mango-desktop` starts it for you. |
| `host` (folder)           | What extensions need. Leave it where it is.                        |
| `README.md`, `LICENSE`... | Text files. Not needed to run Mango.                               |

On Windows the two programs are `mango-desktop.exe` and `mango.exe`. If Windows
hides file endings, they show as just `mango-desktop` and `mango`.

Keep these files together in one folder: `mango-desktop` looks for `mango` and
`host` right beside it.


## 3. Start it

### Windows

1. Find the downloaded `.zip` file (usually in **Downloads**).
2. Right-click it, choose **Extract All...**, then **Extract**.
   Do not open Mango from inside the zip: it would not find its other files.
3. Optional: move the extracted folder somewhere you will find again, such as
   **Documents**.
4. Open the folder and double-click **`mango-desktop.exe`**.
5. Windows may show a blue box, "Windows protected your PC". Mango is not
   signed with a paid certificate, so Windows does not know it yet. Click
   **More info**, then **Run anyway**. It asks only the first time.
6. A Mango window opens. Go on to "4. First visit" below.

Closing the window does **not** stop Mango. It keeps running as a small icon
near the clock (look under the `^` arrow if you do not see it), so downloads
carry on. To stop Mango, right-click that icon and choose **Quit**.

If the window stays blank, Windows is missing Microsoft Edge WebView2, which
draws the window. Install the "Evergreen Bootstrapper" from
https://developer.microsoft.com/microsoft-edge/webview2/ and try again, or use
Mango in your browser instead (see "Without the window" below).

### macOS

1. Find the downloaded `.tar.gz` file in **Downloads** and double-click it.
   A folder named `mango-<version>-macos-...` appears beside it.
2. macOS blocks programs from the internet that do not come from the App Store
   or a paid developer account. To allow this one, open **Terminal** (press
   Cmd + Space, type `Terminal`, press Enter) and type this, with a space at
   the end, **without pressing Enter yet**:

       xattr -dr com.apple.quarantine 

   Drag the Mango folder from Finder into the Terminal window (that types its
   location for you), then press Enter. Nothing is printed when it works.
3. Open the folder and double-click **`mango-desktop`**.
4. A Mango window opens. Go on to "4. First visit" below.

Closing the window leaves Mango running behind an icon in the menu bar at the
top of the screen. Click it and choose **Quit** to stop Mango.

### Linux

1. Unpack the download. Most file managers do it with a right-click,
   **Extract Here**. Or in a terminal:

       tar -xzf mango-*-linux-*.tar.gz

2. The window needs WebKitGTK 4.1. On Debian or Ubuntu:

       sudo apt install libwebkit2gtk-4.1-0

   On Fedora: `sudo dnf install webkit2gtk4.1`.
3. Open the folder and double-click **`mango-desktop`**, or in a terminal:

       cd mango-*-linux-*/
       ./mango-desktop

4. A Mango window opens. Go on to "4. First visit" below.

The ARM download (`aarch64`) has no `mango-desktop`. Start `mango` instead, as
in "Without the window" below.

### Without the window

On every system you can start `mango` (`mango.exe` on Windows) instead of
`mango-desktop`, and read in your usual web browser:

1. Double-click `mango` (or run `./mango` in a terminal). A black text window
   opens and prints a few lines. **Leave it open**: closing it stops Mango.
2. In your browser, go to http://127.0.0.1:8787

Started this way, Mango keeps its data in a `data` folder inside the Mango
folder.

### With Docker

If you already use Docker, the README's Docker section is shorter:
https://github.com/apixelsnet/mango#docker


## 4. First visit

1. Mango asks you to choose a password. Pick one and keep it: it protects your
   library.
2. A short tour shows you around. You can skip it.
3. To read your own files (`.cbz`, `.zip` or folders of pictures), see
   "Your own files" in the README:
   https://github.com/apixelsnet/mango#your-own-files


## 5. Extensions need Java

Mango reads online sources through extensions, and extensions need **Java 21
or newer**. Without Java, Mango still opens and reads your own files; only
extensions do not work.

To check, open a terminal (on Windows: press the Windows key, type `cmd`,
press Enter) and type:

    java -version

If it prints version 21 or higher, you are done. If it says the command is not
found, or prints a lower number:

1. Go to https://adoptium.net and download **Temurin 21** or newer. The site
   picks the right file for your system.
2. Run the installer and click through it. On Windows, leave "Add to PATH"
   switched on (it is by default).
3. Quit Mango (tray icon, **Quit**) and start it again, so it finds Java.

On Linux, your package manager has it, for example
`sudo apt install openjdk-21-jre-headless`.

Then open **Extensions** in Mango, add a repository by its link, and install the
sources you want. More in the README:
https://github.com/apixelsnet/mango#extensions


## Every day after

Double-click `mango-desktop` the same way. Your library is kept between starts.

To start it from the desktop on Windows: right-click `mango-desktop.exe`,
**Show more options**, **Send to**, **Desktop (create shortcut)**. Use a
shortcut rather than moving the program itself.


## Updating to a new version

Settings shows when a new version is out. Mango does not update itself:

1. Quit Mango (tray icon, **Quit**).
2. Download the new file as in step 1 and unpack it.
3. Copy everything in it over your old Mango folder, replacing the old files.
4. Start `mango-desktop` again.

Your library, progress and settings are not among the files you replace, so
they stay.


## Where your data is

| Started with             | Data folder                                                          |
| ------------------------ | -------------------------------------------------------------------- |
| `mango-desktop`, Windows | `%APPDATA%\Mango\data` (paste it into the File Explorer address bar) |
| `mango-desktop`, macOS   | `~/Library/Application Support/Mango/data`                           |
| `mango-desktop`, Linux   | `~/.local/share/mango/data`                                          |
| `mango`                  | the `data` folder inside the Mango folder                            |

Copy this folder somewhere safe to back up your library. Deleting it starts
Mango from scratch.


## Something went wrong

**"Mango could not start" or "Mango did not answer".** Another program may be
using the same port (8787), or an earlier Mango is still running. Quit every
Mango (the tray icon, and any black `mango` text window) and try again. The
message names Mango's log file, which says what went wrong.

**"mango is not in ...".** `mango-desktop` was moved out of its folder. Put it
back beside `mango` and `host`, and make a shortcut instead.

**Forgot the password.** Quit Mango, then open a terminal in the Mango folder
(on Windows: click the File Explorer address bar, type `cmd`, press Enter) and
run the lines for how you start Mango:

    Windows, mango-desktop:   set MANGO_DATA=%APPDATA%\Mango\data
                              mango.exe reset-password
    macOS, mango-desktop:     MANGO_DATA="$HOME/Library/Application Support/Mango/data" ./mango reset-password
    Linux, mango-desktop:     MANGO_DATA="$HOME/.local/share/mango/data" ./mango reset-password
    mango on its own:         mango reset-password      (./mango on macOS and Linux)

Start Mango again and choose a new password.

**Extensions say "Could not run Java".** Java is missing or too old. See
step 5.

**Reading on a phone or another computer.** At first only this computer can
open Mango. The README's Settings section shows how to open it to your home
network (`MANGO_LISTEN`): https://github.com/apixelsnet/mango#settings

Still stuck? Open an issue saying which system you use and what you saw:
https://github.com/apixelsnet/mango/issues/new/choose
