-- Finder mouse behaviour (overrides yazi's default click handler, finder only):
--   click a folder                → go into it
--   click a file                  → select it (the right pane previews it)
--   double-click / right-click a file → edit it in the right pane
--   click empty space in the parent column → go up (yazi default)
-- Stock yazi would run its own opener on right-click, launching an editor inside this pane.

local DOUBLE_CLICK = 0.4 -- seconds
local last = { url = nil, at = 0, entered_at = 0 }

local function edit_in_right_pane(url)
	local send = os.getenv("TFINDER_HOME") .. "/bin/send"
	ya.emit("shell", { ya.quote(send) .. " edit " .. ya.quote(url), orphan = true })
end

function Entity:click(event, up)
	if up or event.is_middle then
		return
	end
	local now = ya.time()
	-- Second click of a double-click on a folder: the folder is already open, and
	-- this click would hit whatever item now sits under the mouse — ignore it.
	if now - last.entered_at < DOUBLE_CLICK then
		return
	end

	local file = self._file
	local url = tostring(file.url)
	local double = last.url == url and now - last.at < DOUBLE_CLICK
	last.url, last.at = url, now

	ya.emit("reveal", { file.url })
	if file.cha.is_dir then
		ya.emit("enter", {})
		last.entered_at = now
	elseif event.is_right or double then
		edit_in_right_pane(url)
	end
end
