--- @sync entry
-- Glue between yazi (left pane) and preview.py (right pane).
--   plugin tfinder edit  → folder: enter it; file: edit it in the right pane
--   plugin tfinder open  → open the hovered file with the desktop app
return {
	entry = function(_, job)
		local action = job.args[1] or "edit"
		local h = cx.active.current.hovered
		if not h then
			return
		end
		if action == "edit" and h.cha.is_dir then
			ya.emit("enter", {})
			return
		end
		local send = os.getenv("TFINDER_HOME") .. "/bin/send"
		ya.emit("shell", {
			ya.quote(send) .. " " .. action .. " " .. ya.quote(tostring(h.url)),
			orphan = true,
		})
	end,
}
