-- Neovim config used only by the terminal finder:
--   NVIM_APPNAME=tfinder-nvim nvim -u ~/terminal-browser/nvim/init.lua <file>
-- Goal: notepad-like. Opens ready to type; markdown stays rendered while you edit.
-- zellij owns Ctrl+S (scroll) and Ctrl+Q (quit zellij!), so saving/closing use Alt.

local o = vim.opt
o.number = true
o.mouse = "a"
o.wrap = true
o.linebreak = true
o.breakindent = true
o.termguicolors = true
o.signcolumn = "no"
o.scrolloff = 3
o.confirm = true          -- ask "save changes?" instead of refusing to quit
o.undofile = true         -- undo survives closing the file
o.showmode = true
o.laststatus = 2
o.statusline = " %f %m%r%=  Alt+s save  ·  Alt+q close  ·  Ctrl+z undo  %l:%c "

local map = vim.keymap.set
map({ "n", "i", "v" }, "<M-s>", "<Cmd>write<CR>", { desc = "Save" })
map({ "n", "i", "v" }, "<M-q>", "<Cmd>confirm quitall<CR>", { desc = "Close (asks to save)" })
-- Ctrl+z would suspend Neovim and freeze the preview pane; make it undo instead.
map("n", "<C-z>", "u", { desc = "Undo" })
map("i", "<C-z>", "<C-o>u", { desc = "Undo" })
map("n", "<C-y>", "<C-r>", { desc = "Redo" })
map("i", "<C-y>", "<C-o><C-r>", { desc = "Redo" })

-- Open ready to type.
vim.api.nvim_create_autocmd("VimEnter", {
	callback = function()
		if vim.bo.modifiable and not vim.bo.readonly then
			vim.cmd.startinsert()
		end
	end,
})

vim.api.nvim_create_autocmd("FileType", {
	pattern = "markdown",
	callback = function(ev)
		pcall(vim.treesitter.start, ev.buf, "markdown")
		vim.opt_local.conceallevel = 2
	end,
})

-- Nerd Font icons only when a font that has them is installed (the terminal falls back
-- to it); otherwise plain Unicode symbols that every font has. Force: TFINDER_NERD_FONT=1/0
local function have_nerd_font()
	if vim.env.TFINDER_NERD_FONT then
		return vim.env.TFINDER_NERD_FONT == "1"
	end
	if vim.fn.executable("fc-list") == 0 then
		return false
	end
	return vim.trim(vim.fn.system({ "fc-list", ":charset=f0ca1", "family" })) ~= ""
end

local function plain_symbols()
	local opts = {
		heading = { icons = { "◉ ", "○ ", "✸ ", "✿ ", "◆ ", "◇ " } },
		checkbox = {
			unchecked = { icon = "☐ " },
			checked = { icon = "☑ " },
			custom = { todo = { rendered = "◐ " } },
		},
		link = {
			footnote = { icon = "" },
			image = "▣ ",
			email = "✉ ",
			hyperlink = "↗ ",
			wiki = { icon = "↗ " },
			custom = {},
		},
		callout = {},
	}
	local ok, defaults = pcall(require, "render-markdown.settings")
	if ok then
		for name in pairs(defaults.link.default.custom or {}) do
			opts.link.custom[name] = { icon = "↗ " }
		end
		for name, c in pairs(defaults.callout.default or {}) do
			if type(c) == "table" and type(c.rendered) == "string" then
				opts.callout[name] = { rendered = (c.rendered:gsub("^[^%w%s]+%s", "")) } -- drop the icon
			end
		end
	end
	return opts
end

-- Render markdown in place: headings, bullets, checkboxes, tables, code blocks.
-- Only the line under the cursor shows raw markdown — in every mode, including insert.
local ok, render_markdown = pcall(require, "render-markdown")
if ok then
	local opts = {
		file_types = { "markdown" },
		render_modes = { "n", "c", "t", "i" },
		anti_conceal = { enabled = true },
		-- These need extra tree-sitter parsers we don't install; off = no warnings, no wasted work.
		html = { enabled = false },
		latex = { enabled = false },
		yaml = { enabled = false },
	}
	if not have_nerd_font() then
		opts = vim.tbl_deep_extend("force", opts, plain_symbols())
	end
	render_markdown.setup(opts)
end
