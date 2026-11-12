@tool
class_name VimdowScriptTab
extends RefCounted
## Hides the built-in Script Editor's main-screen tab while Vimdow is active.
##
## Only the *tab* is hidden. The Script Editor feature stays enabled, so the editor's own
## script handling keeps working: the Scene dock's Attach / Remove / Extend Script buttons
## and context menu entries, class reference pages, `ScriptCreateDialog`, and so on.
##
## Why the scene tree
## ------------------
## Godot has no EditorPlugin/EditorInterface API for hiding another main screen's tab
## (`EditorInterface.set_main_screen_editor` only switches; `EditorFeatureProfile` would
## disable the whole feature, including the Scene dock's script buttons). The tabs are
## ordinary nodes of the editor UI, so this finds the "Script" one and hides it:
##
## * Godot 4.5 / 4.6: a `Button` named "Script" inside the `EditorMainScreenButtons`
##   container of the title bar. `EditorMainScreen` already treats a hidden button as "this
##   editor is unavailable": `select()` ignores it, next/previous screen skips it, and
##   `EditorNode` no longer picks that plugin as the main editor for an edited object, so
##   it can neither take over the workspace nor intercept scripts that Vimdow handles.
## * Newer editors where the main screen is a tab container: the Script Editor's tab is
##   hidden with `TabContainer.set_tab_hidden`.
##
## The editor shows the button again whenever the active feature profile changes
## (`EditorMainScreen.set_button_enabled`), so the button's visibility is watched and hidden
## again. If neither layout is found nothing is hidden (`hide_tab` returns false); the
## plugin still bounces any switch to the Script screen back to Vimdow.

## Name of the Script Editor's main-screen entry (the plugin name Godot registers it with).
const SCREEN_NAME := "Script"
## Name of the title bar container that holds the main-screen buttons (Godot 4.5 / 4.6).
const BUTTONS_CONTAINER := "EditorMainScreenButtons"

var _button: Button
var _tab_container: TabContainer
var _tab_control: Control


## Hides the tab. Returns true if the Script Editor's tab was found and hidden.
func hide_tab() -> bool:
	if _button != null or _tab_container != null:
		return true # already hidden

	_button = _find_screen_button()
	if _button != null:
		_button.visibility_changed.connect(_on_button_visibility_changed)
		_button.visible = false
		return true

	var script_editor := EditorInterface.get_script_editor()
	if script_editor != null and script_editor.get_parent() is TabContainer:
		_tab_container = script_editor.get_parent()
		_tab_control = script_editor
		_set_tab_hidden(true)
		return true
	return false


## Shows the tab again (plugin disabled), unless the active feature profile disables the
## Script feature, in which case the editor wants it hidden.
func restore_tab() -> void:
	if _button != null and is_instance_valid(_button):
		if _button.visibility_changed.is_connected(_on_button_visibility_changed):
			_button.visibility_changed.disconnect(_on_button_visibility_changed)
		_button.visible = not _is_script_feature_disabled()
	_button = null

	if _tab_container != null and is_instance_valid(_tab_container):
		_set_tab_hidden(false)
	_tab_container = null
	_tab_control = null


## True if the Script Editor's tab is currently the selected main screen.
func is_selected() -> bool:
	return _button != null and is_instance_valid(_button) and _button.button_pressed


func _find_screen_button() -> Button:
	var container := EditorInterface.get_base_control().find_child(BUTTONS_CONTAINER, true, false)
	if container == null:
		return null
	for child in container.get_children():
		if child is Button and (child.name == SCREEN_NAME or child.text == SCREEN_NAME):
			return child
	return null


func _set_tab_hidden(hidden: bool) -> void:
	var idx := _tab_container.get_tab_idx_from_control(_tab_control)
	if idx >= 0:
		_tab_container.set_tab_hidden(idx, hidden)


## The editor re-shows the button on feature profile changes; hide it again.
func _on_button_visibility_changed() -> void:
	if _button != null and is_instance_valid(_button) and _button.visible:
		_button.set_deferred("visible", false)


## True if the active editor feature profile disables the Script feature.
func _is_script_feature_disabled() -> bool:
	var profile_name := str(EditorInterface.get_editor_settings().get_setting("_default_feature_profile"))
	if profile_name.is_empty():
		return false
	var path := EditorInterface.get_editor_paths().get_config_dir() \
			.path_join("feature_profiles").path_join(profile_name + ".profile")
	var profile := EditorFeatureProfile.new()
	if not FileAccess.file_exists(path) or profile.load_from_file(path) != OK:
		return false
	return profile.is_feature_disabled(EditorFeatureProfile.FEATURE_SCRIPT)
