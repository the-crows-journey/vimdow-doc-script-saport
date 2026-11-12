<h1 align="center"> Vimdow </h1>

<h1 align="center">
    <img src="img/vimdow_logo.png" alt="Vimdow Logo" width="320" height="320">
</h1>
<div align="center">
    It's two things at once!
    <ul style="text-align: center; list-style-position: inside;">
        <li> A <a href="https://Neovim.io">Neovim</a> client based on godot </li>
        <li> A <a href="https://godotengine.org">Godot</a> editor plugin that lets you use Neovim </li>
    </ul>
</div>

<p align="center">
    <img alt="GitHub Downloads (all assets, all releases)" src="https://img.shields.io/github/downloads/migmoog/vimdow/total">
    <img alt="GitHub Release" src="https://img.shields.io/github/v/release/migmoog/vimdow">
</p>

## About

Tired of godot's in-house script editor? **Vimdow** can let you use the comfort of your own Neovim config 
to edit text for any file you want!

Vimdow can be installed like a regular plugin off of the asset store, or
it can be downloaded as a standalone Neovim client for your system.

### What's Neovim?

If you don't know, [watch this](https://www.youtube.com/watch?v=c4OyfL5o7DU).

In short it's a keyboard centric text editor that can enable high productivity should you choose to master it.
It also follows the philosophy of "Configuration as Code", meaning you can create a highly personalized environment suited 
to your needs.

Vimdow aims to provide this accessibility to Godot developers!

### Project Goals / Features

Vimdow provides a developer experience with:
- A visually customizable Neovim frontend
- Portability, vimdow can work wherever Godot can
- Integration that is seamless between both the Godot Editor and using Neovim on your own system
- Non invasive integration into the project for teams of any size

## Configuration

**Requirements**: Neovim 0.11 or later.

Vimdow only needs to know where Neovim's binary is located on your system to get working. By default both modes assume 
that you have Neovim in your `PATH`

### Plugin mode

#### Path to Neovim

In `addons/vimdow`, there is a default file called `local.cfg`. It will have default settings. To start vimdow, you need to set the `path_to_nvim` key in the `[neovim]` section. You should also remove the `template=true` to remove the warning.

It is reccomended that you add `local.cfg` to the `.gitignore` of your project. It is meant to be your way to configure vimdow for your system only as a way for collaborating groups on the same project with their own preferences.

#### Theme

There are two options. First are the settings in the `[theme]` section of `local.cfg` (Read the config file section in **Standalone mode**). Your other option is to 
edit `addons/vimdow/vimdow_theme.tres` in the editor to do things like change fonts and default font size. However, this approach isn't reccomended as it will make your changes committed to the entire respository.

#### Shortcuts

Keyboard shortcuts (such as font size) are located under `Editor Settings > Shortcuts > Vimdow`.

#### Lua plugin

Vimdow is also a Neovim plugin. The code for it is located in `addons/vimdow/lua`. In it is the `start.lua` script that is used on neovim startup with the `-S` flag. The other file is the configurations, the defaults of which are:
```lua
{
	-- Default keybindings for vimdow actions
	keybinds = {
		toggle_breakpoint = "<leader>gb",
		clear_breakpoints = "<leader>cb",
		release_focus = "<C-Esc>"
	},

	-- default color themes
	colors = {
		-- color when a brekpoint gutter is hovered with a mouse
		breakpoint_hover = "#ffabb2",

		-- color when a breakpoint is set
		set_breakpoint = "#ff0016"
	},
}
```

### Godot documentation

Vimdow shows Godot's own class reference pages inside the Vimdow tab. Godot still does the searching and symbol lookup:

- `F1` (Godot's native search dialog), "Open Documentation" in the scene tree / inspector, and links inside a reference page open the page as a **document**: a listed Neovim buffer named `godot-doc://<Class>`. Your script stays in its own buffer.
- Switch, split and close documentation exactly like files (`:bnext`, `:b`, `<C-w>`, `:bd`, tabs, your buffer plugins). The buffer holds the page's text (one buffer line per wrapped line, and one per code line of a code block), so Neovim motions, `/` search and yanks work on it, and the page scrolls to keep the line under Neovim's cursor in view. The mouse works as in Godot's own documentation: wheel scrolling, clicking links and selecting text.
- In a GDScript buffer, right click a symbol and pick **Open in Documentation**, press `<leader>hd` (`keybinds.open_docs`), or run `:VimdowDocs [symbol]`.

**Limitations:** Godot does not expose its reference viewer (`EditorHelp`) to extensions. Vimdow moves the page Godot creates into the Vimdow editor and draws it over the Neovim window showing its document buffer (Neovim itself can only draw text cells), so the Script Editor may flash for a frame and its tab list can show a stale entry until it refreshes.

**Documentation pages have no cursor.** The page is drawn by Godot's `RichTextLabel`, which cannot report where a character is (no per-character rectangle, no font of a run of text, nothing about the lines inside a code block). A cursor could only be guessed from font widths and derived padding, and a guessed cursor is wrong on lines that mix fonts and in code blocks, so none is drawn. Neovim's own cursor still moves and the page follows its line, but clicking a page does not move it. The script editor's cursor is not affected.

### The built-in Script Editor tab

While Vimdow is enabled its **Script** tab is hidden from the main screen switcher (Godot has no supported API for this, so the tab's button is found in the editor UI; if it cannot be found, any switch to the Script screen is sent back to Vimdow instead). The Script Editor *feature* stays enabled, so everything that depends on it keeps working natively: the Scene dock's **Attach Script**, **Remove Script** and **Extend Script** buttons, the Scene tree context menu, and the class reference pages Vimdow adopts. Scripts that Godot opens (for example after attaching one) open in Vimdow. Disabling the plugin shows the tab again. Do not disable the Script feature in *Manage Editor Features*: Godot then also hides those Scene dock buttons.

### Standalone mode

Vimdow as a standalone client looks for a godot [ConfigFile](https://docs.godotengine.org/en/stable/classes/class_configfile.html#configfile) on your system. It checks for an environment variable called
`VIMDOW_CONFIG_PATH`, but by default it will search for `user://vimdow.cfg`.

#### Default config file

```cfg
[neovim]

; delete this key or set it to false to remove warnings
template = true

; by default assumes nvim is in your path
path_to_nvim="nvim"


; Overrides defaults in the plugin's theme resource.
; Reccomended for individual configurations to visuals.
[theme]

font_size=16

normal = "./path/to/normal.ttf"
bold = "../path/to/bold.ttf"
italic = "/path/to/italic.ttf"
```

## Contributing

Vimdow is open source and open to any and all contributions. 
If you need a quick and easy introduction to the steps of compilation and source code, check out [CONTRIBUTING.md](CONTRIBUTING.md)
