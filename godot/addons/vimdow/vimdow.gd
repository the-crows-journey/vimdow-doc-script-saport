@tool
extends EditorPlugin

const MAIN_SCREEN_NAME = "Vimdow"
const EDITOR = preload("res://addons/vimdow/vimdow_editor.tscn")

var _last_main_screen: String = ""
var editor: VimdowEditor
var window_wrapper: Window

var debugger: VimdowDebugger
var docs: VimdowDocs
## Hides the built-in Script Editor's main-screen tab (the feature itself stays enabled).
var script_tab: VimdowScriptTab

var pop_out_shortcut: Shortcut
var focus_shortcut: Shortcut

const DEFAULT_SETTINGS = {
	"auto_open_root_script": true,
	"auto_focus_on_scene_change": true,
}

func _enter_tree() -> void:
	if DisplayServer.get_name() == "headless": # CI/CD sanity check
		print_debug("Skipping plugin initilization")
		return

	var editor_settings := EditorInterface.get_editor_settings()
	for setting in DEFAULT_SETTINGS:
		var full_setting = "vimdow/" + setting
		if not editor_settings.has_setting(full_setting):
			editor_settings.set_setting(full_setting, DEFAULT_SETTINGS[setting])
		editor_settings.set_initial_value(full_setting, DEFAULT_SETTINGS[setting], false)

	editor = EDITOR.instantiate()
	editor.conf_path = "res://addons/vimdow/local.cfg"
	EditorInterface.get_editor_main_screen().add_child(editor)
	editor.client.neovim_request.connect(_on_neovim_request)

	# Hosts Godot's native class reference pages (F1, "Open Documentation", doc links)
	docs = VimdowDocs.new()
	editor.add_child(docs)
	docs.attach(MAIN_SCREEN_NAME)
	docs.page_adopted.connect(_on_doc_page_adopted)
	docs.page_text_changed.connect(_on_doc_text_changed)
	editor.client.neovim_event.connect(_on_neovim_event)
	editor.call_deferred("start", PackedStringArray(["-S", "addons/vimdow/lua/start.lua"]))

	get_viewport().gui_focus_changed.connect(_on_gui_focus_changed)

	window_wrapper = Window.new()
	add_child(window_wrapper)
	window_wrapper.hide()
	window_wrapper.close_requested.connect(_on_editor_window_close)

	pop_out_shortcut = Shortcut.new()
	var poev = InputEventKey.new()
	poev.pressed = true
	poev.ctrl_pressed = true
	poev.shift_pressed = true
	poev.keycode = KEY_SPACE
	pop_out_shortcut.events = [poev]

	EditorInterface.get_editor_settings() \
			.add_shortcut("vimdow/pop_out_window", pop_out_shortcut)

	focus_shortcut = Shortcut.new()
	var fev = InputEventKey.new()
	fev.pressed = true
	fev.ctrl_pressed = true
	fev.alt_pressed = true
	fev.keycode = KEY_V
	focus_shortcut.events = [fev]

	EditorInterface.get_editor_settings() \
			.add_shortcut("vimdow/focus", focus_shortcut)

	_make_visible(false)

	main_screen_changed.connect(_on_main_screen_changed)
	scene_changed.connect(_on_scene_changed)

	debugger = VimdowDebugger.new()
	debugger.setup(editor)
	add_debugger_plugin(debugger)

	# The Script Editor feature stays enabled (the Scene dock's native Attach / Remove /
	# Extend Script buttons depend on it); only its main-screen tab is hidden. Scripts are
	# opened by `_handles` / `_edit` below, which Godot prefers over the Script Editor's.
	script_tab = VimdowScriptTab.new()
	script_tab.hide_tab()
	if script_tab.is_selected():
		_show_vimdow.call_deferred()

## Switches the main screen to Vimdow.
func _show_vimdow() -> void:
	EditorInterface.set_main_screen_editor(MAIN_SCREEN_NAME)
	if editor:
		editor.grab_focus()

func _on_gui_focus_changed(control: Control):
	# Script editor steals focus when making edits to an already existing script
	# so the VimdowEditor needs to steal it back
	var se := EditorInterface.get_script_editor()
	if editor.visible and se.is_ancestor_of(control):
		editor.grab_focus()

## A native documentation page arrived: open it as a document (buffer) in Neovim.
func _on_doc_page_adopted(page_name: String) -> void:
	editor.client.request(
		"nvim_exec_lua",
		["Vimdow.docs.open_page(...)", [page_name]],
	)
	editor.grab_focus()

## The rendered lines of a page changed: mirror them into its Neovim buffer.
func _on_doc_text_changed(page_name: String, lines: PackedStringArray) -> void:
	editor.client.request(
		"nvim_exec_lua",
		["Vimdow.docs.set_lines(...)", [page_name, Array(lines)]],
	)

## Neovim -> editor notifications about documentation documents.
func _on_neovim_event(method: String, params: Array) -> void:
	var m := method.lstrip('"').rstrip('"')
	if m == "vimdow_saved":
		_request_editor_save()
		return
	if not m.begins_with("vimdow_docs_") or params.is_empty() or not params[0] is Dictionary:
		return
	var data: Dictionary = params[0]
	match m:
		"vimdow_docs_layout":
			var origin: Vector2 = editor.vimdow_window.global_position - docs.global_position
			docs.apply_layout(data.get("entries", []), editor.get_cell_size(), origin)
		"vimdow_docs_close":
			docs.close_page(str(data.get("name", "")))

