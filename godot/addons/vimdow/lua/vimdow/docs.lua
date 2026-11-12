-- Open Godot's native documentation for the symbol under the cursor.
--
-- Neovim only gathers *hints* (the receiver type reported by Godot's own GDScript
-- language server and the script's `extends` class). Godot's ClassDB, on the editor
-- side, decides whether the symbol exists and which reference page documents it, and
-- the page is shown by Godot's native class reference viewer.

local M = {}

local PREFIX = "godot-doc://"
local MENU_ENTRY = "PopUp.Open\\ in\\ Documentation"

--- Base class of the current script, from its `extends` line.
local function script_base_class(bufnr)
	for _, line in ipairs(vim.api.nvim_buf_get_lines(bufnr, 0, 50, false)) do
		local base = line:match "^%s*extends%s+([%a_][%w_]*)"
		if base then
			return base
		end
	end
end

--- Capitalised identifiers in Godot's hover text for the symbol under the cursor
--- (e.g. "Node2D", "Vector2" for `position`). They are only candidates: the editor
--- validates each one against ClassDB.
local function hover_class_hints(bufnr)
	local hints, seen = {}, {}
	local client = vim.lsp.get_clients({ bufnr = bufnr, name = "gdscript" })[1]
	if not client then
		return hints
	end

	local params = vim.lsp.util.make_position_params(0, client.offset_encoding)
	local res = client:request_sync("textDocument/hover", params, 500, bufnr)
	local contents = res and res.result and res.result.contents
	if not contents then
		return hints
	end

	local text = table.concat(vim.lsp.util.convert_input_to_markdown_lines(contents), "\n")
	for ident in text:gmatch "%u[%w_]*" do
		if not seen[ident] and #hints < 10 then
			seen[ident] = true
			table.insert(hints, ident)
		end
	end
	return hints
end

-- Documents ------------------------------------------------------------------
-- A reference page is an ordinary listed Neovim buffer (`godot-doc://Node`), so it is
-- opened, switched, split, closed and listed with the normal buffer/window/tab commands.
-- Godot's native page is drawn by the editor over the window that shows the buffer
-- (see `VimdowDocs.apply_layout`). The buffer's lines mirror the page's text: one buffer
-- line per wrapped line, and one per code line of a code block (`M.set_lines`). The editor
-- scrolls the page to the cursor's line. The page shows no cursor of its own: Godot's
-- `RichTextLabel` cannot report where a character is, so one cannot be drawn reliably.

local function notify(method, payload)
	pcall(vim.rpcnotify, 1, method, payload)
end

local function find_buffer(name)
	for _, buf in ipairs(vim.api.nvim_list_bufs()) do
		if vim.b[buf].vimdow_doc == name then
			return buf
		end
	end
end

--- Tells the editor where every visible document window is, in grid cells, and which
--- line its cursor is on (the page scrolls to it).
function M.sync()
	local entries, seen = {}, {}
	local wins = vim.api.nvim_tabpage_list_wins(0)
	-- the current window wins if the same page is shown twice
	table.sort(wins, function(a, b)
		return a == vim.api.nvim_get_current_win() and b ~= a
	end)
	for _, win in ipairs(wins) do
		local name = vim.b[vim.api.nvim_win_get_buf(win)].vimdow_doc
		if name and not seen[name] and vim.api.nvim_win_get_config(win).relative == "" then
			seen[name] = true
			local pos = vim.api.nvim_win_get_position(win)
			local cursor = vim.api.nvim_win_get_cursor(win)
			table.insert(entries, {
				name = name,
				line = cursor[1],
				row = pos[1],
				col = pos[2],
				width = vim.api.nvim_win_get_width(win),
				height = vim.api.nvim_win_get_height(win),
			})
		end
	end
	notify("vimdow_docs_layout", { entries = entries })
end

local sync_pending = false
local function schedule_sync()
	if sync_pending then
		return
	end
	sync_pending = true
	vim.schedule(function()
		sync_pending = false
		M.sync()
	end)
end

--- Opens (or switches to) the document buffer of a class reference page.
function M.open_page(name)
	local buf = find_buffer(name)
	if not buf then
		buf = vim.api.nvim_create_buf(true, true)
		vim.api.nvim_buf_set_name(buf, PREFIX .. name)
		vim.b[buf].vimdow_doc = name
		vim.bo[buf].buftype = "nofile"
		vim.bo[buf].bufhidden = "hide"
		vim.bo[buf].swapfile = false
		vim.bo[buf].filetype = "godotdoc"
		vim.bo[buf].modifiable = false
	else
		vim.bo[buf].buflisted = true
	end

	-- like opening a file: show it in the current window; the previous buffer stays loaded
	for _, win in ipairs(vim.api.nvim_tabpage_list_wins(0)) do
		if vim.api.nvim_win_get_buf(win) == buf then
			vim.api.nvim_set_current_win(win)
			schedule_sync()
			return
		end
	end
	vim.api.nvim_win_set_buf(0, buf)
	schedule_sync()
end

--- Replaces the buffer's lines with the page's current visual lines.
function M.set_lines(name, lines)
	local buf = find_buffer(name)
	if not buf then
		return
	end

	-- Replacing every line can move the cursor; remember it and put it back (clamped).
	local cursors = {}
	for _, win in ipairs(vim.fn.win_findbuf(buf)) do
		cursors[win] = vim.api.nvim_win_get_cursor(win)
	end

	vim.bo[buf].modifiable = true
	vim.api.nvim_buf_set_lines(buf, 0, -1, false, lines)
	vim.bo[buf].modifiable = false
	vim.bo[buf].modified = false

	local last = vim.api.nvim_buf_line_count(buf)
	for win, cursor in pairs(cursors) do
		if vim.api.nvim_win_is_valid(win) then
			pcall(vim.api.nvim_win_set_cursor, win, { math.max(1, math.min(cursor[1], last)), cursor[2] })
		end
	end
	schedule_sync()
end

local function setup_documents()
	local group = vim.api.nvim_create_augroup("VimdowDocsLayout", { clear = true })
	vim.api.nvim_create_autocmd(
		{ "BufWinEnter", "WinEnter", "WinNew", "WinClosed", "WinResized", "VimResized", "TabEnter", "CursorMoved" },
		{ group = group, callback = schedule_sync }
	)
	vim.api.nvim_create_autocmd({ "BufDelete", "BufWipeout" }, {
		group = group,
		callback = function(args)
			local name = vim.b[args.buf].vimdow_doc
			if name then
				notify("vimdow_docs_close", { name = name })
			end
			schedule_sync()
		end,
	})
end

--- Opens the documentation of `word` (default: the word under the cursor).
function M.open(word)
	word = (word and word ~= "") and word or vim.fn.expand "<cword>"
	if word == "" then
		return
	end

	local bufnr = vim.api.nvim_get_current_buf()
	local contexts = hover_class_hints(bufnr)
	local base = script_base_class(bufnr)
	if base then
		table.insert(contexts, base)
	end

	local ok, response = pcall(vim.fn.rpcrequest, 1, "VimdowDocs:open_symbol", word, contexts)
	if not ok then
		vim.notify("Vimdow: documentation request failed: " .. tostring(response), vim.log.levels.WARN)
	elseif type(response) ~= "table" or response.return_value ~= true then
		vim.notify(("Vimdow: no Godot documentation found for '%s'"):format(word), vim.log.levels.INFO)
	end
end

function M.setup(keybinds)
	keybinds = keybinds or {}
	setup_documents()

	vim.api.nvim_create_user_command("VimdowDocs", function(o)
		M.open(o.fargs[1])
	end, { nargs = "?", desc = "open Godot documentation for a symbol" })

	vim.keymap.set("n", keybinds.open_docs or "<leader>hd", function()
		M.open()
	end, { desc = "open Godot documentation for the symbol under the cursor" })

	-- Right click context menu: extends Neovim's own PopUp menu, nothing is replaced.
	vim.cmd("amenu " .. MENU_ENTRY .. " <Cmd>VimdowDocs<CR>")
	vim.api.nvim_create_autocmd("MenuPopup", {
		group = vim.api.nvim_create_augroup("VimdowDocsMenu", { clear = true }),
		callback = function()
			local state = vim.bo.filetype == "gdscript" and "enable" or "disable"
			vim.cmd("amenu " .. state .. " " .. MENU_ENTRY)
		end,
	})
end

return M
