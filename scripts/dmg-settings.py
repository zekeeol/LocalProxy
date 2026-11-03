"""Reuse the former electron-builder DMG defaults for the native app."""

application = defines["app"]
files = [application]
symlinks = {"Applications": "/Applications"}
icon = defines["icon"]
background = defines["background"]
format = "UDZO"

# The old build specified the LocalProxy volume title and inherited these
# electron-builder defaults: a 540×380 background, 80 px icons, and a 12 pt
# label with the app at (130, 220) and Applications at (410, 220).
window_rect = ((400, 530), (540, 380))
default_view = "icon-view"
icon_locations = {"LocalProxy.app": (130, 220), "Applications": (410, 220)}
icon_size = 80
text_size = 12
show_status_bar = False
show_toolbar = False
show_pathbar = False
show_sidebar = False
