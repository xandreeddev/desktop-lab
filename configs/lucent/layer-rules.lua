-- Lucent animates its own dock-to-launcher shape and keeps its bottom edge fixed.
-- The compositor places/resizes that layer without applying a second animation.
-- Ordinary application window animations and geometry retain their configuration.
hl.layer_rule({
  name = "lucent-native-dock-motion",
  match = { namespace = "^lucent-dock$" },
  no_anim = true,
})
