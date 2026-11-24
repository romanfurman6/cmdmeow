# dmgbuild settings for CmdMeow.dmg (used by scripts/make-dmg.sh).
# Icon centres sit at the two ends of the arrow drawn in assets/dmg/background.png.
import os.path

app = defines["app"]  # noqa: F821  (dmgbuild passes -D app=...)
app_name = os.path.basename(app)

format = "UDZO"
filesystem = "HFS+"
files = [app]
symlinks = {"Applications": "/Applications"}
hide_extensions = [app_name]

icon = defines.get("volume_icon")  # noqa: F821
background = defines["background"]  # noqa: F821

# 660x440 pt of picture; Finder's bounds include the 28 pt title bar.
window_rect = ((200, 140), (660, 440 + 28))
default_view = "icon-view"
show_status_bar = False
show_tab_view = False
show_toolbar = False
show_pathbar = False
show_sidebar = False
show_icon_preview = False
include_icon_view_settings = True

icon_size = 128
text_size = 13
label_pos = "bottom"
arrange_by = None
icon_locations = {
    app_name: (150, 180),
    "Applications": (505, 180),
}