## Neovim wrote a file (any :w/:write/:wq/:x, mapping or command): also save Godot's state.
## Several writes in one frame (e.g. :wa) are coalesced into a single editor save.
var _editor_save_pending := false

func _request_editor_save() -> void:
	if _editor_save_pending:
		return
	_editor_save_pending = true
	_save_editor_state.call_deferred()

func _save_editor_state() -> void:
	_editor_save_pending = false
	var root := EditorInterface.get_edited_scene_root()
	# Unsaved, never-named scenes would need a Save As dialog: leave those alone.
	if root == null or root.scene_file_path.is_empty():
		return
	var err := EditorInterface.save_scene()
	if err != OK:
		var msg := "Vimdow: Godot could not save the scene (%s)" % error_string(err)
		push_warning(msg)
		editor.client.request(
			"nvim_exec_lua",
			["vim.notify(..., vim.log.levels.WARN)", [msg]],
		)

func _on_neovim_request(msgid: int, method: String, params: Array):
	var m := method.lstrip('"').rstrip('"')
	if m.begins_with("EditorInterface:"):
		var ei_method = m.trim_prefix("EditorInterface:")
		if EditorInterface.has_method(ei_method):
			editor.client.respond(
				msgid,
				null,
				{
					return_value = EditorInterface.callv(ei_method, params),
				},
			)
		else:
			editor.client.respond(
				msgid,
				"EditorInterface does not have a method called: %s",
				null,
			)
	elif m.begins_with("VimdowDocs:"):
		var ok := false
		if m == "VimdowDocs:open_symbol" and params.size() >= 2:
			var contexts := PackedStringArray()
			if params[1] is Array:
				contexts = PackedStringArray(params[1])
			ok = docs.open_symbol(str(params[0]), contexts)
		editor.client.respond(msgid, null, { return_value = ok })


func _exit_tree() -> void:
	if editor:
		# Godot/plugin teardown: Neovim's exit from here on must not auto-restart.
		editor.shutting_down = true
	if scene_changed.is_connected(_on_scene_changed):
		scene_changed.disconnect(_on_scene_changed)
	if script_tab:
		script_tab.restore_tab()
	if docs:
		docs.detach()
	if editor:
		editor.client.kill_process()
		editor.queue_free()
	if window_wrapper:
		window_wrapper.queue_free()
	if debugger:
		remove_debugger_plugin(debugger)


func _input(event: InputEvent) -> void:
	if event.is_pressed() and focus_shortcut.matches_event(event):
		get_viewport().set_input_as_handled()
		EditorInterface.set_main_screen_editor(MAIN_SCREEN_NAME)
		return

	if editor.visible \
			and event.is_pressed() \
			and pop_out_shortcut.matches_event(event):
		get_viewport().set_input_as_handled()
		var ms := EditorInterface.get_editor_main_screen()
		if not window_wrapper.visible:
			window_wrapper.show()
			ms.remove_child(editor)
			window_wrapper.add_child(editor)
			editor.lock_to_window(window_wrapper)
			editor.grab_focus()


func _on_editor_window_close():
	editor.unlock_from_window()
	window_wrapper.remove_child(editor)
	window_wrapper.hide()

	EditorInterface.get_editor_main_screen().add_child(editor)
	EditorInterface.set_main_screen_editor(MAIN_SCREEN_NAME)


func _on_main_screen_changed(screen_name: String):
	# Something (a shortcut, an old layout where the tab could not be hidden) went to the
	# built-in Script Editor: Vimdow is the scripting interface, go back to it.
	if screen_name == VimdowScriptTab.SCREEN_NAME:
		_show_vimdow.call_deferred()
		return
	if screen_name != _get_plugin_name():
		_last_main_screen = screen_name


func _on_scene_changed(scene_root: Node) -> void:
	var editor_settings := EditorInterface.get_editor_settings()
	if not editor_settings.get_setting("vimdow/auto_open_root_script"):
		return
	if scene_root == null:
		return
	var script = scene_root.get_script()
	if script and script.resource_path.begins_with("res://"):
		editor.open_file(ProjectSettings.globalize_path(script.resource_path))
		if editor_settings.get_setting("vimdow/auto_focus_on_scene_change"):
			editor.grab_focus()


func _handles(object: Object) -> bool:
	return [
		&"Script",
		&"TextFile",
		&"GDExtension",
		&"JSON",
	].any(
		func(c):
			return ClassDB.is_parent_class(object.get_class(), c)
	)


func _edit(object: Object):
	if object == null:
		return

	editor.open_file(ProjectSettings.globalize_path(object.resource_path))
	editor.grab_focus()


func _has_main_screen() -> bool:
	return true


func _get_plugin_icon() -> Texture2D:
	return preload("res://addons/vimdow/images/vimdow_icon.png")


func _make_visible(visible: bool) -> void:
	if editor:
		editor.visible = window_wrapper.visible or visible
	if window_wrapper and window_wrapper.visible:
		_focus_last_editor()
		window_wrapper.show()
	if visible:
		editor.grab_focus()


func _focus_last_editor():
	if window_wrapper.visible:
		assert(not _last_main_screen.is_empty())
		EditorInterface.get_base_control().get_viewport().gui_release_focus()
		EditorInterface.set_main_screen_editor(_last_main_screen)


func _get_plugin_name() -> String:
	return MAIN_SCREEN_NAME
