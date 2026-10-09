-- Dedicated login compositor: no user shell, terminal binding or desktop autostart.
hl.monitor({ output = "", mode = "preferred", position = "auto", scale = 1 })
hl.config({
  input = { kb_layout = "us" },
  animations = { enabled = false },
  misc = { disable_hyprland_logo = true, disable_splash_rendering = true },
})
hl.on("hyprland.start", function()
  hl.exec_cmd("/usr/local/lib/lucent/run-greeter")
end)
